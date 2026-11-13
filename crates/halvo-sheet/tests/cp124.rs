//! The cases of the coordinator's ruling CP-124 for this crate. Each uses only calls that the
//! crate had before the ruling, so each also runs against the code that the ruling changed.
mod common;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use common::{corpus, link_file, Scratch, RUN};
use halvo_checkpoint::{remove, Expect, Removal, Row};
use halvo_sheet::{Cause, Folder, Locks, Range, RunId, Sha256, Value};

const OTHER: &str = "fedcba9876543210fedcba9876543210";
const CHILD_DIR: &str = "HALVO_SHEET_CP124_CHILD_DIR";
const CHILD_DATA: &str = "HALVO_SHEET_CP124_CHILD_DATA";

fn own_locks(data: &Path) -> Locks {
    Locks::open(data, Duration::from_secs(10)).expect("the locks open")
}

fn values() -> [Vec<Value>; 1] {
    [vec![Value::Number(5.0)]]
}

fn prepare_at(
    folder: &Folder,
    workbook: &Path,
    run: &str,
    cell: &str,
) -> Result<halvo_sheet::Prepared, halvo_sheet::Error> {
    let run = RunId::parse(run).expect("a run id");
    let range = Range::parse(cell).expect("a range");
    folder.prepare(workbook, &run, "Values", range, &values())
}

fn prepare(folder: &Folder, run: &str, cell: &str) -> halvo_sheet::Prepared {
    prepare_at(folder, Path::new("values.xlsx"), run, cell).expect("prepared")
}

fn copies_of(scratch: &Scratch, run: &str) -> Vec<String> {
    let mark = format!(".halvo-prepared-{run}-");
    let names = scratch.names("");
    names.into_iter().filter(|n| n.contains(&mark)).collect()
}

fn orphan_name(stem: &str, run: &str, bytes: &[u8]) -> String {
    let digest = Sha256::of(bytes).hex();
    format!(
        "{stem}.halvo-prepared-{run}-00000000000000ff-{}.xlsx",
        &digest[..16]
    )
}

#[cfg(unix)]
fn live_marks(data: &Scratch) -> usize {
    let names = data.names("agents/locks");
    names.iter().filter(|n| n.ends_with(".live")).count()
}

/// The review's case P1: a party that writes in the folder swaps a new checkpoint for a link to
/// a file outside the folder while `prepare` fills it. The file outside keeps its mode, gets no
/// extended attribute and no access list entry of the workbook, every round.
#[cfg(target_os = "macos")]
#[test]
fn a_link_swapped_in_for_a_new_checkpoint_gets_nothing_of_the_workbook() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("cp124-p1");
    let data = Scratch::new("cp124-p1-data");
    let outside = Scratch::new("cp124-p1-outside");
    let workbook = scratch.path("values.xlsx");
    fs::copy(corpus("values.xlsx"), &workbook).expect("copy");
    let tool = |program: &str, args: &[&str], path: &Path| {
        Command::new(program)
            .args(args)
            .arg(path)
            .output()
            .expect("the tool runs")
    };
    let marked = tool("xattr", &["-w", "org.example.probe", "carried"], &workbook);
    assert!(marked.status.success(), "the workbook's attribute");
    let acl = tool("chmod", &["+a", "everyone deny delete"], &workbook);
    assert!(acl.status.success(), "the workbook's access list");
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    let rounds = 30u64;
    let (mut swaps, mut touched) = (0, 0);
    for round in 0..rounds {
        let victim = outside.path(&format!("victim-{round}.txt"));
        fs::write(&victim, b"a file of the person outside the folder").expect("victim");
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).expect("mode");
        let run = format!("{:032x}", 0x00ab_c000u64 + round);
        let stop = Arc::new(AtomicBool::new(false));
        let swapped = Arc::new(AtomicBool::new(false));
        let racer = {
            let (dir, victim) = (scratch.dir.clone(), victim.clone());
            let (stop, swapped) = (stop.clone(), swapped.clone());
            let name = format!("values.halvo-checkpoint-{run}.xlsx");
            thread::spawn(move || {
                let target: PathBuf = dir.join(&name);
                while !stop.load(Ordering::SeqCst) {
                    if fs::symlink_metadata(&target).is_ok() {
                        let away = dir.join(format!("away-{name}"));
                        if fs::rename(&target, &away).is_ok()
                            && std::os::unix::fs::symlink(&victim, &target).is_ok()
                        {
                            swapped.store(true, Ordering::SeqCst);
                        }
                        return;
                    }
                }
            })
        };
        let answer = prepare_at(&folder, &workbook, &run, "B5");
        stop.store(true, Ordering::SeqCst);
        racer.join().expect("the racer ends");
        drop(answer);
        let attribute = tool("xattr", &["-p", "org.example.probe"], &victim);
        let listing = tool("ls", &["-le"], &victim);
        let entries = String::from_utf8_lossy(&listing.stdout)
            .lines()
            .filter(|line| line.trim_start().starts_with("0:"))
            .count();
        let mode = fs::metadata(&victim).expect("victim").permissions().mode() & 0o777;
        swaps += usize::from(swapped.load(Ordering::SeqCst));
        if attribute.status.success() || entries != 0 || mode != 0o600 {
            touched += 1;
        }
    }
    println!("cp124 p1 rounds={rounds} swaps={swaps} outside_files_touched={touched}");
    assert_eq!(
        touched, 0,
        "a file outside the folder took the workbook's mode, attribute or access list"
    );
}

