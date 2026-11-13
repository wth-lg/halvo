//! The cases of the coordinator's ruling CP-124 for this crate. Each uses only calls that the
//! crate had before the ruling, so each also runs against the code that the ruling changed.
mod common;

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use common::{Scratch, RUN};
use halvo_checkpoint::{checkpoint_name, remove, Expect, Locks, Removal, Row, RunId, Sha256};

fn run() -> RunId {
    RunId::parse(RUN).expect("a run id")
}

fn made(bytes: &[u8]) -> Expect {
    Expect::Made {
        sha256: Sha256::of(bytes),
        bytes: bytes.len() as u64,
    }
}

#[cfg(unix)]
fn removing(dir: &Path) -> usize {
    fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".halvo-removing-")
            })
            .count()
    })
}

/// The review's case P4b: a party that writes in the folder flips a directory on the way
/// between a real directory and a link to a directory outside the folder while `remove` runs.
/// Outside lies a file with the checkpoint's name and bytes. No round may remove it, and no
/// copy may stay under a removing name.
#[cfg(unix)]
#[test]
fn a_directory_flipped_to_a_link_during_remove_never_touches_a_file_outside_the_folder() {
    let scratch = Scratch::new("cp124-p4b");
    let folder = scratch.path("folder");
    let outside = scratch.path("outside");
    let data = scratch.path("data");
    for dir in [&folder.join("sub"), &outside, &data] {
        fs::create_dir_all(dir).expect("make the directory");
    }
    let locks = Locks::open(&data, Duration::from_secs(5)).expect("the locks open");
    let run = run();
    let workbook = folder.join("sub").join("budget.xlsx");
    fs::write(&workbook, b"the workbook now").expect("the workbook");
    let name = checkpoint_name(&workbook, &run).expect("a name");
    let bytes = b"the workbook as it was before the run".to_vec();
    let stop = Arc::new(AtomicBool::new(false));
    let racer = {
        let (folder, outside, stop) = (folder.clone(), outside.clone(), stop.clone());
        thread::spawn(move || {
            let sub = folder.join("sub");
            let real = folder.join("sub-real");
            let parked = folder.join("link-parked");
            std::os::unix::fs::symlink(&outside, &parked).expect("the link");
            let mut flips = 0u64;
            while !stop.load(Ordering::SeqCst) {
                if fs::rename(&sub, &real).is_ok() && fs::rename(&parked, &sub).is_ok() {
                    flips += 1;
                }
                thread::sleep(Duration::from_micros(150 + (flips % 7) * 40));
                if fs::rename(&sub, &parked).is_ok() && fs::rename(&real, &sub).is_ok() {
                    flips += 1;
                }
                thread::sleep(Duration::from_micros(150 + (flips % 5) * 60));
            }
            if fs::symlink_metadata(&sub).is_ok_and(|meta| meta.file_type().is_symlink()) {
                let _ = fs::rename(&sub, &parked);
                let _ = fs::rename(&real, &sub);
            }
            flips
        })
    };
    let rounds = 1500;
    let (mut removed, mut outside_touched) = (0, 0);
    for _ in 0..rounds {
        let real = if fs::symlink_metadata(folder.join("sub")).is_ok_and(|meta| meta.is_dir()) {
            folder.join("sub")
        } else {
            folder.join("sub-real")
        };
        let _ = fs::write(real.join(&name), &bytes);
        fs::write(outside.join(&name), &bytes).expect("the person's copy outside");
        let row = Row {
            folder: &folder,
            workbook: &workbook,
            run: &run,
            expect: made(&bytes),
        };
        if remove(&locks, &row) == Ok(Removal::Removed) {
            removed += 1;
        }
        let kept = fs::read(outside.join(&name)).is_ok_and(|read| read == bytes);
        if !kept || removing(&outside) != 0 {
            outside_touched += 1;
        }
    }
    stop.store(true, Ordering::SeqCst);
    let flips = racer.join().expect("the racer ends");
    let left = removing(&folder.join("sub")) + removing(&folder.join("sub-real"));
    println!(
        "cp124 p4b rounds={rounds} flips={flips} removed={removed} \
         outside_touched={outside_touched} left_under_a_removing_name={left}"
    );
    assert_eq!(outside_touched, 0, "a file outside the folder was touched");
    assert_eq!(left, 0, "a copy stayed under a removing name");
}

