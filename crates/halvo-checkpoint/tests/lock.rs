mod common;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{folds_case, link_dir, link_file, Scratch};
use halvo_checkpoint::{LockError, Locks};

const SHORT: Duration = Duration::from_millis(100);
const CHILD_DATA: &str = "HALVO_CHECKPOINT_CHILD_DATA";
const CHILD_DIR: &str = "HALVO_CHECKPOINT_CHILD_DIR";

fn locks(scratch: &Scratch, wait: Duration) -> Locks {
    std::fs::create_dir_all(scratch.path("data")).expect("make the data directory");
    Locks::open(&scratch.path("data"), wait).expect("the locks open")
}

#[test]
fn a_second_holder_waits_for_the_wait_and_is_refused_as_busy() {
    let scratch = Scratch::new("lock-busy");
    std::fs::create_dir_all(scratch.path("folder")).expect("mkdir");
    let locks = locks(&scratch, Duration::from_millis(300));
    let held = locks
        .hold(&scratch.path("folder"))
        .expect("the first holder");
    let started = Instant::now();
    let second = locks.hold(&scratch.path("folder"));
    assert_eq!(second.err(), Some(LockError::Busy));
    assert!(started.elapsed() >= Duration::from_millis(300), "it waited");
    drop(held);
    assert!(
        locks.hold(&scratch.path("folder")).is_ok(),
        "free once dropped"
    );
}

#[test]
fn a_holder_that_lets_go_within_the_wait_hands_the_lock_on() {
    let scratch = Scratch::new("lock-wait");
    std::fs::create_dir_all(scratch.path("folder")).expect("mkdir");
    let locks = locks(&scratch, Duration::from_secs(10));
    let held = locks
        .hold(&scratch.path("folder"))
        .expect("the first holder");
    let other = locks.clone();
    let dir = scratch.path("folder");
    let waiter = std::thread::spawn(move || other.hold(&dir).map(drop));
    std::thread::sleep(Duration::from_millis(200));
    drop(held);
    assert_eq!(waiter.join().expect("the waiter"), Ok(()));
}

#[test]
fn every_spelling_of_one_directory_takes_one_lock_and_two_directories_take_two() {
    let scratch = Scratch::new("lock-spellings");
    std::fs::create_dir_all(scratch.path("folder/sub")).expect("mkdir");
    std::fs::create_dir_all(scratch.path("other")).expect("mkdir");
    link_dir(&scratch.path("folder"), &scratch.path("alias"));
    let locks = locks(&scratch, SHORT);
    let held = locks.hold(&scratch.path("folder")).expect("held");
    let mut spellings: Vec<PathBuf> = vec![
        scratch.path("alias"),
        scratch.path("folder/sub/.."),
        scratch.path("folder/."),
    ];
    if folds_case(&scratch.dir) {
        spellings.push(scratch.path("FOLDER"));
    }
    for spelling in &spellings {
        assert_eq!(
            locks.hold(spelling).err(),
            Some(LockError::Busy),
            "{spelling:?}"
        );
    }
    assert!(locks.hold(&scratch.path("other")).is_ok());
    assert!(locks.hold(&scratch.path("folder/sub")).is_ok());
    drop(held);
}