/// The review's case P1 on Windows: a link swapped in for a new checkpoint while `prepare`
/// fills it; the file outside gets no named stream of the workbook and keeps its attributes and
/// its access list, every round.
#[cfg(windows)]
#[test]
fn a_link_swapped_in_for_a_new_checkpoint_gets_nothing_of_the_workbook() {
    let scratch = Scratch::new("cp124-p1");
    let data = Scratch::new("cp124-p1-data");
    let outside = Scratch::new("cp124-p1-outside");
    let workbook = scratch.path("values.xlsx");
    fs::copy(corpus("values.xlsx"), &workbook).expect("copy");
    let mut stream = workbook.as_os_str().to_os_string();
    stream.push(":org.example.probe");
    fs::write(&stream, b"carried").expect("the workbook's stream");
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    let rounds = 30u64;
    let (mut swaps, mut touched) = (0, 0);
    for round in 0..rounds {
        let victim = outside.path(&format!("victim-{round}.txt"));
        fs::write(&victim, b"a file of the person outside the folder").expect("victim");
        let access = |path: &Path| {
            Command::new("icacls")
                .arg(path)
                .output()
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
                .unwrap_or_default()
        };
        let before = access(&victim);
        let attributes = fs::metadata(&victim)
            .expect("victim")
            .permissions()
            .readonly();
        let run = format!("{:032x}", 0x00ab_c000u64 + round);
        let stop = Arc::new(AtomicBool::new(false));
        let swapped = Arc::new(AtomicBool::new(false));
        let racer = {
            let (dir, victim) = (scratch.dir.clone(), victim.clone());
            let (stop, swapped) = (stop.clone(), swapped.clone());
            let name = format!("values.halvo-checkpoint-{run}.xlsx");
            thread::spawn(move || {
                let target: PathBuf = dir.join(&name);
                while !stop.load(Ordering::SeqCst) {
                    if fs::symlink_metadata(&target).is_ok() {
                        let away = dir.join(format!("away-{name}"));
                        if fs::rename(&target, &away).is_ok()
                            && std::os::windows::fs::symlink_file(&victim, &target).is_ok()
                        {
                            swapped.store(true, Ordering::SeqCst);
                        }
                        return;
                    }
                }
            })
        };
        let answer = prepare_at(&folder, &workbook, &run, "B5");
        stop.store(true, Ordering::SeqCst);
        racer.join().expect("the racer ends");
        drop(answer);
        let mut carried = victim.as_os_str().to_os_string();
        carried.push(":org.example.probe");
        let readonly = fs::metadata(&victim)
            .expect("victim")
            .permissions()
            .readonly();
        let after = access(&victim).replace(&before, "");
        swaps += usize::from(swapped.load(Ordering::SeqCst));
        if fs::metadata(&carried).is_ok() || readonly != attributes || !after.trim().is_empty() {
            touched += 1;
        }
    }
    println!("cp124 p1 rounds={rounds} swaps={swaps} outside_files_touched={touched}");
    assert_eq!(
        touched, 0,
        "a file outside the folder took the workbook's stream, attributes or access list"
    );
}

