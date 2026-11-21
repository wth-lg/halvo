use std::io::Write;
use std::process::ExitCode;

use halvo_capture::daemon::{self, Mode};
use halvo_control::exit;
use halvo_control::paths::SystemAccount;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let parsed = daemon::parse(std::env::args_os().skip(1), &SystemAccount);
    if let Ok(mode) = &parsed {
        if let Err(error) = daemon::prepare_signals(mode) {
            return fail(format_args!("the signal flag was not installed: {error}"));
        }
    }
    match parsed {
        Ok(Mode::Version) => {
            print_or_exit_error(&mut std::io::stdout().lock(), &format!("{VERSION}\n"))
        }
        Ok(Mode::ProbePermissions(permissions)) => status(daemon::probe_answer(
            &permissions,
            &mut std::io::stdout().lock(),
        )),
        Ok(Mode::RequestPermission(permission)) => status(daemon::request_answer(
            permission,
            &mut std::io::stdout().lock(),
        )),
        #[cfg(debug_assertions)]
        Ok(Mode::SelfCheckRunLoop) => {
            status(daemon::self_check_run_loop(&mut std::io::stdout().lock()))
        }
        #[cfg(debug_assertions)]
        Ok(Mode::SelfCheckServiceRun { data_dir }) => status(daemon::self_check_service_run(
            &data_dir,
            daemon::DurableState::writer(),
            &mut std::io::stdout().lock(),
        )),
        #[cfg(debug_assertions)]
        Ok(Mode::TimedRun(run)) => match daemon::timed_run(&run) {
            Ok(outcome) => status(daemon::outcome_answer(
                &outcome,
                &mut std::io::stdout().lock(),
                &mut std::io::stderr(),
            )),
            Err(error) => fail(format_args!("{error}")),
        },
        #[cfg(debug_assertions)]
        Ok(mode @ (Mode::VerifyStore { .. } | Mode::VerifyDeletion { .. })) => {
            run_verify(mode, measure_store, measure_deletion)
        }
        #[cfg(debug_assertions)]
        Ok(Mode::DeleteAll { store_dir }) => status(delete_all(
            &store_dir,
            &mut std::io::stdout().lock(),
            &mut std::io::stderr(),
        )),
        #[cfg(any(debug_assertions, target_os = "macos"))]
        Ok(Mode::ServiceRun { data_dir }) => status(daemon::service_run(
            &data_dir,
            daemon::DurableState::writer(),
        )),
        #[cfg(any(debug_assertions, target_os = "macos"))]
        Ok(Mode::LifelineServiceRun { data_dir }) => status(daemon::lifeline_service_run(
            &data_dir,
            daemon::DurableState::writer(),
        )),
        #[cfg(target_os = "macos")]
        Ok(Mode::DeleteAllOfADataDir { data_dir }) => status(daemon::delete_all::offline(
            &data_dir,
            &mut std::io::stdout().lock(),
            &mut std::io::stderr(),
        )),
        #[cfg(all(not(target_os = "macos"), debug_assertions))]
        Ok(Mode::DeleteAllOfADataDir { .. }) => status(exit::USAGE),
        #[cfg(any(debug_assertions, target_os = "macos"))]
        Ok(Mode::Ctl(ctl)) => status(daemon::ctl(
            &ctl,
            &mut std::io::stdout().lock(),
            &mut std::io::stderr(),
        )),
        #[cfg(all(debug_assertions, feature = "test-seam"))]
        Ok(Mode::ScriptedServiceRun { data_dir }) => status(daemon::seam::scripted_service_run(
            &data_dir,
            daemon::DurableState::writer(),
        )),
        Err(usage) => status(daemon::usage_answer(&usage, &mut std::io::stderr())),
    }
}

#[cfg(debug_assertions)]
fn delete_all(store_dir: &std::path::Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    daemon::scratch_delete(store_dir, scratch_tail, out, err)
}

#[cfg(debug_assertions)]
fn scratch_tail(store_dir: &std::path::Path) -> daemon::TailAnswer {
    daemon::port::ScratchDir::at(store_dir).delete_tail()
}

