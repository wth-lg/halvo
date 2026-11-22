#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use halvo_control::paths::{AccountLookup, DataDir, LoginItemDir, PathError, ScratchDir};
#[cfg(target_os = "macos")]
use halvo_control::service::{
    install_login_item, login_item_path, login_item_state, InstallOutcome, LoginItemSpec,
    LoginItemState, SpecError, LOGIN_ITEM_LABEL,
};

#[cfg(target_os = "macos")]
use crate::commands::capture::LoginItemWriter;
#[cfg(target_os = "macos")]
use crate::launch::Exit;
#[cfg(target_os = "macos")]
use crate::poller::Views;

#[cfg(target_os = "macos")]
/// The sentence that a person reads where Halvo runs under an AppTranslocation path, and so
/// writes no login item.
pub const MOVE_TO_APPLICATIONS: &str = "Halvo cannot start at login while macOS runs it from a temporary location. Move Halvo to your Applications folder, open it from there, and try again.";

#[cfg(target_os = "macos")]
/// The exit status with which the system's service manager answers a print of a label that
/// launchd does not hold.
pub const NOT_LOADED: i32 = 113;

#[cfg(target_os = "macos")]
/// Runs the system's service manager with the arguments that the manager builds and answers its
/// exit status alone: none where it did not start, ended by a signal or outlived its wait.
pub trait Runner {
    fn status(&self, args: &[String]) -> Option<i32>;
}

#[cfg(target_os = "macos")]
/// Why the manager writes no login item for a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Translocated,
    Spec(SpecError),
}

#[cfg(target_os = "macos")]
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Translocated => f.write_str(MOVE_TO_APPLICATIONS),
            Refusal::Spec(error) => error.fmt(f),
        }
    }
}

#[cfg(target_os = "macos")]
/// The app's one manager of its login item. It takes the label, the program, the login item's
/// directory and, for a scratch run, the data directory as values that main resolves, and
/// writes through the one writer of halvo-control; a write starts no process.
#[derive(Debug)]
pub struct LoginItem {
    spec: LoginItemSpec,
    dir: LoginItemDir,
}

#[cfg(target_os = "macos")]
impl LoginItem {
    /// Refuses a program under an AppTranslocation path, before any read or write of the login
    /// item.
    pub fn new(
        label: &str,
        program: &Path,
        dir: LoginItemDir,
        data_dir: Option<&DataDir>,
    ) -> Result<Self, Refusal> {
        let spec = LoginItemSpec::new(label, program, data_dir).map_err(|error| match error {
            SpecError::Translocated => Refusal::Translocated,
            other => Refusal::Spec(other),
        })?;
        Ok(Self { spec, dir })
    }

    /// Writes the plist through `install_login_item`, atomically with mode 0644.
    pub fn write(&self) -> std::io::Result<InstallOutcome> {
        install_login_item(&self.spec, &self.dir)
    }