fn listing(scratch: &Scratch) -> Vec<String> {
    scratch.names("")
}

/// The names of `units` units as the crate counts them, for the bound cases: ASCII, or one non-ASCII
/// character that is two bytes on macOS and one UTF-16 unit on Windows, padded with ASCII.
fn long_name(units: usize, wide: bool) -> String {
    let stem_units = units - ".xlsx".len();
    let stem = if !wide {
        "a".repeat(stem_units)
    } else if cfg!(windows) {
        "\u{e9}".repeat(stem_units)
    } else {
        let mut stem = "\u{e9}".repeat(stem_units / 2);
        stem.push_str(&"a".repeat(stem_units % 2));
        stem
    };
    format!("{stem}.xlsx")
}

fn bound_case(units: usize, wide: bool) -> (bool, Vec<String>, Vec<String>, Option<Cause>) {
    let scratch = Scratch::new(&format!("cp124-bound-{units}-{wide}"));
    let data = Scratch::new(&format!("cp124-bound-{units}-{wide}-data"));
    let name = long_name(units, wide);
    fs::copy(corpus("values.xlsx"), scratch.path(&name)).expect("copy");
    let before = listing(&scratch);
    let locks = own_locks(&data.dir);
    let folder = Folder::new(&scratch.dir, locks.clone()).expect("the folder opens");
    let answer = prepare_at(&folder, Path::new(&name), RUN, "B5");
    let cause = answer.as_ref().err().map(halvo_sheet::Error::cause);
    let prepared = answer.is_ok();
    if let Ok(prepared) = answer {
        let row_expect = Expect::Made {
            sha256: prepared.checkpoint_sha256(),
            bytes: prepared.checkpoint_bytes(),
        };
        drop(prepared);
        let run = RunId::parse(RUN).expect("a run id");
        let row = Row {
            folder: &scratch.dir,
            workbook: Path::new(&name),
            run: &run,
            expect: row_expect,
        };
        assert_eq!(
            remove(&locks, &row),
            Ok(Removal::Removed),
            "{units}: the checkpoint is removed"
        );
    }
    let after = listing(&scratch);
    (prepared, before, after, cause)
}

/// E2 of CP-124: a name of 173 units is prepared and its checkpoint removed; 174 and 205 units,
/// where the checkpoint's own name would still fit, and 206 units are refused before any write,
/// with the cause `step_failed`; one non-ASCII name at the bound each way.
#[test]
fn a_workbook_name_is_refused_before_any_write_when_a_built_name_would_be_too_long() {
    let mut misses = Vec::new();
    for (units, wide, fits) in [
        (173, false, true),
        (174, false, false),
        (205, false, false),
        (206, false, false),
        (173, true, true),
        (174, true, false),
    ] {
        let (prepared, before, after, cause) = bound_case(units, wide);
        let same = before == after;
        println!(
            "cp124 bound units={units} non_ascii={wide} prepared={prepared} cause={cause:?} \
             unchanged={same}"
        );
        println!("cp124 bound before={before:?}");
        println!("cp124 bound after={after:?}");
        let refused = fits || cause == Some(Cause::StepFailed);
        if prepared != fits || !same || !refused {
            misses.push(format!("{units} non_ascii={wide}"));
        }
    }
    assert!(misses.is_empty(), "the cases that miss: {misses:?}");
}