#[cfg(debug_assertions)]
fn measure_store(store_dir: &std::path::Path, marker: &str) -> Result<String, String> {
    daemon::port::ScratchDir::at(store_dir).measure_store(marker)
}

#[cfg(debug_assertions)]
fn measure_deletion(store_dir: &std::path::Path) -> Result<String, String> {
    daemon::port::ScratchDir::at(store_dir).measure_deletion()
}

fn print_or_exit_error(out: &mut dyn Write, text: &str) -> ExitCode {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(format_args!("the report was not written: {error}")),
    }
}

#[cfg(debug_assertions)]
type MeasureStore = fn(&std::path::Path, &str) -> Result<String, String>;

#[cfg(debug_assertions)]
type MeasureDeletion = fn(&std::path::Path) -> Result<String, String>;

#[cfg(debug_assertions)]
fn run_verify(mode: Mode, store: MeasureStore, deletion: MeasureDeletion) -> ExitCode {
    match mode {
        Mode::VerifyStore { store_dir, marker } => print_or_fail(store(&store_dir, &marker)),
        Mode::VerifyDeletion { store_dir } => print_or_fail(deletion(&store_dir)),
        _ => status(exit::USAGE),
    }
}

#[cfg(debug_assertions)]
fn print_or_fail<E: std::fmt::Display>(result: Result<String, E>) -> ExitCode {
    match result {
        Ok(text) => print_or_exit_error(&mut std::io::stdout().lock(), &text),
        Err(error) => fail(format_args!("{error}")),
    }
}

fn fail(error: std::fmt::Arguments<'_>) -> ExitCode {
    let _ = writeln!(std::io::stderr(), "halvo-capture: {error}");
    status(exit::ERROR)
}