    /// Whether launchd holds the label, read through `runner` from the exit status alone: true
    /// for 0, false for NOT_LOADED while the plist stands as the manager writes it; none where no
    /// such plist stands and for any other answer. The domain's uid is that plist's owner, which
    /// `login_item_state` reads as Current only where it is the effective user.
    pub fn load(&self, runner: &dyn Runner) -> Option<bool> {
        if login_item_state(&self.spec, &self.dir).ok()? != LoginItemState::Current {
            return None;
        }
        let plist = std::fs::symlink_metadata(login_item_path(&self.spec, &self.dir)).ok()?;
        let uid = std::os::unix::fs::MetadataExt::uid(&plist);
        let args = [
            "print".to_owned(),
            format!("gui/{uid}/{}", self.spec.label()),
        ];
        match runner.status(&args)? {
            0 => Some(true),
            NOT_LOADED => Some(false),
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
/// The scratch form `--write-login-item --login-label LABEL --data-dir DIR`: writes DIR/LABEL.plist
/// through the manager, with the label and the data directory replaced, and answers the exit of
/// the launch: 0 for a plist written or already standing; 64 for the real label, for a label that
/// the writer refuses and for a DIR that a scratch run may not take, the default data directory
/// among them; 1 where nothing was written for another cause. It prints a refusal to stderr.
pub fn write_scratch(
    label: &str,
    data_dir: &Path,
    program: std::io::Result<PathBuf>,
    account: &dyn AccountLookup,
) -> Exit {
    if label.eq_ignore_ascii_case(LOGIN_ITEM_LABEL) {
        eprintln!(
            "halvo: the scratch form does not take the label {LOGIN_ITEM_LABEL} of the real login item"
        );
        return Exit::Usage;
    }
    let dirs = DataDir::from_flag(data_dir).and_then(|flag| {
        let scratch = ScratchDir::new(data_dir, account)?;
        Ok((flag, LoginItemDir::scratch_form(&scratch, account)?))
    });
    let (flag, dir) = match dirs {
        Ok(dirs) => dirs,
        Err(error) => {
            eprintln!(
                "halvo: the scratch form does not take {}: {error}",
                data_dir.display()
            );
            return refused_dir(error);
        }
    };
    let program = match program {
        Ok(program) => program,
        Err(error) => {
            eprintln!("halvo: the app's own path cannot be read: {error}");
            return Exit::LoginItemNotWritten;
        }
    };
    let item = match LoginItem::new(label, &program, dir, Some(&flag)) {
        Ok(item) => item,
        Err(refusal) => {
            eprintln!("halvo: {refusal}");
            return match refusal {
                Refusal::Spec(SpecError::Label) => Exit::Usage,
                Refusal::Translocated | Refusal::Spec(_) => Exit::LoginItemNotWritten,
            };
        }
    };
    match item.write() {
        Ok(_) => Exit::Quit,
        Err(error) => {
            eprintln!("halvo: the login item was not written: {error}");
            Exit::LoginItemNotWritten
        }
    }
}

#[cfg(target_os = "macos")]
/// What a start of `run` does with the login item: with --data-dir (`given`), or on a data
/// directory that does not read provisioned, nothing. Otherwise the manager that `item` makes for
/// the app's own path reads the plist's state, and writes it again where it reads Stale; Absent
/// and Current write nothing, and a program that the manager refuses writes nothing. Then the
/// load state that the manager reads through `runner` stands in every view of `views`.
pub fn at_start(
    views: &Views,
    data_dir: &DataDir,
    given: bool,
    item: impl FnOnce() -> Option<LoginItem>,
    runner: &dyn Runner,
) {
    if given || !matches!(data_dir.is_provisioned(), Ok(true)) {
        return;
    }
    let Some(item) = item() else {
        return;
    };
    if let Ok(LoginItemState::Stale) = login_item_state(&item.spec, &item.dir) {
        let _ = item.write();
    }
    if let Some(loaded) = item.load(runner) {
        let _ = views.login_item_loaded.set(loaded);
    }
}

#[cfg(target_os = "macos")]
/// The login item that the consent click writes (contracts.md C14 item 7): none with
/// --data-dir (`given`); otherwise a writer that writes the plist through the manager that
/// `item` makes for the app's own path and answers whether it stands written, false where the
/// manager refuses the program or the write fails. It runs no process.
pub fn at_the_click(
    given: bool,
    item: impl Fn() -> Option<LoginItem> + Send + Sync + 'static,
) -> Option<LoginItemWriter> {
    if given {
        return None;
    }
    Some(std::sync::Arc::new(move || {
        item().is_some_and(|item| item.write().is_ok())
    }))
}

#[cfg(target_os = "macos")]
fn refused_dir(error: PathError) -> Exit {
    match error {
        PathError::InsideDataDir
        | PathError::AboveDataDir
        | PathError::InsideLaunchAgents
        | PathError::Unresolved
        | PathError::Relative
        | PathError::NotNormal
        | PathError::NotUtf8 => Exit::Usage,
        PathError::Lookup(_)
        | PathError::Unset
        | PathError::Empty
        | PathError::SocketPathTooLong => Exit::LoginItemNotWritten,
    }
}

/// The Windows half (ADR 0007): the value Halvo under the Run key of the person's own registry
/// starts the app with --background at each logon. Its calls reach the registry through
/// `RunValue`: halvo-control's calls on Windows, a stand-in in the tests, so that no test
/// writes a registry.
#[cfg(any(windows, test))]
pub mod run_value {
    use std::sync::Arc;

    use halvo_control::paths::DataDir;
    use halvo_control::service::{InstallOutcome, RemoveOutcome, StartValueSpec};

    use crate::commands::capture::LoginItemWriter;
    use crate::poller::Views;

    /// The calls of the Run value.
    pub trait RunValue: Send + Sync {
        /// Reads the value of `spec`: none where it does not stand.
        fn read(&self, spec: &StartValueSpec) -> std::io::Result<Option<String>>;

        /// Writes the value of `spec` where it does not stand as `spec` makes it.
        fn write(&self, spec: &StartValueSpec) -> std::io::Result<InstallOutcome>;

        /// Takes the value of `spec` away.
        fn remove(&self, spec: &StartValueSpec) -> std::io::Result<RemoveOutcome>;
    }

    /// The person's own registry, through halvo-control's calls.
    #[cfg(windows)]
    pub struct Registry;

    /// The code with which the registry refuses to read a value of another type as a string.
    #[cfg(windows)]
    const ERROR_UNSUPPORTED_TYPE: i32 = 1630;

    #[cfg(windows)]
    impl RunValue for Registry {
        /// A value of another type is no value that the app's path makes: it reads as one that
        /// differs, so that the start writes the string over it, as `write_start_value` does.
        fn read(&self, spec: &StartValueSpec) -> std::io::Result<Option<String>> {
            match halvo_control::service::read_start_value(spec) {
                Err(e) if e.raw_os_error() == Some(ERROR_UNSUPPORTED_TYPE) => {
                    Ok(Some(String::new()))
                }
                read => read,
            }
        }

        fn write(&self, spec: &StartValueSpec) -> std::io::Result<InstallOutcome> {
            halvo_control::service::write_start_value(spec)
        }

        fn remove(&self, spec: &StartValueSpec) -> std::io::Result<RemoveOutcome> {
            halvo_control::service::remove_start_value(spec)
        }
    }

    /// What the start step did with the value.
    #[derive(Debug)]
    pub enum AtStart {
        /// With --data-dir, or on a data directory that does not read consented: nothing read
        /// or written.
        Nothing,
        /// A consented app whose value stood as its own path makes it.
        Kept,
        /// A consented app whose value differed or stood not: written again.
        Written,
        /// An app with no consent: its value taken away, or none stood.
        Removed,
        /// A call that failed.
        Failed(Failed),
    }

    /// A call of the start step that failed. Every view of a consented app then says that the
    /// login item is not written, and never that it is not registered.
    #[derive(Debug)]
    pub enum Failed {
        /// The app's own path cannot be read or makes no value.
        NoPath,
        Read(std::io::Error),
        Write(std::io::Error),
        Remove(std::io::Error),
    }

    impl std::fmt::Display for Failed {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Failed::NoPath => f.write_str("the start value Halvo has no path of the app"),
                Failed::Read(e) => write!(f, "the start value Halvo could not be read: {e}"),
                Failed::Write(e) => write!(f, "the start value Halvo could not be written: {e}"),
                Failed::Remove(e) => {
                    write!(f, "the start value Halvo could not be taken away: {e}")
                }
            }
        }
    }

    /// The start step of `run` on Windows, which main runs before the app is built, so that no
    /// press on Start meets it: with --data-dir (`given`), or on a data directory that reads its
    /// own cause, nothing. A consented app, whose data directory reads provisioned, reads the
    /// value of its own path, which `spec` makes, and writes it where it differs or stands not;
    /// where one of those calls fails, the step answers that error and every view of `views`
    /// says that the login item is not written. An app that finds no consent takes the value
    /// away; a removal that fails is the step's error alone, as the page then shows the Start
    /// step and no line on the login item.
    pub fn at_start(
        views: &Views,
        data_dir: &DataDir,
        given: bool,
        spec: impl FnOnce() -> Option<StartValueSpec>,
        values: &dyn RunValue,
    ) -> AtStart {
        if given {
            return AtStart::Nothing;
        }
        match data_dir.is_provisioned() {
            Ok(true) => {
                let answer = rewrite(spec(), values);
                if let AtStart::Failed(_) = answer {
                    views.hold().login_item_written(false);
                }
                answer
            }
            Ok(false) => take_away(spec(), values),
            Err(_) => AtStart::Nothing,
        }
    }

    /// Takes the value of `spec` away.
    fn take_away(spec: Option<StartValueSpec>, values: &dyn RunValue) -> AtStart {
        let Some(spec) = spec else {
            return AtStart::Failed(Failed::NoPath);
        };
        match values.remove(&spec) {
            Ok(_) => AtStart::Removed,
            Err(e) => AtStart::Failed(Failed::Remove(e)),
        }
    }

    /// Reads the value of `spec` and writes it where it differs or stands not.
    fn rewrite(spec: Option<StartValueSpec>, values: &dyn RunValue) -> AtStart {
        let Some(spec) = spec else {
            return AtStart::Failed(Failed::NoPath);
        };
        match values.read(&spec) {
            Err(e) => AtStart::Failed(Failed::Read(e)),
            Ok(Some(stored)) if stored == spec.data() => AtStart::Kept,
            Ok(_) => match values.write(&spec) {
                Ok(_) => AtStart::Written,
                Err(e) => AtStart::Failed(Failed::Write(e)),
            },
        }
    }

    /// The writer that the press on Start calls first (contracts.md C14 item 7): none with
    /// --data-dir (`given`); otherwise one that writes the value for the app's own path, which
    /// `spec` reads at the press, through `values`, and answers whether it stands written: false
    /// where the path makes no value or the write fails. Nothing is read or written before that
    /// press.
    pub fn at_the_click(
        given: bool,
        spec: impl Fn() -> Option<StartValueSpec> + Send + Sync + 'static,
        values: Arc<dyn RunValue>,
    ) -> Option<LoginItemWriter> {
        if given {
            return None;
        }
        Some(Arc::new(move || {
            spec().is_some_and(|spec| values.write(&spec).is_ok())
        }))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::ErrorKind;
        use std::path::Path;
        use std::sync::Mutex;

        const PROGRAM: &str = r"C:\Users\a b\AppData\Local\Halvo\halvo.exe";

        /// A call of the stand-in, with the key, the value's name and the data of its spec.
        #[derive(Debug, Clone, PartialEq, Eq)]
        enum Call {
            Read(String, String, String),
            Write(String, String, String),
            Remove(String, String, String),
        }

        /// The calls of the Run value, recorded, each with the test's own answer; `stored` is
        /// the value that a read finds.
        #[derive(Default)]
        struct StandIn {
            calls: Mutex<Vec<Call>>,
            stored: Option<String>,
            read: Option<ErrorKind>,
            write: Option<ErrorKind>,
            remove: Option<ErrorKind>,
        }

        impl StandIn {
            fn calls(&self) -> Vec<Call> {
                self.calls.lock().expect("the record").clone()
            }

            fn record(&self, call: Call) {
                self.calls.lock().expect("the record").push(call);
            }
        }

        fn named(spec: &StartValueSpec) -> (String, String, String) {
            (
                spec.key().to_owned(),
                spec.value_name().to_owned(),
                spec.data(),
            )
        }

        fn answer<T>(refused: Option<ErrorKind>, done: T) -> std::io::Result<T> {
            refused.map_or(Ok(done), |kind| Err(kind.into()))
        }

        impl RunValue for StandIn {
            fn read(&self, spec: &StartValueSpec) -> std::io::Result<Option<String>> {
                let (key, name, data) = named(spec);
                self.record(Call::Read(key, name, data));
                answer(self.read, self.stored.clone())
            }

            fn write(&self, spec: &StartValueSpec) -> std::io::Result<InstallOutcome> {
                let (key, name, data) = named(spec);
                self.record(Call::Write(key, name, data));
                answer(self.write, InstallOutcome::Written)
            }

            fn remove(&self, spec: &StartValueSpec) -> std::io::Result<RemoveOutcome> {
                let (key, name, data) = named(spec);
                self.record(Call::Remove(key, name, data));
                let outcome = match self.stored {
                    Some(_) => RemoveOutcome::Removed,
                    None => RemoveOutcome::Absent,
                };
                answer(self.remove, outcome)
            }
        }

        fn the_app() -> Option<StartValueSpec> {
            StartValueSpec::new(Path::new(PROGRAM)).ok()
        }

        fn written() -> Call {
            Call::Write(
                r"Software\Microsoft\Windows\CurrentVersion\Run".to_owned(),
                "Halvo".to_owned(),
                format!("\"{PROGRAM}\" --background"),
            )
        }

        fn read() -> Call {
            let Call::Write(key, name, data) = written() else {
                unreachable!("written is a write")
            };
            Call::Read(key, name, data)
        }

        fn removed() -> Call {
            let Call::Write(key, name, data) = written() else {
                unreachable!("written is a write")
            };
            Call::Remove(key, name, data)
        }

        fn data_dir(name: &str, consented: bool) -> DataDir {
            let base = std::env::current_exe()
                .expect("the test binary has a path")
                .ancestors()
                .nth(3)
                .expect("the test binary sits under target/<profile>/deps")
                .join("tmp");
            let dir = DataDir::from_flag(halvo_test_dir::make(&base, name).expect("a scratch"))
                .expect("an absolute directory");
            if consented {
                std::fs::write(dir.state_file(), b"{}").expect("a state file");
            }
            dir
        }

        fn shown(views: &Views, key: &str) -> serde_json::Value {
            serde_json::to_value(views.view()).expect("the view writes")[key].clone()
        }

        #[test]
        fn the_run_value_is_written_at_the_press_alone_and_never_for_a_given_directory() {
            let values = Arc::new(StandIn::default());
            let writer = at_the_click(false, the_app, values.clone()).expect("a writer");
            assert_eq!(
                values.calls(),
                Vec::<Call>::new(),
                "nothing before the press"
            );
            assert!(writer(), "the value stands written");
            assert_eq!(values.calls(), vec![written()], "one write at the press");

            let refused = Arc::new(StandIn {
                write: Some(ErrorKind::PermissionDenied),
                ..StandIn::default()
            });
            let writer = at_the_click(false, the_app, refused.clone()).expect("a writer");
            assert!(!writer(), "a refused write answers not written");
            assert_eq!(refused.calls(), vec![written()]);

            let given = Arc::new(StandIn::default());
            assert!(
                at_the_click(true, the_app, given.clone()).is_none(),
                "no writer with --data-dir"
            );
            assert_eq!(given.calls(), Vec::<Call>::new(), "nothing with --data-dir");
        }

        #[test]
        fn a_consented_start_writes_a_value_that_differs_again_and_a_refused_read_is_an_error() {
            let consented = data_dir("run-value-consented", true);
            let own = format!("\"{PROGRAM}\" --background");
            let other = r#""D:\Halvo\halvo.exe" --background"#.to_owned();
            let refused = Some(ErrorKind::PermissionDenied);
            for (row, values, calls, written_shown) in [
                (
                    "a value that differs",
                    StandIn {
                        stored: Some(other.clone()),
                        ..StandIn::default()
                    },
                    vec![read(), written()],
                    serde_json::Value::Null,
                ),
                (
                    "no value",
                    StandIn::default(),
                    vec![read(), written()],
                    serde_json::Value::Null,
                ),
                (
                    "the app's own value",
                    StandIn {
                        stored: Some(own.clone()),
                        ..StandIn::default()
                    },
                    vec![read()],
                    serde_json::Value::Null,
                ),
                (
                    "a refused read",
                    StandIn {
                        read: refused,
                        ..StandIn::default()
                    },
                    vec![read()],
                    serde_json::Value::Bool(false),
                ),
                (
                    "a refused write",
                    StandIn {
                        stored: Some(other.clone()),
                        write: refused,
                        ..StandIn::default()
                    },
                    vec![read(), written()],
                    serde_json::Value::Bool(false),
                ),
            ] {
                let views = Views::default();
                let answer = at_start(&views, &consented, false, the_app, &values);
                let expected = match row {
                    "a value that differs" | "no value" => matches!(answer, AtStart::Written),
                    "the app's own value" => matches!(answer, AtStart::Kept),
                    "a refused read" => matches!(answer, AtStart::Failed(Failed::Read(_))),
                    _ => matches!(answer, AtStart::Failed(Failed::Write(_))),
                };
                assert!(expected, "{row}: {answer:?}");
                assert_eq!(values.calls(), calls, "{row}");
                assert_eq!(shown(&views, "login_item_written"), written_shown, "{row}");
                assert_eq!(
                    shown(&views, "login_item_loaded"),
                    serde_json::Value::Null,
                    "{row}: never not registered"
                );
            }

            let given = StandIn {
                stored: Some(other),
                ..StandIn::default()
            };
            let answer = at_start(&Views::default(), &consented, true, the_app, &given);
            assert!(matches!(answer, AtStart::Nothing), "--data-dir: {answer:?}");
            assert_eq!(given.calls(), Vec::<Call>::new(), "nothing with --data-dir");
        }

        #[test]
        fn a_start_with_no_consent_takes_the_value_away_and_a_failed_removal_is_an_error() {
            let unconsented = data_dir("run-value-unconsented", false);
            let consented = data_dir("run-value-consented-kept", true);
            let own = format!("\"{PROGRAM}\" --background");

            let values = StandIn {
                stored: Some(own.clone()),
                ..StandIn::default()
            };
            let views = Views::default();
            let answer = at_start(&views, &unconsented, false, the_app, &values);
            assert!(matches!(answer, AtStart::Removed), "no consent: {answer:?}");
            assert_eq!(values.calls(), vec![removed()], "no consent");
            assert_eq!(shown(&views, "login_item_written"), serde_json::Value::Null);

            let kept = StandIn {
                stored: Some(own),
                ..StandIn::default()
            };
            let answer = at_start(&Views::default(), &consented, false, the_app, &kept);
            assert!(matches!(answer, AtStart::Kept), "consent: {answer:?}");
            assert_eq!(
                kept.calls(),
                vec![read()],
                "a start with consent takes nothing away"
            );

            let refused = StandIn {
                remove: Some(ErrorKind::PermissionDenied),
                ..StandIn::default()
            };
            let views = Views::default();
            let answer = at_start(&views, &unconsented, false, the_app, &refused);
            assert!(
                matches!(answer, AtStart::Failed(Failed::Remove(_))),
                "a refused removal: {answer:?}"
            );
            assert_eq!(refused.calls(), vec![removed()], "a refused removal");
            assert_eq!(
                shown(&views, "login_item_written"),
                serde_json::Value::Null,
                "the page shows the Start step, with no logon line"
            );

            let given = StandIn::default();
            let answer = at_start(&Views::default(), &unconsented, true, the_app, &given);
            assert!(matches!(answer, AtStart::Nothing), "--data-dir: {answer:?}");
            assert_eq!(given.calls(), Vec::<Call>::new(), "nothing with --data-dir");
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::launch::{parse, Launch};
    use crate::view_state::StatusView;
    use halvo_control::paths::DarwinUserTempDir;
    use halvo_control::proto::Platform;
    use halvo_control::testing::sample_status;
    use serde_json::{json, Value};
    use std::ffi::OsString;
    use std::sync::Mutex;

    const PROGRAM: &str = "/Applications/Halvo.app/Contents/MacOS/halvo";
    const LABEL: &str = "dev.halvo.test.scratch";
    const OTHER_PROGRAM: &str = "/Users/Shared/Halvo.app/Contents/MacOS/halvo";
    const TRANSLOCATED: &str =
        "/private/var/folders/zz/T/AppTranslocation/0A1B2C3D/d/Halvo.app/Contents/MacOS/halvo";

    struct Recorded {
        answer: Option<i32>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl Recorded {
        fn answering(answer: Option<i32>) -> Self {
            Self {
                answer,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().expect("the record").clone()
        }
    }

    impl Runner for Recorded {
        fn status(&self, args: &[String]) -> Option<i32> {
            self.calls.lock().expect("the record").push(args.to_vec());
            self.answer
        }
    }

    struct Home(PathBuf);

    impl AccountLookup for Home {
        fn base_dir(&self) -> Result<Option<OsString>, PathError> {
            Ok(Some(self.0.clone().into_os_string()))
        }

        fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError> {
            Ok(None)
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::current_exe()
            .expect("the test binary has a path")
            .ancestors()
            .nth(3)
            .expect("the test binary sits under target/<profile>/deps")
            .join("tmp");
        halvo_test_dir::make(&base, name).expect("a scratch directory")
    }

    fn entries(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                found.extend(entries(&path));
            }
            found.push(path);
        }
        found.sort();
        found
    }

    fn agents_of(home: &Path) -> LoginItemDir {
        LoginItemDir::for_account(&Home(home.to_path_buf()))
            .expect("the scratch home's LaunchAgents")
    }

    fn plist_of(program: &str) -> String {
        LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new(program), None)
            .expect("a spec")
            .plist()
    }

    #[test]
    fn the_manager_writes_through_the_one_writer_and_refuses_a_translocated_path() {
        let home = scratch("login-item-home");
        let item = LoginItem::new(LOGIN_ITEM_LABEL, Path::new(PROGRAM), agents_of(&home), None)
            .expect("a manager");
        assert_eq!(item.write().expect("the write"), InstallOutcome::Written);
        let plist = home.join("Library/LaunchAgents/com.halvo.desktop.plist");
        assert_eq!(
            std::fs::read_to_string(&plist).expect("the plist stands"),
            plist_of(PROGRAM)
        );

        let other = scratch("login-item-translocated");
        let refused = LoginItem::new(
            LOGIN_ITEM_LABEL,
            Path::new(TRANSLOCATED),
            agents_of(&other),
            None,
        )
        .expect_err("a refusal");
        assert_eq!(refused, Refusal::Translocated);
        assert_eq!(refused.to_string(), MOVE_TO_APPLICATIONS);
        assert_eq!(entries(&other), Vec::<PathBuf>::new());
    }

    fn scratch_form(label: &str, dir: &Path, account: &Home) -> Exit {
        let args = [
            "--write-login-item",
            "--login-label",
            label,
            "--data-dir",
            dir.to_str().expect("a UTF-8 path"),
        ];
        match parse(args.map(OsString::from)) {
            Ok(Launch::WriteLoginItem { label, data_dir }) => {
                write_scratch(&label, &data_dir, Ok(PathBuf::from(PROGRAM)), account)
            }
            other => panic!("parse reads the scratch form: {other:?}"),
        }
    }

    #[test]
    fn the_scratch_form_writes_label_plist_and_exits_64_for_the_real_label_or_the_default_directory(
    ) {
        let home = scratch("login-item-scratch-home");
        let account = Home(home.clone());
        let dir = scratch("login-item-scratch-dir");
        assert_eq!(scratch_form(LABEL, &dir, &account), Exit::Quit);
        let dir_text = dir.to_str().expect("a UTF-8 path");
        let expected = plist_of(PROGRAM)
            .replace(
                "<string>com.halvo.desktop</string>",
                &format!("<string>{LABEL}</string>"),
            )
            .replace(
                "\t\t<string>--background</string>\n",
                &format!(
                    "\t\t<string>--background</string>\n\t\t<string>--data-dir</string>\n\t\t<string>{dir_text}</string>\n"
                ),
            );
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{LABEL}.plist")))
                .expect("DIR/LABEL.plist stands"),
            expected
        );
        assert_eq!(entries(&dir), vec![dir.join(format!("{LABEL}.plist"))]);
        assert_eq!(entries(&home), Vec::<PathBuf>::new());

        let real = scratch("login-item-scratch-real");
        assert_eq!(scratch_form(LOGIN_ITEM_LABEL, &real, &account), Exit::Usage);
        assert_eq!(Exit::Usage.code(), 64);
        assert_eq!(entries(&real), Vec::<PathBuf>::new());
        let default = home.join("Library/Application Support/Halvo");
        std::fs::create_dir_all(&default).expect("the scratch home's data directory");
        assert_eq!(scratch_form(LABEL, &default, &account), Exit::Usage);
        assert_eq!(
            entries(&home),
            vec![
                home.join("Library"),
                home.join("Library/Application Support"),
                default
            ]
        );
    }

    fn data_dir(name: &str, provisioned: bool) -> DataDir {
        let dir = DataDir::from_flag(scratch(name)).expect("an absolute directory");
        if provisioned {
            std::fs::write(dir.state_file(), b"{}").expect("a state file");
        }
        dir
    }

    fn stand(home: &Path, program: &str) -> PathBuf {
        let spec = LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new(program), None).expect("a spec");
        install_login_item(&spec, &agents_of(home)).expect("the plist is written");
        home.join("Library/LaunchAgents/com.halvo.desktop.plist")
    }

    fn inode(path: &Path) -> u64 {
        std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(path).expect("the plist stands"))
    }