/// E3 of CP-124: a live copy that a call of another data directory holds has no mark in this
/// data directory; the sweep leaves it and that call's `replace` succeeds.
#[test]
fn a_live_copy_of_another_data_directory_is_not_swept_and_still_replaces() {
    let scratch = Scratch::new("cp124-other-data");
    let one = Scratch::new("cp124-other-data-one");
    let two = Scratch::new("cp124-other-data-two");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let first = Folder::new(&scratch.dir, own_locks(&one.dir)).expect("the first folder");
    let second = Folder::new(&scratch.dir, own_locks(&two.dir)).expect("the second folder");
    let held = prepare(&first, OTHER, "B5");
    let other = prepare(&second, RUN, "C7");
    assert!(held.copy().exists(), "the first call's copy stays");
    drop(other);
    first.replace(&held).expect("the first call replaces");
}

/// E3 of CP-124, the verifier's case I5: an entry with a prepared name of this workbook and
/// its own digest, but no mark, is left by the sweep.
#[test]
fn an_own_copy_with_no_mark_stays() {
    let scratch = Scratch::new("cp124-unmarked");
    let data = Scratch::new("cp124-unmarked-data");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let bytes = b"a copy whose mark is not in this data directory".as_slice();
    let orphan = orphan_name("values", OTHER, bytes);
    scratch.write(&orphan, bytes);
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    let held = prepare(&folder, RUN, "B5");
    assert!(
        scratch.names("").contains(&orphan),
        "the unmarked copy stays"
    );
    assert_eq!(fs::read(scratch.path(&orphan)).expect("read"), bytes);
    drop(held);
}

/// E3 of CP-124: when the copy's mark cannot be made, no copy is left.
#[cfg(unix)]
#[test]
fn a_mark_that_cannot_be_made_leaves_no_copy() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("cp124-no-mark");
    let data = Scratch::new("cp124-no-mark-data");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    drop(prepare(&folder, OTHER, "B5"));
    let locks_dir = data.path("agents/locks");
    fs::set_permissions(&locks_dir, fs::Permissions::from_mode(0o500)).expect("read-only");
    let answer = prepare_at(&folder, Path::new("values.xlsx"), RUN, "C7");
    fs::set_permissions(&locks_dir, fs::Permissions::from_mode(0o700)).expect("writable");
    assert!(answer.is_err(), "the call fails: {answer:?}");
    assert_eq!(
        copies_of(&scratch, RUN),
        Vec::<String>::new(),
        "no copy is left"
    );
    assert_eq!(live_marks(&data), 0, "no mark is left");
}

/// E3 of CP-124: when the copy cannot be made, its mark goes.
#[cfg(unix)]
#[test]
fn a_copy_that_cannot_be_made_leaves_no_mark() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("cp124-no-copy");
    let data = Scratch::new("cp124-no-copy-data");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    drop(prepare(&folder, RUN, "B5"));
    fs::set_permissions(&scratch.dir, fs::Permissions::from_mode(0o500)).expect("read-only");
    let answer = prepare_at(&folder, Path::new("values.xlsx"), RUN, "C7");
    fs::set_permissions(&scratch.dir, fs::Permissions::from_mode(0o700)).expect("writable");
    assert!(answer.is_err(), "the call fails: {answer:?}");
    assert_eq!(
        copies_of(&scratch, RUN),
        Vec::<String>::new(),
        "no copy is left"
    );
    assert_eq!(live_marks(&data), 0, "no mark is left");
}

