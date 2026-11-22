use std::path::{Path, PathBuf};

use halvo_control::paths::{AccountLookup, DataDir, LoginItemDir, PathError, ScratchDir};
use halvo_control::service::{
    install_login_item, login_item_state, InstallOutcome, LoginItemSpec, LoginItemState, SpecError,
    LOGIN_ITEM_LABEL,
};

use crate::launch::Exit;

/// The sentence that a person reads where Halvo runs under an AppTranslocation path, and so
/// writes no login item.
pub const MOVE_TO_APPLICATIONS: &str = "Halvo cannot start at login while macOS runs it from a temporary location. Move Halvo to your Applications folder, open it from there, and try again.";

/// Why the manager writes no login item for a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Translocated,
    Spec(SpecError),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Translocated => f.write_str(MOVE_TO_APPLICATIONS),
            Refusal::Spec(error) => error.fmt(f),
        }
    }
}

/// The app's one manager of its login item. It takes the label, the program, the login item's
/// directory and, for a scratch run, the data directory as values that main resolves, and
/// writes through the one writer of halvo-control; a write starts no process.
#[derive(Debug)]
pub struct LoginItem {
    spec: LoginItemSpec,
    dir: LoginItemDir,
}

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
}

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

/// What a start of `run` does with the login item: with --data-dir (`given`), or on a data
/// directory that does not read provisioned, nothing. Otherwise the manager that `item` makes for
/// the app's own path reads the plist's state, and writes it again where it reads Stale; Absent
/// and Current write nothing, and a program that the manager refuses writes nothing.
pub fn at_start(data_dir: &DataDir, given: bool, item: impl FnOnce() -> Option<LoginItem>) {
    if given || !matches!(data_dir.is_provisioned(), Ok(true)) {
        return;
    }
    let Some(item) = item() else {
        return;
    };
    if let Ok(LoginItemState::Stale) = login_item_state(&item.spec, &item.dir) {
        let _ = item.write();
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::{parse, Launch};
    use halvo_control::paths::DarwinUserTempDir;
    use std::ffi::OsString;

    const PROGRAM: &str = "/Applications/Halvo.app/Contents/MacOS/halvo";
    const LABEL: &str = "dev.halvo.test.scratch";
    const OTHER_PROGRAM: &str = "/Users/Shared/Halvo.app/Contents/MacOS/halvo";
    const TRANSLOCATED: &str =
        "/private/var/folders/zz/T/AppTranslocation/0A1B2C3D/d/Halvo.app/Contents/MacOS/halvo";

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

    fn start(home: &Path, data_dir: &DataDir, given: bool, program: &str) {
        at_start(data_dir, given, || {
            LoginItem::new(LOGIN_ITEM_LABEL, Path::new(program), agents_of(home), None).ok()
        });
    }

    #[test]
    fn a_start_rewrites_a_plist_of_another_path_and_writes_none_where_none_stands() {
        let ready = data_dir("login-item-ready", true);
        let unready = data_dir("login-item-unready", false);

        let home = scratch("login-item-stale");
        let plist = stand(&home, OTHER_PROGRAM);
        start(&home, &ready, false, PROGRAM);
        assert_eq!(
            std::fs::read_to_string(&plist).expect("the plist stands"),
            plist_of(PROGRAM),
            "Stale is written again"
        );

        let home = scratch("login-item-absent");
        start(&home, &ready, false, PROGRAM);
        assert_eq!(
            entries(&home),
            Vec::<PathBuf>::new(),
            "Absent writes nothing"
        );

        let home = scratch("login-item-current");
        let plist = stand(&home, PROGRAM);
        let before = inode(&plist);
        start(&home, &ready, false, PROGRAM);
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
            start(&home, data_dir, given, program);
            assert_eq!(inode(&plist), before, "{row}");
            assert_eq!(
                std::fs::read_to_string(&plist).expect("the plist stands"),
                plist_of(OTHER_PROGRAM),
                "{row}"
            );
        }
    }
}