    fn start(
        home: &Path,
        data_dir: &DataDir,
        given: bool,
        program: &str,
        runner: &Recorded,
    ) -> Views {
        let views = Views::default();
        at_start(
            &views,
            data_dir,
            given,
            || LoginItem::new(LOGIN_ITEM_LABEL, Path::new(program), agents_of(home), None).ok(),
            runner,
        );
        views
    }

    #[test]
    fn a_start_rewrites_a_plist_of_another_path_and_writes_none_where_none_stands() {
        let ready = data_dir("login-item-ready", true);
        let unready = data_dir("login-item-unready", false);

        let home = scratch("login-item-stale");
        let plist = stand(&home, OTHER_PROGRAM);
        start(&home, &ready, false, PROGRAM, &Recorded::answering(Some(0)));
        assert_eq!(
            std::fs::read_to_string(&plist).expect("the plist stands"),
            plist_of(PROGRAM),
            "Stale is written again"
        );

        let home = scratch("login-item-absent");
        start(&home, &ready, false, PROGRAM, &Recorded::answering(Some(0)));
        assert_eq!(
            entries(&home),
            Vec::<PathBuf>::new(),
            "Absent writes nothing"
        );

        let home = scratch("login-item-current");
        let plist = stand(&home, PROGRAM);
        let before = inode(&plist);
        start(&home, &ready, false, PROGRAM, &Recorded::answering(Some(0)));
        assert_eq!(inode(&plist), before, "Current writes nothing");

        for (row, data_dir, given, program) in [
            ("a translocated path", &ready, false, TRANSLOCATED),
            ("a start with --data-dir", &ready, true, PROGRAM),
            (
                "a directory that is not provisioned",
                &unready,
                false,
                PROGRAM,
            ),
        ] {
            let home = scratch("login-item-kept");
            let plist = stand(&home, OTHER_PROGRAM);
            let before = inode(&plist);
            let runner = Recorded::answering(Some(0));
            start(&home, data_dir, given, program, &runner);
            assert_eq!(inode(&plist), before, "{row}");
            assert_eq!(runner.calls(), Vec::<Vec<String>>::new(), "{row}");
            assert_eq!(
                std::fs::read_to_string(&plist).expect("the plist stands"),
                plist_of(OTHER_PROGRAM),
                "{row}"
            );
        }
    }