#[test]
fn cp124_sweep_child() {
    let (Ok(dir), Ok(data)) = (std::env::var(CHILD_DIR), std::env::var(CHILD_DATA)) else {
        return;
    };
    let folder = Folder::new(Path::new(&dir), own_locks(Path::new(&data))).expect("the folder");
    let _held = prepare(&folder, OTHER, "B5");
    println!("child prepared");
    thread::sleep(Duration::from_secs(120));
}

/// The review's patch sweep-follows-links: a killed call left its copy and its mark; the copy
/// is then moved outside the folder and a link with the copy's name, which holds the digest of
/// its target, put in its place. The sweep takes no link: the link and its target stay.
#[test]
fn a_link_whose_name_holds_its_targets_digest_stays_and_so_does_its_target() {
    let scratch = Scratch::new("cp124-sweep-link");
    let data = Scratch::new("cp124-sweep-link-data");
    let outside = Scratch::new("cp124-sweep-link-outside");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let mut child = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "cp124_sweep_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_DIR, &scratch.dir)
        .env(CHILD_DATA, &data.dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the child");
    let stdout = child.stdout.take().expect("the child's output");
    let prepared = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.contains("child prepared"));
    assert!(prepared, "the child prepared");
    child.kill().expect("kill the child");
    child.wait().expect("the child ends");
    let left = copies_of(&scratch, OTHER);
    assert_eq!(left.len(), 1, "the killed call left its copy");
    let target = outside.path("target.xlsx");
    fs::rename(scratch.path(&left[0]), &target).expect("move the copy outside");
    let bytes = fs::read(&target).expect("read the target");
    link_file(&target, &scratch.path(&left[0]));
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    let held = prepare(&folder, RUN, "C7");
    let link = fs::symlink_metadata(scratch.path(&left[0]));
    assert!(
        link.is_ok_and(|meta| meta.file_type().is_symlink()),
        "the link stays"
    );
    assert_eq!(fs::read(&target).expect("the target stays"), bytes);
    drop(held);
}

/// The review's patch prepare-unlocked-writes: `prepare` holds the lock of the directory over
/// the sweep, the checkpoint, the mark and the copy. A second holder that takes the lock once
/// the checkpoint stands never sees the copy appear while it holds the lock.
#[test]
fn prepare_holds_its_lock_from_the_checkpoint_to_the_copy() {
    let scratch = Scratch::new("cp124-prepare-lock");
    let data = Scratch::new("cp124-prepare-lock-data");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    let rounds = 20u64;
    let crossed = Arc::new(AtomicUsize::new(0));
    let (mut took, mut prepared) = (0, 0);
    for round in 0..rounds {
        let run = format!("{:032x}", 0x00cd_e000u64 + round);
        let other = Locks::open(&data.dir, Duration::ZERO).expect("the second locks");
        let stop = Arc::new(AtomicBool::new(false));
        let spinner = {
            let (dir, stop, crossed) = (scratch.dir.clone(), stop.clone(), crossed.clone());
            let checkpoint = dir.join(format!("values.halvo-checkpoint-{run}.xlsx"));
            let copy_mark = format!(".halvo-prepared-{run}-");
            thread::spawn(move || {
                let copy_stands = || {
                    fs::read_dir(&dir).is_ok_and(|entries| {
                        entries
                            .flatten()
                            .any(|e| e.file_name().to_string_lossy().contains(&copy_mark))
                    })
                };
                while !stop.load(Ordering::SeqCst) {
                    if !checkpoint.exists() {
                        continue;
                    }
                    let Ok(held) = other.hold(&dir) else {
                        continue;
                    };
                    let before = copy_stands();
                    thread::sleep(Duration::from_millis(30));
                    if !before && copy_stands() {
                        crossed.fetch_add(1, Ordering::SeqCst);
                    }
                    drop(held);
                    return true;
                }
                false
            })
        };
        let answer = prepare_at(&folder, Path::new("values.xlsx"), &run, "B5");
        stop.store(true, Ordering::SeqCst);
        took += usize::from(spinner.join().expect("the spinner ends"));
        prepared += usize::from(answer.is_ok());
        drop(answer);
    }
    let crossed = crossed.load(Ordering::SeqCst);
    println!("cp124 prepare-lock rounds={rounds} prepared={prepared} second_holder_took={took} copy_made_while_held={crossed}");
    assert_eq!(
        crossed, 0,
        "prepare made its copy while a second holder held the lock"
    );
}

