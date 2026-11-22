//! The run marker of a service run, run/running of the data directory
//! (contracts.md C3.2 step 5 and its stop, D-05): read at the start to set
//! `last_stop`, written anew for the life that starts, and removed by a life
//! that ends in order. It holds the process id of its life, and the hold of
//! ruling PL-3 part (5) while that stands: a record of a life that a source
//! which did not stop ended, or the line `hold=yes` that carries such a
//! record's hold on through the lives after it until a Resume lifts it.

use std::io::{self, Read, Write};
#[cfg(target_os = "macos")]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;

use halvo_control::paths::DataDir;
use halvo_control::proto::LastStop;

const MARKER_READ_MAX_BYTES: u64 = 4_096;

const HOLD_LINE: &str = "hold=yes";

/// Reads the marker that the last life left and writes the marker of this
/// one, with the hold when the marker that stood held it. No marker and no
/// run/ before it is a first life (`none`); no marker in a run/ that stands,
/// or a marker that holds the hold alone, is a life that ended in order
/// (`clean`); any other marker that stands is a life that did not remove it
/// (`unclean`). A marker that cannot be read, or one that cannot be
/// written, is an error; the reading is then `unclean`.
pub fn begin(dir: &DataDir) -> (LastStop, io::Result<()>) {
    let run = dir.run_dir();
    let run_stood = std::fs::symlink_metadata(&run).is_ok_and(|meta| meta.is_dir());
    let read = read(dir);
    let last_stop = match &read {
        Ok(None) if run_stood => LastStop::Clean,
        Ok(None) => LastStop::None,
        Ok(Some(text)) if text.trim_end() == HOLD_LINE => LastStop::Clean,
        Ok(Some(_)) | Err(_) => LastStop::Unclean,
    };
    let written = read.and_then(|text| write(dir, text.as_deref().is_some_and(holds_the_hold)));
    (last_stop, written)
}

/// Whether the marker that stands holds the hold of ruling PL-3 part (5).
pub fn held(dir: &DataDir) -> bool {
    read(dir).is_ok_and(|text| text.as_deref().is_some_and(holds_the_hold))
}

/// Removes the marker of this life and syncs run/, as a life that ended in
/// order does; while the hold stands, the marker keeps the hold alone.
pub fn end(dir: &DataDir) -> io::Result<()> {
    if held(dir) {
        return replace(dir, format!("{HOLD_LINE}\n").as_bytes());
    }
    match std::fs::remove_file(dir.run_marker()) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    sync_folder(&dir.run_dir())
}

fn holds_the_hold(text: &str) -> bool {
    text.lines()
        .any(|line| line == HOLD_LINE || line.starts_with("last_stop="))
}

fn read(dir: &DataDir) -> io::Result<Option<String>> {
    match std::fs::File::open(dir.run_marker()) {
        Ok(file) => {
            let mut held = Vec::new();
            file.take(MARKER_READ_MAX_BYTES).read_to_end(&mut held)?;
            Ok(Some(String::from_utf8_lossy(&held).into_owned()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write(dir: &DataDir, hold: bool) -> io::Result<()> {
    let mut text = format!("pid={}\n", std::process::id());
    if hold {
        text.push_str(HOLD_LINE);
        text.push('\n');
    }
    replace(dir, text.as_bytes())
}

fn replace(dir: &DataDir, bytes: &[u8]) -> io::Result<()> {
    let run = dir.run_dir();
    make_own_folder(&run)?;
    let path = dir.run_marker();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    let next = path.with_file_name(name);
    let mut file = std::fs::File::create(&next)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&next, &path)?;
    sync_folder(&run)
}

#[cfg(target_os = "macos")]
fn make_own_folder(run: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(run)?;
    std::fs::set_permissions(run, std::fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn make_own_folder(run: &Path) -> io::Result<()> {
    std::fs::create_dir_all(run)
}

#[cfg(target_os = "macos")]
fn sync_folder(folder: &Path) -> io::Result<()> {
    std::fs::File::open(folder)?.sync_all()
}

#[cfg(windows)]
fn sync_folder(_folder: &Path) -> io::Result<()> {
    Ok(())
}
