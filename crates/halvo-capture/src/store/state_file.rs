//! The durable write of the daemon's state file (contracts.md C3.7): the
//! daemon's port [`StateWrite`], implemented by the store's durable-write
//! helper.

use std::ffi::OsStr;
use std::io;

use super::durable::{self, Dir, DurableFs, OsFs};
use crate::daemon::state::{StateDir, StateWrite};

/// The daemon's state-file write through the store's durable-write helper: a
/// temporary name beside the file, a full flush that reaches the drive and
/// refuses when it fails, a rename over the file and a flush of the
/// directory. A temporary name that an earlier life left is removed first.
pub struct DurableStateWrite {
    fs: Box<dyn DurableFs>,
}

impl DurableStateWrite {
    pub fn new() -> DurableStateWrite {
        DurableStateWrite { fs: Box::new(OsFs) }
    }
}

impl Default for DurableStateWrite {
    fn default() -> DurableStateWrite {
        DurableStateWrite::new()
    }
}

impl StateWrite for DurableStateWrite {
    fn write_whole(&self, dir: &StateDir, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        let held = held_dir(dir)?;
        let mut temp = name.to_os_string();
        temp.push(".new");
        let discard = |dir: &Dir, temp: &OsStr| match self.fs.remove_file(dir, temp) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
        discard(&held, &temp)?;
        durable::write_durably(
            &*self.fs,
            &held,
            &temp,
            name,
            |error| error,
            |_| Ok(()),
            discard,
            |out| out.write_all(bytes),
        )
    }
}

#[cfg(target_os = "macos")]
fn held_dir(dir: &StateDir) -> io::Result<Dir> {
    Dir::from_fd(dir.descriptor().try_clone_to_owned()?)
}