/// The sentence of `prepare`'s documentation that states what its lock holds.
#[test]
fn the_documentation_of_prepare_states_what_its_lock_holds() {
    let source = include_str!("../src/lib.rs");
    let sentence = "It holds the lock of the workbook's directory, which it waits for up to the \
                    wait of the [`Locks`], from before its sweep until its prepared copy is \
                    made: over the sweep, the checkpoint, the copy's mark and the copy.";
    let text: String = source
        .lines()
        .map(|line| line.trim().trim_start_matches("///").trim())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        text.contains(sentence),
        "the sentence is gone from the documentation of prepare"
    );
}

/// E5 of CP-124, the verifier's M47 and case K2: when the carry fails (the folder's inherited
/// access entry denies writing an extended attribute), `prepare` removes the file that it made
/// and the folder is as it was.
#[cfg(target_os = "macos")]
#[test]
fn a_carry_that_fails_leaves_the_folder_as_it_was() {
    let scratch = Scratch::new("cp124-k2");
    let data = Scratch::new("cp124-k2-data");
    let workbook = scratch.path("values.xlsx");
    fs::copy(corpus("values.xlsx"), &workbook).expect("copy");
    let tool = |program: &str, args: &[&str], path: &Path| {
        Command::new(program)
            .args(args)
            .arg(path)
            .status()
            .expect("the tool runs")
            .success()
    };
    assert!(tool(
        "xattr",
        &["-w", "org.example.probe", "carried"],
        &workbook
    ));
    let before = scratch.names("");
    assert!(tool(
        "chmod",
        &["+a", "everyone deny writeextattr,file_inherit"],
        &scratch.dir
    ));
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    let answer = prepare_at(&folder, &workbook, RUN, "B5");
    assert!(tool("chmod", &["-N"], &scratch.dir));
    let cause = answer.as_ref().err().map(halvo_sheet::Error::cause);
    drop(answer);
    let after = scratch.names("");
    println!("cp124 k2 cause={cause:?} before={before:?} after={after:?}");
    assert_eq!(cause, Some(Cause::StepFailed));
    assert_eq!(before, after, "the folder after a carry that failed");
}

/// The answer of the coordinator to the windows run 43366645986: `prepare` reads the workbook
/// before it waits for the lock and opens it again under the lock. A workbook that another
/// program saved while `prepare` waited no longer holds the bytes that `prepare` read: it
/// refuses with `Error::Io` of the kind `Interrupted`, the cause `step_failed`, and writes
/// nothing.
#[test]
fn a_workbook_changed_while_prepare_waits_for_the_lock_is_refused_and_nothing_is_written() {
    let scratch = Scratch::new("cp124-changed-while-waiting");
    let data = Scratch::new("cp124-changed-while-waiting-data");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let locks = own_locks(&data.dir);
    let folder = Folder::new(&scratch.dir, locks.clone()).expect("the folder opens");
    let held = locks.hold(&scratch.dir).expect("the lock");
    let worker =
        thread::spawn(move || prepare_at(&folder, Path::new("values.xlsx"), RUN, "B5").map(drop));
    let waited = waiting_for_the_lock(&data.dir);
    println!("cp124 changed-while-waiting event={waited}");
    assert!(waited, "prepare did not wait for the lock within the bound");
    let mut saved = fs::read(corpus("values.xlsx")).expect("read");
    saved.extend_from_slice(b"saved again by another program");
    fs::write(scratch.path("values.xlsx"), &saved).expect("save the workbook");
    drop(held);
    let answer = worker.join().expect("the prepare thread");
    let cause = answer.as_ref().err().map(halvo_sheet::Error::cause);
    println!("cp124 changed-while-waiting answer={answer:?} cause={cause:?}");
    assert_eq!(
        answer,
        Err(halvo_sheet::Error::Io(std::io::ErrorKind::Interrupted))
    );
    assert_eq!(cause, Some(Cause::StepFailed));
    assert_eq!(scratch.names(""), vec!["values.xlsx".to_string()]);
    assert_eq!(fs::read(scratch.path("values.xlsx")).expect("read"), saved);
}

