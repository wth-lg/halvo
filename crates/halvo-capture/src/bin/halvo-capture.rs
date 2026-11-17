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
        Ok(mode @ (Mode::VerifyStore { .. } | Mode::VerifyDeletion { .. })) => run_verify(
            mode,
            halvo_capture::store::measure_store,
            halvo_capture::store::measure_deletion,
        ),
        #[cfg(debug_assertions)]
        Ok(Mode::DeleteAll { store_dir }) => status(delete_all(
            &store_dir,
            &mut std::io::stdout().lock(),
            &mut std::io::stderr(),
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
    use halvo_capture::store::db::EncryptedStore;
    use halvo_capture::store::{Layout, StoreError};

    match EncryptedStore::delete_data_dir(store_dir, Layout::ScratchStore) {
        Ok(report) => daemon::TailAnswer::Finished(tail_report(&report)),
        Err(StoreError::DeleteIncomplete(report)) => {
            daemon::TailAnswer::NotFinished(tail_report(&report))
        }
        Err(StoreError::Locked) => daemon::TailAnswer::Held,
        Err(error) => daemon::TailAnswer::Failed(error.to_string()),
    }
}

#[cfg(debug_assertions)]
fn tail_report(report: &halvo_capture::store::DeleteReport) -> daemon::TailReport {
    use halvo_capture::store::RootReading;

    daemon::TailReport {
        keyed_segments: report.keyed_segments,
        root: match report.root {
            RootReading::Destroyed => daemon::TailRoot::Destroyed,
            RootReading::Left(id) => daemon::TailRoot::Left(id.map(|id| id.to_string())),
            RootReading::Unnamed => daemon::TailRoot::Unnamed,
        },
        root_files_left: report.roots.names.clone(),
        others_left: daemon::EntriesLeft {
            root_shaped: report.roots.others_left.root_shaped,
            directories: report.roots.others_left.directories,
            other: report.roots.others_left.other,
        },
        roots_listed: report.roots.listed,
        roots_at_their_path: report.roots.at_its_path,
        key_file_gone: report.key_file_gone,
        database_left: report.database_left,
        files_left: report.files_left,
        not_followed: report.not_followed,
    }
}

fn print_or_exit_error(out: &mut dyn Write, text: &str) -> ExitCode {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(format_args!("the report was not written: {error}")),
    }
}

#[cfg(debug_assertions)]
type MeasureStore = fn(&std::path::Path, &str) -> halvo_capture::store::StoreResult<String>;

#[cfg(debug_assertions)]
type MeasureDeletion = fn(&std::path::Path) -> halvo_capture::store::StoreResult<String>;

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

    fn recorded_store(dir: &Path, marker: &str) -> halvo_capture::store::StoreResult<String> {
        CALLS.with(|calls| {
            calls
                .borrow_mut()
                .push(format!("store {} {marker}", dir.display()))
        });
        Ok(String::new())
    }

    fn recorded_deletion(dir: &Path) -> halvo_capture::store::StoreResult<String> {
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

    fn live_store(dir: &Path) -> halvo_capture::store::db::EncryptedStore {
        use halvo_capture::custody::{dev_fallback::DevFallbackCustody, RootId};
        halvo_capture::store::db::EncryptedStore::create(
            dir,
            Box::new(DevFallbackCustody::new(
                RootId::fresh().unwrap(),
                dir.join("roots"),
            )),
            std::time::Duration::from_secs(60),
        )
        .unwrap()
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
        }
    }

    #[test]
    fn a_store_whose_root_verify_store_destroyed_loses_its_files_with_1_and_then_reads_no_store() {
        let scratch = scratch();
        let dir = scratch.dir("verified");
        halvo_capture::store::measure_store(&dir, "gate-marker-01").unwrap();
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