#[cfg(not(target_os = "macos"))]
fn held_dir(dir: &StateDir) -> io::Result<Dir> {
    Dir::open(dir.path())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs::File;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::super::durable::test_support::{Call, Kind, RecordingFs};
    use super::super::durable::Entry;
    use super::*;
    use crate::daemon::state::{Desired, StateError, StateFile, StateHealth};

    struct Shared(Arc<RecordingFs>);

    impl DurableFs for Shared {
        fn create_new(&self, dir: &Dir, name: &OsStr) -> io::Result<File> {
            self.0.create_new(dir, name)
        }

        fn sync_file(&self, file: &File, dir: &Dir, name: &OsStr) -> io::Result<()> {
            self.0.sync_file(file, dir, name)
        }

        fn rename(&self, dir: &Dir, from: &OsStr, to: &OsStr) -> io::Result<()> {
            self.0.rename(dir, from, to)
        }

        fn remove_file(&self, dir: &Dir, name: &OsStr) -> io::Result<()> {
            self.0.remove_file(dir, name)
        }

        fn sync_dir(&self, dir: &Dir) -> io::Result<()> {
            self.0.sync_dir(dir)
        }

        fn entries(&self, dir: &Dir) -> io::Result<Vec<Entry>> {
            self.0.entries(dir)
        }

        fn open_dir(&self, dir: &Dir, name: &OsStr) -> io::Result<Dir> {
            self.0.open_dir(dir, name)
        }
    }

    fn real(dir: &Path) -> PathBuf {
        Dir::open(dir).unwrap().path().to_path_buf()
    }

    #[test]
    fn the_state_file_is_written_by_the_durable_helper_and_a_failed_full_flush_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let fs = Arc::new(RecordingFs::new());
        let writer = DurableStateWrite {
            fs: Box::new(Shared(fs.clone())),
        };
        let mut file = StateFile::open(StateDir::open(dir.path()).unwrap(), Box::new(writer));
        let at = real(dir.path());
        let temp = at.join("daemon-state.json.new");
        let target = at.join("daemon-state.json");
        assert_eq!(
            fs.calls(),
            vec![
                Call::RemoveFile(temp.clone()),
                Call::CreateNew(temp.clone()),
                Call::SyncFile(temp.clone()),
                Call::Rename(temp.clone(), target.clone()),
                Call::SyncDir(at.clone()),
            ]
        );
        assert_eq!(file.health(), StateHealth::Ok);
        let defaults = std::fs::read(&target).unwrap();
        fs.fail(Kind::SyncFile);
        let refused = file.update(|state| state.desired = Desired::Running);
        assert!(
            matches!(
                refused,
                Err(StateError::NotWritten {
                    kind: io::ErrorKind::StorageFull,
                    ..
                })
            ),
            "{refused:?}"
        );
        assert_eq!(file.state().desired, Desired::Paused);
        assert_eq!(std::fs::read(&target).unwrap(), defaults);
        let names: Vec<OsString> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![OsString::from("daemon-state.json")]);
    }

    #[test]
    fn a_temporary_name_that_an_earlier_life_left_does_not_block_the_write() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("daemon-state.json.new"), b"left").unwrap();
        let mut file = StateFile::open(
            StateDir::open(dir.path()).unwrap(),
            Box::new(DurableStateWrite::new()),
        );
        assert_eq!(file.health(), StateHealth::Ok);
        file.update(|state| state.desired = Desired::Running)
            .unwrap();
        let reread = StateFile::open(
            StateDir::open(dir.path()).unwrap(),
            Box::new(DurableStateWrite::new()),
        );
        assert_eq!(reread.state().desired, Desired::Running);
        assert!(std::fs::symlink_metadata(dir.path().join("daemon-state.json.new")).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_write_after_the_folder_moved_lands_in_the_held_folder_and_leaves_the_second_holders_file_as_it_was(
    ) {
        use crate::daemon::instance::InstanceLock;
        use halvo_control::paths::DataDir;

        let root = crate::daemon::state::tests::test_dir();
        let root_path = root.path().canonicalize().unwrap();
        let (data, moved) = (root_path.join("data"), root_path.join("moved"));
        std::fs::create_dir(&data).unwrap();
        let named = DataDir::from_flag(data.to_str().unwrap()).unwrap();
        let first_lock = InstanceLock::take(&named).unwrap().unwrap();
        let mut first = StateFile::open(
            first_lock.state_dir().unwrap(),
            Box::new(DurableStateWrite::new()),
        );
        assert_eq!(first.health(), StateHealth::Ok);
        std::fs::rename(&data, &moved).unwrap();
        assert_eq!(first_lock.folder().unwrap(), moved);
        std::fs::create_dir(&data).unwrap();
        let second_lock = InstanceLock::take(&named).unwrap().unwrap();
        let second = StateFile::open(
            second_lock.state_dir().unwrap(),
            Box::new(DurableStateWrite::new()),
        );
        assert_eq!(second.state().desired, Desired::Paused);
        let written_by_the_second = std::fs::read(data.join("daemon-state.json")).unwrap();
        first
            .update(|state| state.desired = Desired::Running)
            .unwrap();
        assert_eq!(
            (reads_running(&moved), reads_running(&data)),
            (Some(true), Some(false)),
            "the first holder's write did not land in the folder that its lock holds"
        );
        assert_eq!(
            std::fs::read(data.join("daemon-state.json")).unwrap(),
            written_by_the_second
        );
        drop((first_lock, second_lock));
    }

    fn next_start(dir: &Path) -> (StateHealth, Desired) {
        let next = StateFile::open(
            StateDir::open(dir).unwrap(),
            Box::new(DurableStateWrite::new()),
        );
        (next.health(), next.state().desired)
    }

    fn reads_running(dir: &Path) -> Option<bool> {
        std::fs::read_to_string(dir.join("daemon-state.json"))
            .ok()
            .map(|text| text.contains("\"desired\":\"running\""))
    }

    fn recorded(dir: &Path, fs: &Arc<RecordingFs>) -> StateFile {
        StateFile::open(
            StateDir::open(dir).unwrap(),
            Box::new(DurableStateWrite {
                fs: Box::new(Shared(fs.clone())),
            }),
        )
    }

    #[test]
    fn an_error_at_each_step_of_a_later_write_leaves_memory_equal_to_what_the_next_start_reads() {
        for step in [
            Kind::RemoveFile,
            Kind::CreateNew,
            Kind::SyncFile,
            Kind::Rename,
            Kind::SyncDir,
        ] {
            let dir = crate::daemon::state::tests::test_dir();
            let fs = Arc::new(RecordingFs::new());
            let mut file = recorded(dir.path(), &fs);
            assert_eq!(
                (file.health(), file.state().desired),
                (StateHealth::Ok, Desired::Paused)
            );
            fs.fail(step);
            let answer = file.update(|state| state.desired = Desired::Running);
            let renamed = step == Kind::SyncDir;
            match (&answer, renamed) {
                (
                    Err(StateError::NotFlushed {
                        kind: io::ErrorKind::StorageFull,
                        ..
                    }),
                    true,
                )
                | (
                    Err(StateError::NotWritten {
                        kind: io::ErrorKind::StorageFull,
                        ..
                    }),
                    false,
                ) => {}
                _ => panic!("{step:?}: {answer:?}"),
            }
            assert_eq!(
                file.health(),
                StateHealth::WriteFailed(io::ErrorKind::StorageFull),
                "{step:?}"
            );
            assert_eq!(reads_running(dir.path()), Some(renamed), "{step:?}");
            assert_eq!(
                (StateHealth::Ok, file.state().desired),
                next_start(dir.path()),
                "{step:?}: memory differs from what the next start reads"
            );
        }
    }

    #[test]
    fn a_pause_whose_folder_flush_fails_leaves_memory_no_more_capturing_than_the_file() {
        let dir = crate::daemon::state::tests::test_dir();
        let fs = Arc::new(RecordingFs::new());
        let mut file = recorded(dir.path(), &fs);
        file.update(|state| state.desired = Desired::Running)
            .unwrap();
        fs.fail(Kind::SyncDir);
        let answer = file.update(|state| state.desired = Desired::Paused);
        assert!(
            matches!(answer, Err(StateError::NotFlushed { .. })),
            "{answer:?}"
        );
        assert_eq!(reads_running(dir.path()), Some(false));
        assert_eq!(file.state().desired, Desired::Paused);
        assert_eq!(next_start(dir.path()), (StateHealth::Ok, Desired::Paused));
    }

    struct StepFs(Arc<std::sync::Mutex<Option<Kind>>>);

    impl StepFs {
        fn at(&self, kind: Kind) -> io::Result<()> {
            if *self.0.lock().unwrap() == Some(kind) {
                return Err(io::Error::from(io::ErrorKind::StorageFull));
            }
            Ok(())
        }
    }

    impl DurableFs for StepFs {
        fn create_new(&self, dir: &Dir, name: &OsStr) -> io::Result<File> {
            self.at(Kind::CreateNew)?;
            OsFs.create_new(dir, name)
        }

        fn sync_file(&self, file: &File, dir: &Dir, name: &OsStr) -> io::Result<()> {
            self.at(Kind::SyncFile)?;
            OsFs.sync_file(file, dir, name)
        }

        fn rename(&self, dir: &Dir, from: &OsStr, to: &OsStr) -> io::Result<()> {
            self.at(Kind::Rename)?;
            OsFs.rename(dir, from, to)
        }

        fn remove_file(&self, dir: &Dir, name: &OsStr) -> io::Result<()> {
            self.at(Kind::RemoveFile)?;
            OsFs.remove_file(dir, name)
        }

        fn sync_dir(&self, dir: &Dir) -> io::Result<()> {
            self.at(Kind::SyncDir)?;
            OsFs.sync_dir(dir)
        }

        fn entries(&self, dir: &Dir) -> io::Result<Vec<Entry>> {
            OsFs.entries(dir)
        }

        fn open_dir(&self, dir: &Dir, name: &OsStr) -> io::Result<Dir> {
            OsFs.open_dir(dir, name)
        }
    }

    #[test]
    fn after_a_failed_folder_flush_a_record_of_a_grant_writes_no_older_desired_back() {
        use halvo_control::onboarding::Permission;

        let dir = crate::daemon::state::tests::test_dir();
        let step = Arc::new(std::sync::Mutex::new(None));
        let mut file = StateFile::open(
            StateDir::open(dir.path()).unwrap(),
            Box::new(DurableStateWrite {
                fs: Box::new(StepFs(step.clone())),
            }),
        );
        *step.lock().unwrap() = Some(Kind::SyncDir);
        let resume = file.update(|state| state.desired = Desired::Running);
        assert!(
            matches!(resume, Err(StateError::NotFlushed { .. })),
            "{resume:?}"
        );
        assert_eq!(reads_running(dir.path()), Some(true));
        *step.lock().unwrap() = None;
        file.record_granted(&[Permission::InputMonitoring]).unwrap();
        assert_eq!(
            reads_running(dir.path()),
            Some(true),
            "the record wrote an older desired back"
        );
        assert_eq!(file.state().desired, Desired::Running);
        assert_eq!(file.health(), StateHealth::Ok);
    }

    #[cfg(all(target_os = "macos", debug_assertions))]
    mod held_folder {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        use std::time::Duration;

        use halvo_control::onboarding::Permission;
        use halvo_control::paths::DataDir;

        use super::super::DurableStateWrite;
        use crate::daemon::instance::tests::{backup_mark, kept_dir};
        use crate::daemon::instance::InstanceLock;
        use crate::daemon::main_loop::{EndCall, MainLoop};
        use crate::daemon::state::{Desired, StateDir, StateFile, StateHealth, StateWrite};

        struct NoTurn(Duration);

        impl MainLoop for NoTurn {
            fn turn(&mut self, wait: Duration) {
                self.0 += wait;
            }

            fn now(&self) -> Duration {
                self.0
            }
        }

        struct NoEnding;

        impl EndCall for NoEnding {
            fn end(&self, status: i32) {
                panic!("the service run made the ending call with {status}");
            }
        }

        fn serve_with(dir: &Path, writer: Box<dyn StateWrite>) -> (i32, String) {
            let stop = AtomicBool::new(true);
            let mut err = Vec::new();
            let status = crate::daemon::serve(
                &named(dir),
                writer,
                NoTurn(Duration::ZERO),
                &stop,
                Arc::new(NoEnding),
                &mut err,
            );
            (status, String::from_utf8(err).unwrap())
        }

        fn named(path: &Path) -> DataDir {
            DataDir::from_flag(path.to_str().unwrap()).unwrap()
        }

        fn names(dir: &Path) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }

        fn moved_folder(root: &Path) -> (PathBuf, PathBuf) {
            let data = root.join("data");
            std::fs::create_dir(&data).unwrap();
            (data, root.join("moved"))
        }

        fn file_text(dir: &Path) -> String {
            String::from_utf8(std::fs::read(dir.join("daemon-state.json")).unwrap()).unwrap()
        }

        #[test]
        fn a_grant_recorded_after_the_held_folder_moved_lands_in_the_held_folder_and_not_at_its_old_name(
        ) {
            let root = kept_dir("state-file-tests-");
            let (data, moved) = moved_folder(&root);
            let lock = InstanceLock::take(&named(&data)).unwrap().unwrap();
            let mut state = StateFile::open(
                lock.state_dir().unwrap(),
                Box::new(DurableStateWrite::new()),
            );
            assert_eq!(state.health(), StateHealth::Ok);
            std::fs::rename(&data, &moved).unwrap();
            std::fs::create_dir(&data).unwrap();
            let second = InstanceLock::take(&named(&data)).unwrap();
            assert!(second.is_some(), "a second take of the name answered held");
            state
                .record_granted(&[Permission::InputMonitoring])
                .unwrap();
            assert!(
                file_text(&moved).contains("input_monitoring"),
                "the grant is not in the folder that the lock holds: {}",
                file_text(&moved)
            );
            assert!(names(&data).is_empty(), "{:?}", names(&data));
            drop((second, lock));
        }

        struct MoveThenWrite {
            held: PathBuf,
            moved: PathBuf,
            inner: DurableStateWrite,
        }

        impl StateWrite for MoveThenWrite {
            fn write_whole(
                &self,
                dir: &StateDir,
                name: &std::ffi::OsStr,
                bytes: &[u8],
            ) -> std::io::Result<()> {
                if !self.moved.exists() {
                    std::fs::rename(&self.held, &self.moved)?;
                    std::fs::create_dir(&self.held)?;
                }
                self.inner.write_whole(dir, name, bytes)
            }
        }

        #[test]
        fn the_first_state_write_of_a_service_run_lands_in_the_held_folder_when_the_folder_moves_before_it(
        ) {
            let root = kept_dir("state-file-tests-");
            let (data, moved) = moved_folder(&root);
            let writer = MoveThenWrite {
                held: data.clone(),
                moved: moved.clone(),
                inner: DurableStateWrite::new(),
            };
            let (status, err) = serve_with(&data, Box::new(writer));
            assert_eq!((status, err.as_str()), (0, ""));
            assert_eq!(names(&moved), vec!["daemon-state.json".to_owned()]);
            assert!(backup_mark(&moved, false).is_some());
            assert!(names(&data).is_empty(), "{:?}", names(&data));
            assert!(backup_mark(&data, false).is_none());
        }

        #[test]
        fn a_later_state_write_lands_in_the_held_folder_after_a_move_and_the_folder_at_the_name_gets_nothing(
        ) {
            let root = kept_dir("state-file-tests-");
            let (data, moved) = moved_folder(&root);
            let lock = InstanceLock::take(&named(&data)).unwrap().unwrap();
            let mut state = StateFile::open(
                lock.state_dir().unwrap(),
                Box::new(DurableStateWrite::new()),
            );
            std::fs::rename(&data, &moved).unwrap();
            std::fs::create_dir(&data).unwrap();
            state
                .update(|state| state.desired = Desired::Running)
                .unwrap();
            let second = InstanceLock::take(&named(&data)).unwrap();
            assert!(second.is_some(), "a second take of the name answered held");
            assert!(
                file_text(&moved).contains("\"desired\":\"running\""),
                "{}",
                file_text(&moved)
            );
            assert!(names(&data).is_empty(), "{:?}", names(&data));
            drop((second, lock));
        }

        #[test]
        fn a_service_run_on_a_data_directory_reached_through_a_link_writes_its_state_file_in_the_linked_folder(
        ) {
            use std::os::unix::fs::PermissionsExt;
            let root = kept_dir("state-file-tests-");
            let real = root.join("real");
            std::fs::create_dir(&real).unwrap();
            std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
            let link = root.join("link");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let (status, err) = serve_with(&link, Box::new(DurableStateWrite::new()));
            assert_eq!((status, err.as_str()), (0, ""));
            assert_eq!(names(&real), vec!["daemon-state.json".to_owned()]);
        }
    }
}