/// The fault of the windows run 43366645986 in a form that macOS shows: while `prepare` waits
/// for the lock, no descriptor of the process names the workbook (`lsof` of the process), so a
/// folder that the person moves while `prepare` waits is never held open. The folder moves,
/// `prepare` fails, and the moved folder holds the workbook alone.
#[cfg(target_os = "macos")]
#[test]
fn prepare_holds_no_descriptor_on_the_workbook_while_it_waits_for_the_lock() {
    let scratch = Scratch::new("cp124-no-handle-while-waiting");
    let data = Scratch::new("cp124-no-handle-while-waiting-data");
    fs::create_dir_all(scratch.path("folder/sub")).expect("mkdir");
    fs::copy(
        corpus("values.xlsx"),
        scratch.path("folder/sub/values.xlsx"),
    )
    .expect("copy");
    let locks = own_locks(&data.dir);
    let folder = Folder::new(&scratch.path("folder"), locks.clone()).expect("the folder opens");
    let held = locks.hold(&scratch.path("folder/sub")).expect("the lock");
    let worker = thread::spawn(move || {
        prepare_at(&folder, Path::new("sub/values.xlsx"), RUN, "B5").map(drop)
    });
    thread::sleep(Duration::from_millis(800));
    let listed = Command::new("lsof")
        .args(["-n", "-P", "-Fn", "-p", &std::process::id().to_string()])
        .output()
        .expect("lsof runs");
    let names = String::from_utf8_lossy(&listed.stdout);
    let open = names
        .lines()
        .filter(|line| line.starts_with('n') && line.ends_with("/sub/values.xlsx"))
        .count();
    let moved = fs::rename(scratch.path("folder/sub"), scratch.path("folder/sub-moved"));
    drop(held);
    let answer = worker.join().expect("the prepare thread");
    println!(
        "cp124 waiting descriptors_on_the_workbook={open} moved={} answer={answer:?}",
        moved.is_ok()
    );
    assert_eq!(
        open, 0,
        "a descriptor names the workbook while prepare waits"
    );
    assert!(moved.is_ok(), "the folder moves while prepare waits");
    assert!(answer.is_err(), "prepare of a workbook whose folder moved");
    assert_eq!(
        scratch.names("folder/sub-moved"),
        vec!["values.xlsx".to_string()]
    );
}

/// Waits until `prepare` waits for the lock of the folder, which it takes only after its first
/// read of the workbook. On macOS the event is the code's own: `lsof` of the process names the
/// lock file of the data directory twice, the test's holder and the `hold` of `prepare`; the
/// wait has a bound of 30 s. Windows has no such list of a process's handles at hand, so there
/// the test waits 800 ms, a named limit: a `prepare` that has not made its first read by then
/// reads the new bytes and the test fails, never passes, for that reason.
fn waiting_for_the_lock(data: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let data_name = data
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            let listed = Command::new("lsof")
                .args(["-n", "-P", "-Fn", "-p", &std::process::id().to_string()])
                .output()
                .expect("lsof runs");
            let names = String::from_utf8_lossy(&listed.stdout);
            let opens = names
                .lines()
                .filter(|line| {
                    line.starts_with('n') && line.ends_with(".lock") && line.contains(&data_name)
                })
                .count();
            if opens >= 2 {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = data;
        thread::sleep(Duration::from_millis(800));
        true
    }
}
