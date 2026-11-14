//! The cases of the coordinator's ruling CP-129: every check that says "this is the file that I
//! checked" compares the identity that the system gives for an open handle, through one
//! function. The case of equal times uses only calls that the crate had before the ruling, so it
//! also runs against the code that the ruling changed.
mod common;

use std::fs::{self, File, FileTimes, OpenOptions};

use common::Scratch;
use halvo_checkpoint::same_entry;

const BYTES: &[u8] = b"the workbook as prepare read it";

/// A file of the same bytes, the same length and, set on purpose, the same creation time and
/// time of the last write as the checked file, moved into its place, is not the checked file.
/// The case does not rest on the grain of the system's clock.
#[test]
fn a_file_of_the_same_bytes_length_and_times_in_the_checked_files_place_is_not_the_checked_file() {
    let scratch = Scratch::new("cp129-equal-times");
    let path = scratch.write("values.halvo-checkpoint-x.xlsx", BYTES);
    let checked = File::open(&path).expect("open the checked file");
    let seen = checked.metadata().expect("its metadata");
    let other = scratch.write("person.xlsx", BYTES);
    let times = FileTimes::new().set_modified(seen.modified().expect("modified"));
    #[cfg(windows)]
    let times = {
        use std::os::windows::fs::FileTimesExt;
        times.set_created(seen.created().expect("created"))
    };
    OpenOptions::new()
        .write(true)
        .open(&other)
        .expect("open the other file")
        .set_times(times)
        .expect("set its times");
    fs::rename(&other, &path).expect("move it into the checked file's place");
    let now = fs::symlink_metadata(&path).expect("the entry now");
    let same_len = now.len() == seen.len();
    let same_written = now.modified().ok() == seen.modified().ok();
    let same_created = now.created().ok() == seen.created().ok();
    println!("cp129 equal-times len={same_len} written={same_written} created={same_created}");
    assert!(
        same_len && same_written,
        "the case needs equal length and times"
    );
    #[cfg(windows)]
    assert!(
        same_created,
        "the case needs equal creation times on Windows"
    );
    assert!(
        !same_entry(&checked, &path),
        "the file put in the checked file's place was taken for the checked file"
    );
    assert!(same_entry(&File::open(&path).expect("open"), &path));
}