/// The review's patch remove-lock-released-at-once: `remove` holds the lock of the directory
/// from its first look at the copy to the removal, so a second holder that tries without a
/// wait while `remove` reads a large copy is refused again and again.
#[test]
fn the_removal_holds_the_lock_while_it_reads_and_removes_the_copy() {
    let scratch = Scratch::new("cp124-lock-held");
    let folder = scratch.path("folder");
    fs::create_dir_all(scratch.path("data")).expect("make the data directory");
    let bytes = vec![b'c'; 8 << 20];
    scratch.write("folder/values.xlsx", b"the workbook now");
    let name = checkpoint_name(Path::new("values.xlsx"), &run()).expect("a name");
    fs::write(folder.join(&name), &bytes).expect("the checkpoint");
    let locks = Locks::open(&scratch.path("data"), Duration::from_secs(5)).expect("the locks");
    let other = Locks::open(&scratch.path("data"), Duration::ZERO).expect("the second locks");
    let stop = Arc::new(AtomicBool::new(false));
    let refused = Arc::new(AtomicUsize::new(0));
    let spinner = {
        let (folder, stop, refused) = (folder.clone(), stop.clone(), refused.clone());
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if other.hold(&folder).is_err() {
                    refused.fetch_add(1, Ordering::SeqCst);
                }
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    thread::sleep(Duration::from_millis(20));
    let before = refused.load(Ordering::SeqCst);
    let run = run();
    let row = Row {
        folder: &folder,
        workbook: Path::new("values.xlsx"),
        run: &run,
        expect: made(&bytes),
    };
    let answer = remove(&locks, &row);
    stop.store(true, Ordering::SeqCst);
    spinner.join().expect("the spinner ends");
    let during = refused.load(Ordering::SeqCst) - before;
    println!("cp124 lock-held refused_during_remove={during}");
    assert_eq!(answer, Ok(Removal::Removed));
    assert!(
        during >= 5,
        "a second holder took the lock while remove read the copy: refused {during} times"
    );
}

/// The review's patch marked-no-places: `Locks::marked` checks that `agents/locks/` is still
/// the directory that the locks opened. A directory put in its place that holds a left mark of
/// the same name, while a live call holds its mark in the real one, answers an error and its
/// mark stays.
#[cfg(unix)]
#[test]
fn marked_refuses_a_directory_put_in_the_place_of_agents_locks() {
    let scratch = Scratch::new("cp124-marked-places");
    let data = scratch.path("data");
    let folder = scratch.path("folder");
    fs::create_dir_all(&data).expect("make the data directory");
    fs::create_dir_all(&folder).expect("make the folder");
    let locks = Locks::open(&data, Duration::from_millis(200)).expect("the locks open");
    let name = std::ffi::OsStr::new("values.halvo-prepared-copy.xlsx");
    let live = locks
        .mark(&folder, name)
        .expect("a live call marks its copy");
    let agents = data.join("agents");
    let marks: Vec<_> = fs::read_dir(agents.join("locks"))
        .expect("list agents/locks")
        .flatten()
        .map(|entry| entry.file_name())
        .filter(|entry| entry.to_string_lossy().ends_with(".live"))
        .collect();
    assert_eq!(marks.len(), 1, "the live call's mark");
    fs::rename(agents.join("locks"), agents.join("locks-real")).expect("move agents/locks");
    fs::create_dir(agents.join("locks")).expect("put a directory in its place");
    let stale = agents.join("locks").join(&marks[0]);
    fs::write(&stale, b"").expect("a left mark of the same name");
    let answer = locks.marked(&folder, name);
    assert!(answer.is_err(), "marked answered {answer:?}");
    assert!(
        stale.exists(),
        "the mark in the directory put in place stays"
    );
    drop(live);
}

/// The Debug form of a held lock names no directory of the test.
#[test]
fn the_debug_form_of_a_held_lock_names_no_path() {
    let scratch = Scratch::new("cp124-held-debug");
    let data = scratch.path("data");
    fs::create_dir_all(&data).expect("make the data directory");
    let locks = Locks::open(&data, Duration::from_millis(200)).expect("the locks open");
    let held = locks.hold(&scratch.dir).expect("the lock");
    let text = format!("{held:?}");
    let base = scratch
        .dir
        .file_name()
        .expect("a name")
        .to_string_lossy()
        .into_owned();
    for part in [base.as_str(), "data", "agents", "locks"] {
        assert!(!text.contains(part), "{text} names {part}");
    }
}

/// E5 of CP-124, the verifier's M04: the SHA-256 is compared before the copy is moved aside. A
/// copy of the row's length but other bytes in a directory that cannot be written is answered
/// as changed, not as a removal that the system refused.
#[cfg(unix)]
#[test]
fn a_copy_of_other_bytes_in_a_directory_that_cannot_be_written_is_changed() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("cp124-m04");
    let folder = scratch.path("folder");
    fs::create_dir_all(scratch.path("data")).expect("make the data directory");
    let row_bytes = b"the workbook as prepare read it".to_vec();
    let mut other = row_bytes.clone();
    other[0] ^= 1;
    scratch.write("folder/values.xlsx", b"the workbook now");
    let name = checkpoint_name(Path::new("values.xlsx"), &run()).expect("a name");
    fs::write(folder.join(&name), &other).expect("the checkpoint");
    let locks = Locks::open(&scratch.path("data"), Duration::from_secs(5)).expect("the locks");
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o500)).expect("read-only");
    let run = run();
    let row = Row {
        folder: &folder,
        workbook: Path::new("values.xlsx"),
        run: &run,
        expect: made(&row_bytes),
    };
    let answer = remove(&locks, &row);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).expect("writable");
    assert_eq!(answer, Ok(Removal::Left(halvo_checkpoint::Left::Changed)));
    assert_eq!(fs::read(folder.join(&name)).expect("read"), other);
}

/// E5 of CP-124, the verifier's M28: the lock file and the mark are made with mode 0600.
#[cfg(unix)]
#[test]
fn the_lock_file_and_the_mark_have_mode_0600() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("cp124-m28");
    let data = scratch.path("data");
    fs::create_dir_all(&data).expect("make the data directory");
    let locks = Locks::open(&data, Duration::from_millis(200)).expect("the locks open");
    let held = locks.hold(&scratch.dir).expect("the lock");
    let mark = locks
        .mark(&scratch.dir, std::ffi::OsStr::new("values.xlsx"))
        .expect("a mark");
    let modes: Vec<(String, u32)> = fs::read_dir(data.join("agents/locks"))
        .expect("list agents/locks")
        .flatten()
        .map(|entry| {
            let mode = entry.metadata().expect("metadata").permissions().mode() & 0o777;
            let name = entry.file_name().to_string_lossy().into_owned();
            (
                name.rsplit('.').next().unwrap_or_default().to_string(),
                mode,
            )
        })
        .collect();
    drop(mark);
    drop(held);
    assert_eq!(modes.len(), 2, "{modes:?}");
    for (kind, mode) in &modes {
        assert_eq!(*mode, 0o600, "the {kind} file");
    }
}
