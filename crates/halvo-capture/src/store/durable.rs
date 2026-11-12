use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

pub(crate) trait DurableFs: Send + Sync {
    fn create_new(&self, path: &Path) -> io::Result<File>;
    fn sync_file(&self, file: &File, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn sync_dir(&self, dir: &Path) -> io::Result<()>;
}

pub(crate) struct OsFs;

impl DurableFs for OsFs {
    fn create_new(&self, path: &Path) -> io::Result<File> {
        let mut options = File::options();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(path)
    }

    fn sync_file(&self, file: &File, _path: &Path) -> io::Result<()> {
        file.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    #[cfg(unix)]
    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        File::open(dir)?.sync_all()
    }

    #[cfg(not(unix))]
    fn sync_dir(&self, _dir: &Path) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn write_durably<T, E>(
    fs: &dyn DurableFs,
    temp: &Path,
    target: &Path,
    io_error: fn(io::Error) -> E,
    discard: impl FnOnce(&Path) -> Result<(), E>,
    fill: impl FnOnce(&mut dyn Write) -> Result<T, E>,
) -> Result<T, E> {
    let file = fs.create_new(temp).map_err(io_error)?;
    match fill_and_publish(fs, file, temp, target, io_error, fill) {
        Ok(value) => Ok(value),
        Err(failed) => match discard(temp) {
            Ok(()) => Err(failed),
            Err(left_behind) => Err(left_behind),
        },
    }
}

fn fill_and_publish<T, E>(
    fs: &dyn DurableFs,
    file: File,
    temp: &Path,
    target: &Path,
    io_error: fn(io::Error) -> E,
    fill: impl FnOnce(&mut dyn Write) -> Result<T, E>,
) -> Result<T, E> {
    let mut writer = BufWriter::new(file);
    let value = fill(&mut writer)?;
    writer.flush().map_err(io_error)?;
    fs.sync_file(writer.get_ref(), temp).map_err(io_error)?;
    drop(writer);
    fs.rename(temp, target).map_err(io_error)?;
    sync_parent(fs, target).map_err(io_error)?;
    Ok(value)
}

#[cfg(target_os = "macos")]
pub(crate) fn no_follow(options: &mut std::fs::OpenOptions) -> &mut std::fs::OpenOptions {
    std::os::unix::fs::OpenOptionsExt::custom_flags(options, libc::O_NOFOLLOW);
    options
}

#[cfg(windows)]
pub(crate) fn no_follow(options: &mut std::fs::OpenOptions) -> &mut std::fs::OpenOptions {
    std::os::windows::fs::OpenOptionsExt::custom_flags(options, FILE_FLAG_OPEN_REPARSE_POINT);
    options
}

#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

pub(crate) fn create_private_dir_all(dir: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

pub(crate) fn make_private(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

pub(crate) fn remove_durably(fs: &dyn DurableFs, path: &Path) -> io::Result<()> {
    match fs.remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    sync_parent(fs, path)
}

fn sync_parent(fs: &dyn DurableFs, path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => fs.sync_dir(dir),
        _ => fs.sync_dir(Path::new(".")),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::fs::File;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use super::{DurableFs, OsFs};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Call {
        CreateNew(PathBuf),
        SyncFile(PathBuf),
        Rename(PathBuf, PathBuf),
        RemoveFile(PathBuf),
        SyncDir(PathBuf),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Kind {
        CreateNew,
        SyncFile,
        Rename,
        RemoveFile,
        SyncDir,
    }

    impl Call {
        fn kind(&self) -> Kind {
            match self {
                Call::CreateNew(_) => Kind::CreateNew,
                Call::SyncFile(_) => Kind::SyncFile,
                Call::Rename(..) => Kind::Rename,
                Call::RemoveFile(_) => Kind::RemoveFile,
                Call::SyncDir(_) => Kind::SyncDir,
            }
        }
    }

    type Observer = Box<dyn Fn(&Call) + Send + Sync>;

    #[derive(Default)]
    pub(crate) struct RecordingFs {
        calls: Mutex<Vec<Call>>,
        failing: Mutex<Vec<Kind>>,
        observer: Option<Observer>,
    }

    impl RecordingFs {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn observed_by(observer: impl Fn(&Call) + Send + Sync + 'static) -> Self {
            Self {
                observer: Some(Box::new(observer)),
                ..Self::default()
            }
        }

        pub(crate) fn fail(&self, kind: Kind) {
            self.failing.lock().unwrap().push(kind);
        }

        pub(crate) fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: Call) -> io::Result<()> {
            if let Some(observer) = &self.observer {
                observer(&call);
            }
            let kind = call.kind();
            self.calls.lock().unwrap().push(call);
            if self.failing.lock().unwrap().contains(&kind) {
                return Err(io::Error::from(io::ErrorKind::StorageFull));
            }
            Ok(())
        }
    }

    impl DurableFs for RecordingFs {
        fn create_new(&self, path: &Path) -> io::Result<File> {
            self.record(Call::CreateNew(path.to_path_buf()))?;
            OsFs.create_new(path)
        }

        fn sync_file(&self, file: &File, path: &Path) -> io::Result<()> {
            self.record(Call::SyncFile(path.to_path_buf()))?;
            OsFs.sync_file(file, path)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.record(Call::Rename(from.to_path_buf(), to.to_path_buf()))?;
            OsFs.rename(from, to)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.record(Call::RemoveFile(path.to_path_buf()))?;
            OsFs.remove_file(path)
        }

        fn sync_dir(&self, dir: &Path) -> io::Result<()> {
            self.record(Call::SyncDir(dir.to_path_buf()))?;
            OsFs.sync_dir(dir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{Call, Kind, RecordingFs};
    use super::*;

    #[test]
    fn a_durable_write_syncs_the_file_then_publishes_it_then_syncs_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let (temp, target) = (dir.path().join("file.tmp"), dir.path().join("file"));
        let fs = RecordingFs::new();
        let written = write_durably(
            &fs,
            &temp,
            &target,
            |e| e,
            |_| Ok(()),
            |out| out.write_all(b"durable bytes").map(|()| 13),
        );
        assert_eq!(written.unwrap(), 13);
        assert_eq!(
            fs.calls(),
            vec![
                Call::CreateNew(temp.clone()),
                Call::SyncFile(temp.clone()),
                Call::Rename(temp.clone(), target.clone()),
                Call::SyncDir(dir.path().to_path_buf()),
            ]
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"durable bytes");
        assert!(!temp.exists());
    }

    #[test]
    fn a_durable_write_on_a_full_volume_publishes_nothing() {
        for kind in [Kind::CreateNew, Kind::SyncFile, Kind::Rename] {
            let dir = tempfile::tempdir().unwrap();
            let (temp, target) = (dir.path().join("file.tmp"), dir.path().join("file"));
            let fs = RecordingFs::new();
            fs.fail(kind);
            let written = write_durably(
                &fs,
                &temp,
                &target,
                |e| e,
                |_| Ok(()),
                |out| out.write_all(b"bytes"),
            );
            assert_eq!(
                written.map_err(|e| e.kind()),
                Err(std::io::ErrorKind::StorageFull),
                "{kind:?}"
            );
            assert!(
                !target.exists(),
                "a write that failed at {kind:?} published the file"
            );
        }
    }

    #[test]
    fn a_durable_write_never_writes_through_an_existing_temporary_name() {
        let dir = tempfile::tempdir().unwrap();
        let (temp, target) = (dir.path().join("file.tmp"), dir.path().join("file"));
        std::fs::write(&temp, b"left by another writer").unwrap();
        let written = write_durably(
            &OsFs,
            &temp,
            &target,
            |e| e,
            |_| Ok(()),
            |out| out.write_all(b"new"),
        );
        assert_eq!(
            written.map_err(|e| e.kind()),
            Err(std::io::ErrorKind::AlreadyExists)
        );
        assert_eq!(std::fs::read(&temp).unwrap(), b"left by another writer");
        assert!(!target.exists());
    }

    #[test]
    fn a_durable_removal_removes_the_file_then_syncs_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        std::fs::write(&path, b"bytes").unwrap();
        let fs = RecordingFs::new();
        remove_durably(&fs, &path).unwrap();
        remove_durably(&fs, &path).unwrap();
        assert_eq!(
            fs.calls(),
            vec![
                Call::RemoveFile(path.clone()),
                Call::SyncDir(dir.path().to_path_buf()),
                Call::RemoveFile(path.clone()),
                Call::SyncDir(dir.path().to_path_buf()),
            ]
        );
        assert!(!path.exists());
    }
}
