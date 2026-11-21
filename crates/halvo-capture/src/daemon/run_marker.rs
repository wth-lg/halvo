//! The run marker of a service run, run/running of the data directory
//! (contracts.md C3.2 step 5 and its stop, D-05): read at the start to set
//! `last_stop`, written anew for the life that starts, and removed by a life
//! that ends in order. It holds the process id of its life alone.

use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

use halvo_control::paths::DataDir;
use halvo_control::proto::LastStop;

const MARKER_READ_MAX_BYTES: u64 = 4_096;

/// Reads the marker that the last life left and writes the marker of this
/// one. No marker and no run/ before it is a first life (`none`); no marker
/// in a run/ that stands is a life that ended in order (`clean`); a marker
/// that stands is a life that did not remove it (`unclean`). A marker that
/// cannot be read, or one that cannot be written, is an error; the reading
/// is then `unclean`.
pub fn begin(dir: &DataDir) -> (LastStop, io::Result<()>) {
    let run = dir.run_dir();
    let run_stood = std::fs::symlink_metadata(&run).is_ok_and(|meta| meta.is_dir());
    let path = dir.run_marker();
    let read = match std::fs::File::open(&path) {
        Ok(file) => {
            let mut held = Vec::new();
            file.take(MARKER_READ_MAX_BYTES)
                .read_to_end(&mut held)
                .map(|_| Some(()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    };
    let last_stop = match &read {
        Ok(None) if run_stood => LastStop::Clean,
        Ok(None) => LastStop::None,
        Ok(Some(())) | Err(_) => LastStop::Unclean,
    };
    let written = read.and_then(|_| write(dir));
    (last_stop, written)
}

/// Removes the marker of this life and syncs run/, as a life that ended in
/// order does.
pub fn end(dir: &DataDir) -> io::Result<()> {
    match std::fs::remove_file(dir.run_marker()) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    std::fs::File::open(dir.run_dir())?.sync_all()
}

fn write(dir: &DataDir) -> io::Result<()> {
    let run = dir.run_dir();
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&run)?;
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700))?;
    let path = dir.run_marker();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    let next = path.with_file_name(name);
    let mut file = std::fs::File::create(&next)?;
    file.write_all(format!("pid={}\n", std::process::id()).as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&next, &path)?;
    std::fs::File::open(&run)?.sync_all()
}