#[test]
fn the_lock_file_is_named_by_a_digest_lies_in_agents_locks_and_holds_nothing() {
    let scratch = Scratch::new("lock-file");
    std::fs::create_dir_all(scratch.path("folder")).expect("mkdir");
    let locks = locks(&scratch, SHORT);
    let held = locks.hold(&scratch.path("folder")).expect("held");
    let names = scratch.names("data/agents/locks");
    assert_eq!(names.len(), 1, "{names:?}");
    let (hex, ext) = names[0].split_at(64);
    assert!(hex
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert_eq!(ext, ".lock");
    let meta = std::fs::metadata(scratch.path("data/agents/locks").join(&names[0])).expect("stat");
    assert_eq!(meta.len(), 0);
    assert_eq!(scratch.names("data"), vec!["agents".to_string()]);
    assert_eq!(scratch.names("data/agents"), vec!["locks".to_string()]);
    drop(held);
    let again = Locks::open(&scratch.path("data"), SHORT).expect("opens twice");
    assert!(again.hold(&scratch.path("folder")).is_ok());
}

#[test]
fn a_link_in_the_place_of_agents_of_locks_or_of_the_lock_file_is_refused() {
    let scratch = Scratch::new("lock-links");
    std::fs::create_dir_all(scratch.path("folder")).expect("mkdir");
    std::fs::create_dir_all(scratch.path("elsewhere")).expect("mkdir");
    std::fs::create_dir_all(scratch.path("data1")).expect("mkdir");
    link_dir(&scratch.path("elsewhere"), &scratch.path("data1/agents"));
    let got = Locks::open(&scratch.path("data1"), SHORT).err();
    assert_eq!(got, Some(LockError::Link));
    std::fs::create_dir_all(scratch.path("data2/agents")).expect("mkdir");
    link_dir(
        &scratch.path("elsewhere"),
        &scratch.path("data2/agents/locks"),
    );
    let got = Locks::open(&scratch.path("data2"), SHORT).err();
    assert_eq!(got, Some(LockError::Link));
    assert!(scratch.names("elsewhere").is_empty());
    let locks = locks(&scratch, SHORT);
    let held = locks.hold(&scratch.path("folder")).expect("held");
    let name = scratch.names("data/agents/locks").remove(0);
    drop(held);
    let lock = scratch.path("data/agents/locks").join(&name);
    std::fs::remove_file(&lock).expect("remove the lock file");
    let person = scratch.write("person.xlsx", b"the person's file");
    link_file(&person, &lock);
    assert_eq!(
        locks.hold(&scratch.path("folder")).err(),
        Some(LockError::Link)
    );
    std::fs::remove_file(&lock).expect("remove the link");
    link_file(&scratch.path("absent.xlsx"), &lock);
    assert_eq!(
        locks.hold(&scratch.path("folder")).err(),
        Some(LockError::Link)
    );
    assert!(
        !scratch.path("absent.xlsx").exists(),
        "no file made through a link"
    );
    assert_eq!(std::fs::read(&person).expect("read"), b"the person's file");
}

#[test]
fn a_data_directory_that_is_no_directory_and_a_workbook_directory_that_is_absent_fail() {
    let scratch = Scratch::new("lock-io");
    let file = scratch.write("data", b"x");
    let got = Locks::open(&file, SHORT).err();
    assert_eq!(got, Some(LockError::Io(std::io::ErrorKind::NotADirectory)));
    let absent = Locks::open(&scratch.path("gone"), SHORT).err();
    assert_eq!(absent, Some(LockError::Io(std::io::ErrorKind::NotFound)));
    let locks = locks(&Scratch::new("lock-io-data"), SHORT);
    let got = locks.hold(&scratch.path("no-such-dir")).err();
    assert_eq!(got, Some(LockError::Io(std::io::ErrorKind::NotFound)));
    let got = locks.hold(&file).err();
    assert_eq!(got, Some(LockError::Io(std::io::ErrorKind::NotADirectory)));
}

#[test]
fn the_debug_form_of_the_locks_names_no_path() {
    let scratch = Scratch::new("lock-debug");
    let locks = locks(&scratch, SHORT);
    let shown = format!("{locks:?}");
    assert!(
        !shown.contains("lock-debug") && !shown.contains('/'),
        "{shown}"
    );
}

#[test]
fn lock_holder_child() {
    let (Ok(data), Ok(dir)) = (std::env::var(CHILD_DATA), std::env::var(CHILD_DIR)) else {
        return;
    };
    let locks = Locks::open(Path::new(&data), SHORT).expect("the locks open");
    let _held = locks
        .hold(Path::new(&dir))
        .expect("the child holds the lock");
    println!("child holds the lock");
    std::thread::sleep(Duration::from_secs(120));
}

#[test]
fn a_killed_holder_releases_the_lock() {
    let scratch = Scratch::new("lock-killed");
    std::fs::create_dir_all(scratch.path("folder")).expect("mkdir");
    let locks = locks(&scratch, SHORT);
    let mut child = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "lock_holder_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_DATA, scratch.path("data"))
        .env(CHILD_DIR, scratch.path("folder"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the child");
    let stdout = child.stdout.take().expect("the child's output");
    let holds = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.contains("child holds the lock"));
    assert!(holds, "the child said that it holds the lock");
    assert_eq!(
        locks.hold(&scratch.path("folder")).err(),
        Some(LockError::Busy)
    );
    child.kill().expect("kill the child");
    child.wait().expect("the child ends");
    let free = Locks::open(&scratch.path("data"), Duration::from_secs(5)).expect("open");
    assert!(
        free.hold(&scratch.path("folder")).is_ok(),
        "free once killed"
    );
}
