#![cfg(debug_assertions)]

use halvo_secrets::{
    Delete, DeleteIfHolds, Directory, Name, Read, Secret, Store, StoreKind, Write,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const HELD: &[u8] = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn scratch(label: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(base).expect("the target's temporary directory");
    loop {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = base.join(format!(
            "halvo-secrets-ops-{}-{label}-{n}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path.join("store"),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("a scratch directory: {error}"),
        }
    }
}

fn store_in(dir: &Path) -> Store {
    Store::in_directory(
        Directory::new(dir.to_path_buf()).expect("an absolute directory"),
        Name::new("account").expect("a name"),
    )
}

fn secret(bytes: &[u8]) -> Secret {
    Secret::new(bytes.to_vec()).expect("a secret")
}

fn found(read: Read) -> Vec<u8> {
    match read {
        Read::Found(secret) => secret.expose().to_vec(),
        other => panic!("read answered {other:?}"),
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("the store directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn one_byte_off(at: usize) -> Vec<u8> {
    let mut other = HELD.to_vec();
    other[at] ^= 1;
    other
}

fn kind_of_this_store() -> StoreKind {
    if cfg!(windows) {
        StoreKind::Dpapi
    } else {
        StoreKind::File
    }
}

#[test]
fn a_delete_if_holds_removes_only_the_bytes_that_its_caller_read() {
    let dir = scratch("holds");
    let store = store_in(&dir);
    assert!(matches!(
        store.delete_if_holds(&secret(HELD)),
        DeleteIfHolds::NotStored
    ));
    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    for at in [0, HELD.len() / 2, HELD.len() - 1] {
        match store.delete_if_holds(&secret(&one_byte_off(at))) {
            DeleteIfHolds::Other(kept) => assert_eq!(kept.expose(), HELD),
            answer => panic!("the delete answered {answer:?}"),
        }
        assert_eq!(found(store.read()), HELD);
        assert_eq!(names_in(&dir), ["account"]);
    }
    assert!(matches!(
        store.delete_if_holds(&secret(HELD)),
        DeleteIfHolds::Deleted
    ));
    assert!(names_in(&dir).is_empty(), "{:?}", names_in(&dir));
    assert!(matches!(store.read(), Read::NotStored));
}

fn left_claim(dir: &Path) {
    fs::rename(
        dir.join("account"),
        dir.join(format!(".account.{}.0.claim", std::process::id() + 1)),
    )
    .expect("the claim of a stopped delete");
}

#[test]
fn a_claim_that_a_stopped_delete_left_is_put_back_by_the_next_operation() {
    for op in ["read", "write", "delete", "delete-if-holds"] {
        let dir = scratch("left-claim");
        let store = store_in(&dir);
        assert_eq!(store.write(&secret(HELD)), Write::Stored);
        left_claim(&dir);
        let answer = match op {
            "read" => format!("{:?}", store.read()),
            "write" => format!("{:?}", store.write(&secret(b"a new sign-in"))),
            "delete" => format!("{:?}", store.delete()),
            _ => format!("{:?}", store.delete_if_holds(&secret(&one_byte_off(1)))),
        };
        let want = match op {
            "read" => "Found(..)",
            "write" => "Occupied",
            "delete" => "Deleted",
            _ => "Other(..)",
        };
        assert_eq!(answer, want, "{op}");
        let names = names_in(&dir);
        assert!(
            !names.iter().any(|name| name.ends_with(".claim")),
            "{op}: {names:?}"
        );
        if matches!(op, "read" | "write" | "delete-if-holds") {
            assert_eq!(found(store.read()), HELD, "{op}");
        } else {
            assert!(names.is_empty(), "{op}: {names:?}");
        }
    }
}

#[test]
fn a_removal_names_the_store_of_this_place_alone_and_then_reads() {
    let dir = scratch("removal");
    let store = store_in(&dir);
    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    for kind in [StoreKind::Keychain, StoreKind::File, StoreKind::Dpapi] {
        if kind != kind_of_this_store() {
            assert!(store.remove(kind).is_none(), "{kind:?}");
            assert_eq!(found(store.read()), HELD, "{kind:?}");
        }
    }
    let removal = store
        .remove(kind_of_this_store())
        .expect("the store of this place");
    assert_eq!(removal.removed, Delete::Deleted);
    assert!(
        matches!(removal.then, Read::NotStored),
        "{:?}",
        removal.then
    );
    assert!(names_in(&dir).is_empty(), "{:?}", names_in(&dir));
    let removal = store
        .remove(kind_of_this_store())
        .expect("the store of this place");
    assert_eq!(removal.removed, Delete::NotStored);
    assert!(matches!(removal.then, Read::NotStored));

    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    left_claim(&dir);
    let removal = store
        .remove(kind_of_this_store())
        .expect("the store of this place");
    assert_eq!(removal.removed, Delete::Deleted);
    assert!(matches!(removal.then, Read::NotStored));
    assert!(names_in(&dir).is_empty(), "{:?}", names_in(&dir));
}

fn renamed_into(dir: &Path, name: &str) {
    fs::rename(dir.join("account"), dir.join(name)).expect("the file under the left name");
}

#[test]
fn a_removal_at_the_person_s_word_ends_a_claim_that_cannot_be_named_back() {
    let dir = scratch("stranded-claim");
    let store = store_in(&dir);
    let aside = scratch("stranded-claim-aside");
    assert_eq!(store_in(&aside).write(&secret(HELD)), Write::Stored);
    assert_eq!(store.write(&secret(&one_byte_off(2))), Write::Stored);
    fs::copy(
        aside.join("account"),
        dir.join(format!(".account.{}.0.claim", std::process::id() + 3)),
    )
    .expect("a claim beside a later sign-in");
    let read = store.read();
    assert!(
        matches!(read, Read::Fault(halvo_secrets::Fault { code: -1 })),
        "{read:?}"
    );
    let removal = store
        .remove(kind_of_this_store())
        .expect("the store of this place");
    assert_eq!(removal.removed, Delete::Deleted);
    assert!(
        matches!(removal.then, Read::NotStored),
        "{:?}",
        removal.then
    );
    assert!(names_in(&dir).is_empty(), "{:?}", names_in(&dir));
}

#[test]
fn a_removal_at_the_person_s_word_leaves_no_temporary_file_of_a_killed_write() {
    let dir = scratch("killed-write");
    let store = store_in(&dir);
    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    renamed_into(&dir, &format!(".account.{}.0.tmp", std::process::id() + 5));
    assert!(matches!(store.read(), Read::NotStored));
    let removal = store
        .remove(kind_of_this_store())
        .expect("the store of this place");
    assert_eq!(removal.removed, Delete::NotStored);
    assert!(
        matches!(removal.then, Read::NotStored),
        "{:?}",
        removal.then
    );
    assert!(names_in(&dir).is_empty(), "{:?}", names_in(&dir));
}

/// G4 (a): the removal at the person's word removes a temporary file only of the exact form
/// `.<name>.<digits>.<digits>.tmp`, and keeps every near form and every other file.
#[test]
fn a_removal_at_the_person_s_word_removes_only_temporary_files_of_the_exact_form() {
    let dir = scratch("near-forms");
    let store = store_in(&dir);
    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    let mut near = [
        ".notes.tmp",
        ".other.1.0.tmp",
        ".account.1.tmp",
        "account.1.0.tmp",
        ".account.1.0.tmpx",
        ".account.1.0.0.tmp",
        ".account.x.3.tmp",
        ".account..3.tmp",
        ".account.3.x.tmp",
        ".account.3..tmp",
        ".account2.1.1.tmp",
        "notes",
        ".account.1.0.claimx",
    ];
    for name in near {
        fs::write(dir.join(name), b"not a file of the store").expect("a near form");
    }
    fs::write(dir.join(".account.1.0.tmp"), b"a killed write").expect("a temporary file");
    let removal = store
        .remove(kind_of_this_store())
        .expect("the store of this place");
    assert_eq!(removal.removed, Delete::Deleted);
    near.sort();
    assert_eq!(names_in(&dir), near);
}

const RACE_ROLE: &str = "HALVO_SECRETS_RACE_ROLE";
const RACE_DIR: &str = "HALVO_SECRETS_RACE_DIR";
const RACE_LIST: &str = "HALVO_SECRETS_RACE_LIST";
const RACE_ROUNDS: usize = 200;

fn shown_read(read: Read) -> String {
    match read {
        Read::Found(held) if held.expose() == HELD => "Found(X)".to_string(),
        Read::Found(_) => "Found(other)".to_string(),
        Read::NotStored => "NotStored".to_string(),
        Read::Locked(locked) => format!("Locked({locked:?})"),
        Read::Fault(fault) => format!("Fault({})", fault.code),
    }
}

fn shown_holds(answer: DeleteIfHolds) -> String {
    match answer {
        DeleteIfHolds::Deleted => "Deleted".to_string(),
        DeleteIfHolds::NotStored => "NotStored".to_string(),
        DeleteIfHolds::Other(held) if held.expose() == HELD => "Other(X)".to_string(),
        DeleteIfHolds::Other(_) => "Other(other)".to_string(),
        DeleteIfHolds::Locked(locked) => format!("Locked({locked:?})"),
        DeleteIfHolds::Fault(fault) => format!("Fault({})", fault.code),
    }
}

fn shown_write(write: Write) -> String {
    match write {
        Write::Fault(fault) => format!("Fault({})", fault.code),
        write => format!("{write:?}"),
    }
}

fn shown_delete(delete: Option<Delete>) -> String {
    match delete {
        Some(Delete::Fault(fault)) => format!("Fault({})", fault.code),
        Some(delete) => format!("{delete:?}"),
        None => "None".to_string(),
    }
}

#[test]
fn child_of_the_two_process_races() {
    let Ok(role) = std::env::var(RACE_ROLE) else {
        return;
    };
    let dir = PathBuf::from(std::env::var(RACE_DIR).expect("the race's directory"));
    let store = store_in(&dir);
    for round in 0..RACE_ROUNDS {
        let answer = match role.as_str() {
            "delete-other" => shown_holds(store.delete_if_holds(&secret(&one_byte_off(7)))),
            "delete-x" => shown_holds(store.delete_if_holds(&secret(HELD))),
            "read" => shown_read(store.read()),
            "write-y" => shown_write(store.write(&secret(&one_byte_off(9)))),
            "remove" => shown_delete(store.remove(kind_of_this_store()).map(|r| r.removed)),
            role => panic!("no role {role}"),
        };
        println!("RACE {role} {round} {answer}");
        if std::env::var_os(RACE_LIST).is_some() {
            let claims: Vec<String> = names_in(&dir)
                .into_iter()
                .filter(|name| name.starts_with(".account.") && name.ends_with(".claim"))
                .collect();
            if !claims.is_empty() {
                println!("CLAIMSEEN {role} {round} {claims:?}");
            }
        }
    }
}

fn child(role: &str, dir: &Path) -> std::process::Child {
    child_with(role, dir, false)
}

/// A child that also lists the store directory after each round and prints each name of
/// the claim's form that it finds.
fn child_listing(role: &str, dir: &Path) -> std::process::Child {
    child_with(role, dir, true)
}

fn child_with(role: &str, dir: &Path, listing: bool) -> std::process::Child {
    let mut command = std::process::Command::new(std::env::current_exe().expect("the test binary"));
    if listing {
        command.env(RACE_LIST, "1");
    }
    command
        .args([
            "--exact",
            "child_of_the_two_process_races",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(RACE_ROLE, role)
        .env(RACE_DIR, dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("a child of the race")
}

#[derive(Default)]
struct Tally {
    counts: std::collections::BTreeMap<String, usize>,
    wrong: Vec<String>,
    windows_forms: Vec<String>,
}

impl Tally {
    fn take(&mut self, race: &str, role: &str, round: &str, answer: &str, allowed: &[&str]) {
        *self
            .counts
            .entry(format!("{race} {role} {answer}"))
            .or_insert(0) += 1;
        if allowed.contains(&answer) {
            return;
        }
        let line = format!("{race} {role} round {round}: {answer}");
        let windows_form = cfg!(windows)
            && matches!(
                answer,
                "Fault(2)" | "Fault(3)" | "Fault(32)" | "Locked(Refused { code: 5 })"
            );
        if windows_form {
            self.windows_forms.push(line);
        } else {
            self.wrong.push(line);
        }
    }

    fn child(&mut self, race: &str, child: std::process::Child, allowed: &[&str]) {
        let output = child.wait_with_output().expect("the child's output");
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        for line in text
            .lines()
            .filter_map(|line| line.find("CLAIMSEEN ").map(|at| &line[at..]))
        {
            self.wrong.push(format!("{race}: {line}"));
        }
        let mut rounds = 0;
        for line in text
            .lines()
            .filter_map(|line| line.find("RACE ").map(|at| &line[at + 5..]))
        {
            let mut words = line.splitn(3, ' ');
            let (role, round, answer) = (
                words.next().unwrap_or(""),
                words.next().unwrap_or(""),
                words.next().unwrap_or(""),
            );
            rounds += 1;
            self.take(race, role, round, answer, allowed);
        }
        if !output.status.success() || rounds != RACE_ROUNDS {
            self.wrong.push(format!(
                "{race}: a child ended with {} after {rounds} rounds: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
}

const FAULT_OF_CONTENTION: &str = "Fault(-1)";

fn parent_answers(race: &str) -> &'static [&'static str] {
    if race == "ii-remove" {
        &[
            "Stored",
            "Occupied",
            FAULT_OF_CONTENTION,
            WRITE_BESIDE_A_REMOVAL,
        ]
    } else {
        &["Stored", "Occupied", FAULT_OF_CONTENTION]
    }
}

const WRITE_BESIDE_A_REMOVAL: &str = "Fault(2)";

#[test]
fn two_processes_on_one_store_answer_what_stands_or_a_fault_of_contention() {
    let mut tally = Tally::default();
    let started = std::time::Instant::now();

    let dir = scratch("race-x-stands");
    let store = store_in(&dir);
    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    let (deleting, reading) = (child("delete-other", &dir), child("read", &dir));
    tally.child("i", deleting, &["Other(X)", FAULT_OF_CONTENTION]);
    tally.child("i", reading, &["Found(X)", FAULT_OF_CONTENTION]);
    assert_eq!(found(store.read()), HELD, "X stands after race i");
    let (writing, reading) = (child("write-y", &dir), child("read", &dir));
    tally.child("i-write", writing, &["Occupied", FAULT_OF_CONTENTION]);
    tally.child("i-write", reading, &["Found(X)", FAULT_OF_CONTENTION]);
    assert_eq!(found(store.read()), HELD, "X stands after the writes");

    write_beside_a_delete_of_other_bytes(&mut tally, "iii");

    for (race, second) in [("ii-remove", "remove"), ("ii-delete", "delete-x")] {
        let dir = scratch(race);
        let store = store_in(&dir);
        assert_eq!(store.write(&secret(HELD)), Write::Stored);
        let mut first = child("delete-x", &dir);
        let mut other = child(second, &dir);
        loop {
            let done = first.try_wait().expect("a child").is_some()
                && other.try_wait().expect("a child").is_some();
            let answer = shown_write(store.write(&secret(HELD)));
            tally.take(race, "parent-write", "-", &answer, parent_answers(race));
            if done {
                break;
            }
        }
        tally.child(race, first, &["Deleted", "NotStored", FAULT_OF_CONTENTION]);
        tally.child(race, other, &["Deleted", "NotStored", FAULT_OF_CONTENTION]);
        match store.read() {
            Read::Found(held) => assert_eq!(held.expose(), HELD, "{race}: only X is ever written"),
            Read::NotStored => {}
            read => panic!("{race}: the store then read {read:?}"),
        }
    }

    for (key, count) in &tally.counts {
        println!("race answers {key}: {count}");
    }
    for line in &tally.windows_forms {
        println!("race windows form {line}");
    }
    println!(
        "race rounds {} each, seconds {}, windows forms {}",
        RACE_ROUNDS,
        started.elapsed().as_secs(),
        tally.windows_forms.len()
    );
    assert!(
        tally.wrong.is_empty(),
        "answers outside the rule: {:#?}",
        tally.wrong
    );
}

/// The pair of G1: one child loops `delete_if_holds` of other bytes than X and one loops a
/// write of Y, on a store that holds X. Every delete answers `Other(X)` and every write
/// `Occupied`, no child sees a name of the claim's form after a round, and X stands.
fn write_beside_a_delete_of_other_bytes(tally: &mut Tally, race: &str) {
    let dir = scratch("write-beside-other-delete");
    let store = store_in(&dir);
    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    let (deleting, writing) = (
        child_listing("delete-other", &dir),
        child_listing("write-y", &dir),
    );
    tally.child(race, deleting, &["Other(X)"]);
    tally.child(race, writing, &["Occupied"]);
    let names = names_in(&dir);
    if names != ["account"] {
        tally
            .wrong
            .push(format!("{race}: the store directory then held {names:?}"));
    }
    if fs::read(dir.join("account")).ok().as_deref() != Some(HELD) {
        tally
            .wrong
            .push(format!("{race}: X did not stand byte for byte"));
    }
    if tally.counts.contains_key(&format!("{race} write-y Stored")) {
        tally.wrong.push(format!("{race}: a write stored beside X"));
    }
}

#[test]
fn a_write_beside_a_delete_if_holds_of_other_bytes_stores_nothing_and_leaves_no_claim() {
    let started = std::time::Instant::now();
    let mut tally = Tally::default();
    write_beside_a_delete_of_other_bytes(&mut tally, "write-beside-other-delete");
    for (key, count) in &tally.counts {
        println!("pair answers {key}: {count}");
    }
    for line in &tally.windows_forms {
        println!("pair windows form {line}");
    }
    println!(
        "pair rounds {} each, seconds {}, windows forms {}",
        RACE_ROUNDS,
        started.elapsed().as_secs(),
        tally.windows_forms.len()
    );
    assert!(
        tally.wrong.is_empty(),
        "answers outside the rule: {:#?}",
        tally.wrong
    );
}