    fn wire(view: StatusView) -> Value {
        serde_json::to_value(view).expect("the view writes")["login_item_loaded"].clone()
    }

    #[test]
    fn the_load_state_reads_the_exit_status_alone_and_the_view_carries_not_loaded() {
        let ready = data_dir("login-item-load-ready", true);
        let not_found = "Could not find service \"com.halvo.desktop\" in domain for user gui";
        for (standing, status, output, expected) in [
            (PROGRAM, Some(0), not_found, json!(true)),
            (PROGRAM, Some(NOT_LOADED), "state = running", json!(false)),
            (
                OTHER_PROGRAM,
                Some(NOT_LOADED),
                "state = running",
                json!(false),
            ),
            (PROGRAM, Some(1), "state = running", Value::Null),
            (PROGRAM, Some(5), not_found, Value::Null),
            (PROGRAM, None, "state = running", Value::Null),
        ] {
            let row =
                format!("a plist of {standing}, the status {status:?}, the output {output:?}");
            let home = scratch("login-item-load");
            let plist = stand(&home, standing);
            let runner = Recorded::answering(status);
            let views = start(&home, &ready, false, PROGRAM, &runner);
            assert_eq!(wire(views.view()), expected, "{row}");
            assert_eq!(
                wire(views.marked(StatusView::of(&sample_status(Platform::Macos)))),
                expected,
                "{row}"
            );
            let uid = std::os::unix::fs::MetadataExt::uid(
                &std::fs::metadata(&plist).expect("the plist stands"),
            );
            assert_eq!(
                runner.calls(),
                vec![vec![
                    "print".to_owned(),
                    format!("gui/{uid}/com.halvo.desktop")
                ]],
                "{row}"
            );
        }

        let home = scratch("login-item-load-absent");
        let runner = Recorded::answering(Some(NOT_LOADED));
        let views = start(&home, &ready, false, PROGRAM, &runner);
        assert_eq!(wire(views.view()), Value::Null, "no plist");
        assert_eq!(runner.calls(), Vec::<Vec<String>>::new(), "no plist");
    }
}