fn status(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use super::*;

    thread_local! {
        static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn recorded_store(dir: &Path, marker: &str) -> Result<String, String> {
        CALLS.with(|calls| {
            calls
                .borrow_mut()
                .push(format!("store {} {marker}", dir.display()))
        });
        Ok(String::new())
    }

    fn recorded_deletion(dir: &Path) -> Result<String, String> {
        CALLS.with(|calls| {
            calls
                .borrow_mut()
                .push(format!("deletion {}", dir.display()))
        });
        Ok(String::new())
    }

    #[test]
    fn a_write_into_a_pipe_whose_reader_is_gone_ends_in_exit_1() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        drop(reader);
        assert_eq!(
            print_or_exit_error(&mut writer, "a report\n"),
            ExitCode::from(1)
        );
    }

    fn calls() -> Vec<String> {
        CALLS.with(|calls| calls.borrow_mut().drain(..).collect())
    }

    #[test]
    fn verify_store_measures_the_store_dir_of_its_flag_with_its_marker() {
        let mode = Mode::VerifyStore {
            store_dir: PathBuf::from("/scratch/flag-dir"),
            marker: "M4rker.from-the-flag".to_owned(),
        };
        assert_eq!(
            run_verify(mode, recorded_store, recorded_deletion),
            ExitCode::SUCCESS
        );
        assert_eq!(calls(), ["store /scratch/flag-dir M4rker.from-the-flag"]);
    }

    struct StandIn(PathBuf);

    impl halvo_control::paths::AccountLookup for StandIn {
        fn base_dir(&self) -> Result<Option<std::ffi::OsString>, halvo_control::paths::PathError> {
            Ok(Some(self.0.clone().into_os_string()))
        }

        fn user_temp_dir(
            &self,
        ) -> Result<Option<halvo_control::paths::DarwinUserTempDir>, halvo_control::paths::PathError>
        {
            Ok(None)
        }
    }

    struct Scratch {
        root: tempfile::TempDir,
        account: StandIn,
        data: PathBuf,
    }

    fn scratch() -> Scratch {
        let target = std::env::current_exe()
            .unwrap()
            .ancestors()
            .nth(3)
            .unwrap()
            .join("tmp");
        std::fs::create_dir_all(&target).unwrap();
        let root = tempfile::Builder::new()
            .prefix("capture-bin-tests-")
            .tempdir_in(target)
            .unwrap();
        let account = StandIn(root.path().join("home"));
        let data = halvo_control::paths::DataDir::for_account(&account)
            .unwrap()
            .path()
            .to_path_buf();
        std::fs::create_dir_all(&data).unwrap();
        Scratch {
            root,
            account,
            data,
        }
    }

    impl Scratch {
        fn dir(&self, name: &str) -> PathBuf {
            let dir = self.root.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        fn delete(&self, store_dir: &str) -> (i32, String, String) {
            let Ok(Mode::DeleteAll { store_dir }) = daemon::parse(
                ["--delete-all", "--store-dir", store_dir].map(std::ffi::OsString::from),
                &self.account,
            ) else {
                panic!("--delete-all --store-dir {store_dir} was refused");
            };
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let status = delete_all(&store_dir, &mut out, &mut err);
            (
                status,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        }
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_owned()
    }

    fn root_files(store_dir: &Path) -> Vec<String> {
        match std::fs::read_dir(store_dir.join("roots")) {
            Ok(entries) => entries
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("roots of {}: {error}", store_dir.display()),
        }
    }

    fn live_store(dir: &Path) -> Box<dyn daemon::port::CaptureStore + Send> {
        let made = daemon::port::ScratchDir::at(dir).create().unwrap();
        std::mem::forget(made.root);
        made.store
    }

    fn deleted_line(out: &str) -> &str {
        out.lines()
            .nth(1)
            .filter(|line| line.starts_with("deleted "))
            .unwrap_or_else(|| panic!("no deleted line in {out}"))
    }

    #[test]
    fn an_empty_store_dir_prints_no_store_and_exits_0_twice() {
        let scratch = scratch();
        let dir = text(&scratch.dir("empty"));
        for _ in 0..2 {
            assert_eq!(
                scratch.delete(&dir),
                (0, format!("store_dir={dir}\nno_store\n"), String::new())
            );
            assert_eq!(top_level(Path::new(&dir)), what_the_tail_leaves(&[]));
        }
    }

    fn top_level(dir: &Path) -> Vec<(String, u64)> {
        let mut names: Vec<(String, u64)> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().into_string().unwrap(),
                    entry.metadata().unwrap().len(),
                )
            })
            .filter(|(name, _)| name != "roots")
            .collect();
        names.sort();
        names
    }

    fn what_the_tail_leaves(kept: &[&str]) -> Vec<(String, u64)> {
        let mut names: Vec<(String, u64)> = kept
            .iter()
            .map(|name| ((*name).to_owned(), u64::MAX))
            .collect();
        if cfg!(windows) {
            names.push(("writer.lock".to_owned(), 0));
        }
        names.sort();
        names
    }

    #[test]
    fn a_store_whose_root_verify_store_destroyed_loses_its_files_with_1_and_then_reads_no_store() {
        let scratch = scratch();
        let dir = scratch.dir("verified");
        measure_store(&dir, "gate-marker-01").unwrap();
        assert!(dir.join("store.key").exists());
        assert_eq!(root_files(&dir), Vec::<String>::new());
        let (status, out, err) = scratch.delete(&text(&dir));
        assert_eq!(status, 1, "{out}{err}");
        let line = deleted_line(&out);
        assert!(
            line.starts_with("deleted segments_shredded=unknown keys_destroyed=unknown root_destroyed=no root_id=")
                && line.ends_with(" files_left=0 database_left=no not_followed=0"),
            "{out}"
        );
        let id = line
            .split(' ')
            .find_map(|token| token.strip_prefix("root_id="))
            .unwrap();
        assert!(
            id.len() == 32
                && id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{out}"
        );
        assert_eq!(
            err,
            "halvo-capture: the delete is not finished: this call did not destroy the store's root\n"
        );
        assert!(!dir.join("store.key").exists(), "{out}{err}");
        assert_eq!(root_files(&dir), Vec::<String>::new(), "{out}{err}");
        assert_eq!(
            scratch.delete(&text(&dir)),
            (
                0,
                format!("store_dir={}\nno_store\n", text(&dir)),
                String::new()
            )
        );
    }

    #[test]
    fn a_live_store_is_held_with_75_and_once_dropped_loses_its_root_with_0() {
        let scratch = scratch();
        let dir = scratch.dir("live");
        let store = live_store(&dir);
        assert_eq!(root_files(&dir).len(), 1);
        let (status, out, err) = scratch.delete(&text(&dir));
        assert_eq!(status, 75, "{out}{err}");
        assert_eq!(out, format!("store_dir={}\n", text(&dir)));
        assert!(dir.join("store.key").exists());
        assert_eq!(root_files(&dir).len(), 1);
        drop(store);
        let (status, out, err) = scratch.delete(&text(&dir));
        assert_eq!(status, 0, "{out}{err}");
        assert!(
            deleted_line(&out).contains(" root_destroyed=yes root_id=- "),
            "{out}"
        );
        assert!(!dir.join("store.key").exists());
        assert_eq!(root_files(&dir), Vec::<String>::new());
    }

    #[test]
    fn a_real_folder_spelled_with_a_trailing_separator_is_deleted_as_when_named_plainly() {
        let scratch = scratch();
        for spelling in ["/", "/.", "//"] {
            let dir = scratch.dir(&format!("spelled{}", spelling.len()));
            drop(live_store(&dir));
            let given = format!("{}{spelling}", text(&dir));
            let (status, out, err) = scratch.delete(&given);
            assert_eq!(status, 0, "{given}: {out}{err}");
            assert!(
                out.starts_with(&format!("store_dir={given}\ndeleted ")),
                "{out}"
            );
            assert!(!dir.join("store.key").exists(), "{given}");
            assert_eq!(root_files(&dir), Vec::<String>::new(), "{given}");
        }
    }

    #[test]
    fn a_folder_in_the_roots_folder_is_counted_and_never_named() {
        let scratch = scratch();
        let dir = scratch.dir("with-a-folder");
        drop(live_store(&dir));
        let named = dir.join("roots").join("holiday-photos-of-a-person");
        std::fs::create_dir(&named).unwrap();
        let (status, out, err) = scratch.delete(&text(&dir));
        assert_eq!(status, 1, "{out}{err}");
        assert!(
            deleted_line(&out).contains(" root_destroyed=yes root_id=- "),
            "{out}"
        );
        assert_eq!(
            err,
            "halvo-capture: the delete is not finished: 1 folders stay in the folder roots at other names\n"
        );
        assert!(!out.contains("holiday") && !err.contains("holiday"));
        assert!(!dir.join("store.key").exists());
        assert!(named.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_root_file_that_cannot_be_removed_is_named_by_its_name_in_the_roots_folder() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let scratch = scratch();
        let dir = scratch.dir("unremovable");
        drop(live_store(&dir));
        let roots = dir.join("roots");
        let names = root_files(&dir);
        assert_eq!(names.len(), 1);
        let id = names[0].strip_suffix(".root").unwrap().to_owned();
        if std::fs::metadata(&roots).unwrap().uid() == 0 {
            println!("not run: the test runs as root, whom a mode of 0500 does not stop");
            return;
        }
        std::fs::write(roots.join("plain-name-of-32"), [7u8; 32]).unwrap();
        std::fs::write(roots.join("plain-name-of-5"), [7u8; 5]).unwrap();
        std::fs::set_permissions(&roots, std::fs::Permissions::from_mode(0o500)).unwrap();
        let answer = scratch.delete(&text(&dir));
        std::fs::set_permissions(&roots, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (status, out, err) = answer;
        assert_eq!(status, 1, "{out}{err}");
        assert!(
            deleted_line(&out).contains(&format!(" root_destroyed=no root_id={id} ")),
            "{out}"
        );
        assert!(
            err.contains(&format!("the root file roots/{} stays", names[0])),
            "{err}"
        );
        assert!(
            err.contains("; 1 files that can hold a root stay in the folder roots at other names"),
            "{err}"
        );
        assert!(
            err.contains("; 1 other entries stay in the folder roots at other names"),
            "{err}"
        );
        assert!(!err.contains("plain-name"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_at_store_key_is_removed_and_counted_in_the_deleted_line_never_as_no_store() {
        let scratch = scratch();
        let dir = scratch.dir("key-link");
        let outside = scratch.root.path().join("outside-key");
        std::fs::write(&outside, b"kept").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("store.key")).unwrap();
        let (status, out, err) = scratch.delete(&text(&dir));
        assert_eq!(status, 0, "{out}{err}");
        assert_eq!(
            deleted_line(&out),
            "deleted segments_shredded=unknown keys_destroyed=unknown root_destroyed=unknown root_id=- files_left=0 database_left=no not_followed=1"
        );
        assert!(std::fs::symlink_metadata(dir.join("store.key")).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"kept");
    }

    fn files_under(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(at) = pending.pop() {
            for entry in std::fs::read_dir(&at).unwrap() {
                let path = entry.unwrap().path();
                if std::fs::symlink_metadata(&path).unwrap().is_dir() {
                    pending.push(path);
                } else {
                    files.push((path.clone(), std::fs::read(&path).unwrap()));
                }
            }
        }
        files.sort();
        files
    }

    fn data_directory_layout(scratch: &Scratch, name: &str) -> PathBuf {
        let dir = scratch.dir(name);
        let store = dir.join("store");
        std::fs::create_dir(&store).unwrap();
        drop(live_store(&store));
        std::fs::create_dir(dir.join("roots")).unwrap();
        for root in root_files(&store) {
            std::fs::copy(
                store.join("roots").join(&root),
                dir.join("roots").join(&root),
            )
            .unwrap();
        }
        dir
    }

    #[test]
    fn a_data_directory_layout_given_as_store_dir_is_refused_with_64_and_keeps_every_file() {
        let scratch = scratch();
        let dir = data_directory_layout(&scratch, "a-data-dir");
        let before = files_under(&dir);
        assert_eq!(root_files(&dir).len(), 1);
        let link = scratch.root.path().join("a-data-dir-link");
        link_to_a_folder(&dir, &link);
        for given in [
            text(&dir),
            format!("{}/", text(&dir)),
            format!("{}/.", text(&dir)),
            format!("{}//", text(&dir)),
            text(&link),
            format!("{}/", text(&link)),
        ] {
            let answer = daemon::parse(
                ["--delete-all", "--store-dir", &given].map(std::ffi::OsString::from),
                &scratch.account,
            );
            let Err(refusal) = answer else {
                panic!("{given} was taken: {answer:?}");
            };
            let mut err = Vec::new();
            assert_eq!(daemon::usage_answer(&refusal, &mut err), 64, "{given}");
            assert_eq!(
                String::from_utf8(err).unwrap(),
                format!("halvo-capture: {given} holds store/ beside roots/, the layout of a data directory; the scratch delete takes a scratch store directory alone, and nothing in this one was opened or removed\n")
            );
            assert_eq!(files_under(&dir), before, "{given}");
        }
    }

    #[test]
    fn a_store_folder_alone_or_a_roots_folder_alone_is_the_scratch_layouts_to_handle() {
        let scratch = scratch();
        let only_store = scratch.dir("only-store");
        drop(live_store(&only_store.join("store")));
        let kept = files_under(&only_store.join("store"));
        assert_eq!(
            scratch.delete(&text(&only_store)),
            (
                0,
                format!("store_dir={}\nno_store\n", text(&only_store)),
                String::new()
            )
        );
        assert_eq!(files_under(&only_store.join("store")), kept);
        let mut left = top_level(&only_store);
        for (name, len) in &mut left {
            if name == "store" {
                *len = u64::MAX;
            }
        }
        assert_eq!(left, what_the_tail_leaves(&["store"]));
        assert_eq!(
            scratch.delete(&text(&only_store)).0,
            0,
            "a second call on the same folder"
        );
        assert_eq!(
            top_level(&only_store).len(),
            what_the_tail_leaves(&["store"]).len()
        );
        let only_roots = scratch.dir("only-roots");
        std::fs::create_dir(only_roots.join("roots")).unwrap();
        std::fs::write(
            only_roots
                .join("roots")
                .join("0123456789abcdef0123456789abcdef.root"),
            [7u8; 32],
        )
        .unwrap();
        assert_eq!(
            scratch.delete(&text(&only_roots)),
            (
                0,
                format!("store_dir={}\nno_store\n", text(&only_roots)),
                String::new()
            )
        );
        assert_eq!(root_files(&only_roots), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn a_store_dir_that_cannot_be_opened_says_so_and_claims_nothing_about_what_stays() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let scratch = scratch();
        let dir = scratch.dir("unopened");
        if std::fs::metadata(&dir).unwrap().uid() == 0 {
            println!("not run: the test runs as root, whom a mode of 0100 does not stop");
            return;
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o100)).unwrap();
        let answer = scratch.delete(&text(&dir));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (status, out, err) = answer;
        assert_eq!(status, 1, "{out}{err}");
        assert_eq!(
            deleted_line(&out),
            "deleted segments_shredded=unknown keys_destroyed=unknown root_destroyed=no root_id=- files_left=1 database_left=yes not_followed=0"
        );
        assert_eq!(
            err,
            "halvo-capture: the delete is not finished: the store directory could not be opened, so this call cannot say what stays in it\n"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn an_opened_dir_whose_roots_cannot_be_listed_is_never_said_to_be_unopened() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let scratch = scratch();
        let dir = scratch.dir("roots-unlistable");
        drop(live_store(&dir));
        std::fs::write(dir.join("create.pending"), b"not a mark").unwrap();
        let names = root_files(&dir);
        let id = names[0].strip_suffix(".root").unwrap().to_owned();
        if std::fs::metadata(&dir).unwrap().uid() == 0 {
            println!("not run: the test runs as root, whom a mode does not stop");
            return;
        }
        let before = files_under(&dir)
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>();
        std::fs::set_permissions(dir.join("roots"), std::fs::Permissions::from_mode(0o000))
            .unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let answer = scratch.delete(&text(&dir));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(dir.join("roots"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let (status, out, err) = answer;
        assert_eq!(status, 1, "{out}{err}");
        assert!(
            deleted_line(&out).contains(&format!(" root_destroyed=no root_id={id} ")),
            "{out}"
        );
        assert_eq!(
            err,
            "halvo-capture: the delete is not finished: this call did not destroy the store's root; the folder roots could not be listed; store.key stays; a database file stays; 1 other files of the store stay\n"
        );
        assert_eq!(
            files_under(&dir)
                .into_iter()
                .map(|(path, _)| path)
                .collect::<Vec<_>>(),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_store_dir_whose_names_cannot_be_read_is_refused_with_64_and_keeps_every_file() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let scratch = scratch();
        for mode in [0o000, 0o400] {
            let dir = scratch.dir(&format!("unread-{mode:o}"));
            if std::fs::metadata(&dir).unwrap().uid() == 0 {
                println!("not run: the test runs as root, whom a mode does not stop");
                return;
            }
            std::fs::create_dir(dir.join("store")).unwrap();
            std::fs::create_dir(dir.join("roots")).unwrap();
            std::fs::write(dir.join("roots").join("plain"), [9u8; 32]).unwrap();
            let before = files_under(&dir);
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
            let answer = daemon::parse(
                ["--delete-all", "--store-dir", &text(&dir)].map(std::ffi::OsString::from),
                &scratch.account,
            );
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            let Err(refusal) = answer else {
                panic!("mode {mode:o} was taken: {answer:?}");
            };
            let mut err = Vec::new();
            assert_eq!(
                daemon::usage_answer(&refusal, &mut err),
                64,
                "mode {mode:o}"
            );
            assert_eq!(
                String::from_utf8(err).unwrap(),
                format!(
                    "halvo-capture: the names store and roots in {} cannot be read, so the scratch delete cannot tell whether it holds a data directory's layout; nothing in it was opened or removed\n",
                    text(&dir)
                ),
                "mode {mode:o}"
            );
            assert_eq!(files_under(&dir), before, "mode {mode:o}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_opened_dir_whose_root_was_destroyed_is_never_said_to_be_unopened() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let scratch = scratch();
        let dir = scratch.dir("read-only-top");
        drop(live_store(&dir));
        std::fs::write(dir.join("create.pending"), b"not a mark").unwrap();
        if std::fs::metadata(&dir).unwrap().uid() == 0 {
            println!("not run: the test runs as root, whom a mode of 0555 does not stop");
            return;
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let answer = scratch.delete(&text(&dir));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (status, out, err) = answer;
        assert_eq!(status, 1, "{out}{err}");
        assert!(
            deleted_line(&out).contains(" root_destroyed=yes root_id=- "),
            "{out}"
        );
        assert_eq!(
            err,
            "halvo-capture: the delete is not finished: store.key stays; a database file stays; 1 other files of the store stay\n"
        );
        assert!(dir.join("store.key").exists() && dir.join("create.pending").exists());
        assert_eq!(root_files(&dir), Vec::<String>::new());
    }

    fn link_to_a_folder(folder: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(folder, link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(folder, link).unwrap();
    }

    #[test]
    fn a_link_or_a_file_at_the_store_dir_is_refused_in_every_spelling_and_keeps_the_store() {
        let scratch = scratch();
        let dir = scratch.dir("E");
        drop(live_store(&dir));
        let roots_before = root_files(&dir);
        assert_eq!(roots_before.len(), 1);
        let link = scratch.root.path().join("E-link");
        link_to_a_folder(&dir, &link);
        let file = scratch.root.path().join("a-file");
        std::fs::write(&file, b"not a folder").unwrap();
        let mut spellings = vec![
            text(&link),
            format!("{}/", text(&link)),
            format!("{}/.", text(&link)),
            format!("{}//", text(&link)),
            text(&file),
        ];
        if cfg!(windows) {
            spellings.push(format!("{}\\", text(&link)));
        }
        for given in spellings {
            let (status, out, err) = scratch.delete(&given);
            assert_eq!(status, 1, "{given}: {out}{err}");
            assert!(
                out.starts_with(&format!("store_dir={given}\ndeleted ")),
                "{out}"
            );
            assert!(deleted_line(&out).ends_with(" not_followed=1"), "{out}");
            assert_eq!(
                err,
                format!("halvo-capture: {given} is a link or a file, not a folder; nothing was removed; give the path of the folder itself\n")
            );
            assert!(dir.join("store.key").exists(), "{given}");
            assert_eq!(root_files(&dir), roots_before, "{given}");
            assert!(std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(std::fs::read(&file).unwrap(), b"not a folder");
        }
    }

    #[test]
    fn a_store_dir_that_reaches_the_default_data_directory_is_refused_with_64() {
        let scratch = scratch();
        let inside = scratch.data.join("scratch");
        std::fs::create_dir_all(&inside).unwrap();
        let side = scratch.dir("side");
        let mut refused = vec![
            text(&scratch.data),
            text(&inside),
            text(&scratch.account.0),
            format!(
                "{}/../{}",
                text(&side),
                text(&scratch.data)
                    .trim_start_matches(&text(scratch.root.path()))
                    .trim_start_matches(['/', '\\'])
            ),
        ];
        let link = scratch.root.path().join("data-link");
        link_to_a_folder(&scratch.data, &link);
        refused.push(text(&link));
        refused.push(format!("{}/scratch", text(&link)));
        for given in refused {
            let answer = daemon::parse(
                ["--delete-all", "--store-dir", &given].map(std::ffi::OsString::from),
                &scratch.account,
            );
            let Err(refusal) = answer else {
                panic!("{given} was taken: {answer:?}");
            };
            let mut err = Vec::new();
            assert_eq!(daemon::usage_answer(&refusal, &mut err), 64, "{given}");
            assert!(String::from_utf8(err).unwrap().starts_with(
                "halvo-capture: DIR is no absolute directory clear of the data directory\n"
            ));
        }
        assert!(matches!(
            daemon::parse(
                ["--delete-all", "--store-dir", &text(&side)].map(std::ffi::OsString::from),
                &scratch.account,
            ),
            Ok(Mode::DeleteAll { .. })
        ));
    }

    #[test]
    fn verify_deletion_measures_the_store_dir_of_its_flag() {
        let mode = Mode::VerifyDeletion {
            store_dir: PathBuf::from("/scratch/flag-dir"),
        };
        assert_eq!(
            run_verify(mode, recorded_store, recorded_deletion),
            ExitCode::SUCCESS
        );
        assert_eq!(calls(), ["deletion /scratch/flag-dir"]);
    }
}
