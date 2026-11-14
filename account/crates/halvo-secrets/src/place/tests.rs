use super::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

const USER: u32 = 501;
const OTHER: u32 = 502;
const ENOENT: i32 = 2;
#[cfg(unix)]
const EIO: i32 = 5;
#[cfg(windows)]
const EIO: i32 = 1117;
#[cfg(unix)]
const EACCES: i32 = 13;
#[cfg(windows)]
const EACCES: i32 = 5;
#[cfg(unix)]
const EPERM: i32 = 1;
const NO_CODE: i32 = 0;
/// A failure of the kind NotFound that carries no code of the system, as the standard
/// library folds codes into that kind (on Windows also a drive or a share that is away).
const FOLDED: i32 = -2;
#[cfg(unix)]
const REFUSALS: [i32; 2] = [EACCES, EPERM];
#[cfg(windows)]
const REFUSALS: [i32; 1] = [EACCES];
#[cfg(unix)]
const CODES: [i32; 5] = [ENOENT, EIO, EACCES, EPERM, NO_CODE];
#[cfg(windows)]
const CODES: [i32; 4] = [ENOENT, EIO, EACCES, NO_CODE];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    kind: EntryKind,
    owner: u32,
    mode: u32,
    bytes: Vec<u8>,
    shown_len: Option<u64>,
    acl: bool,
    acl_write: bool,
    links: u64,
}

impl Entry {
    fn dir(mode: u32) -> Entry {
        Entry {
            kind: EntryKind::Directory,
            owner: USER,
            mode,
            bytes: Vec::new(),
            shown_len: None,
            acl: false,
            acl_write: false,
            links: 1,
        }
    }

    fn file(mode: u32, bytes: &[u8]) -> Entry {
        Entry {
            kind: EntryKind::File,
            owner: USER,
            mode,
            bytes: bytes.to_vec(),
            shown_len: None,
            acl: false,
            acl_write: false,
            links: 1,
        }
    }

    fn of(kind: EntryKind) -> Entry {
        Entry {
            kind,
            ..Entry::file(0o600, b"x")
        }
    }

    fn owned_by(self, owner: u32) -> Entry {
        Entry { owner, ..self }
    }

    fn with_acl(self) -> Entry {
        Entry { acl: true, ..self }
    }

    fn with_acl_write(self) -> Entry {
        Entry {
            acl: true,
            acl_write: true,
            ..self
        }
    }

    fn with_links(self, links: u64) -> Entry {
        Entry { links, ..self }
    }

    fn shown(self, len: u64) -> Entry {
        Entry {
            shown_len: Some(len),
            ..self
        }
    }

    fn meta(&self) -> Meta {
        Meta {
            kind: self.kind,
            owner: self.owner,
            mode: self.mode,
            len: self.shown_len.unwrap_or(self.bytes.len() as u64),
            acl: self.acl,
            acl_write: self.acl_write,
            links: self.links,
        }
    }
}

fn shown(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

struct Memory {
    entries: RefCell<BTreeMap<PathBuf, Entry>>,
    ancestors: RefCell<Vec<Entry>>,
    log: RefCell<Vec<String>>,
    fail: RefCell<Vec<(&'static str, usize, i32)>>,
    calls: RefCell<BTreeMap<&'static str, usize>>,
    makes_no_dir: Cell<bool>,
    parent_missing: Cell<bool>,
    on_create_temp: RefCell<Option<(PathBuf, Entry)>>,
    chunk: Cell<usize>,
}

struct Handle {
    path: PathBuf,
    offset: usize,
}

impl Memory {
    fn new() -> Memory {
        Memory {
            entries: RefCell::new(BTreeMap::new()),
            ancestors: RefCell::new(vec![Entry::dir(0o755).owned_by(0)]),
            log: RefCell::new(Vec::new()),
            fail: RefCell::new(Vec::new()),
            calls: RefCell::new(BTreeMap::new()),
            makes_no_dir: Cell::new(false),
            parent_missing: Cell::new(false),
            on_create_temp: RefCell::new(None),
            chunk: Cell::new(7),
        }
    }

    fn with(entries: &[(&str, Entry)]) -> Memory {
        let memory = Memory::new();
        for (path, entry) in entries {
            memory.put(path, entry.clone());
        }
        memory
    }

    fn put(&self, path: &str, entry: Entry) {
        self.entries.borrow_mut().insert(PathBuf::from(path), entry);
    }

    fn failing(self, step: &'static str, code: i32) -> Memory {
        self.failing_call(step, 0, code)
    }

    fn failing_call(self, step: &'static str, call: usize, code: i32) -> Memory {
        self.fail.borrow_mut().push((step, call, code));
        self
    }

    fn step(&self, name: &'static str, path: &Path) -> io::Result<()> {
        self.log
            .borrow_mut()
            .push(format!("{name} {}", shown(path)));
        let call = {
            let mut calls = self.calls.borrow_mut();
            let count = calls.entry(name).or_insert(0);
            *count += 1;
            *count
        };
        let failing = self
            .fail
            .borrow()
            .iter()
            .find(|(step, nth, _)| *step == name && (*nth == 0 || *nth == call))
            .map(|(_, _, code)| *code);
        match failing {
            Some(NO_CODE) => Err(io::Error::other("a failure with no code")),
            Some(FOLDED) => Err(io::Error::from(io::ErrorKind::NotFound)),
            Some(code) => Err(io::Error::from_raw_os_error(code)),
            None => Ok(()),
        }
    }

    fn entry(&self, path: &str) -> Option<Entry> {
        self.entries.borrow().get(Path::new(path)).cloned()
    }

    fn paths(&self) -> Vec<String> {
        self.entries
            .borrow()
            .keys()
            .map(|path| shown(path))
            .collect()
    }

    fn steps(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl Fs for Memory {
    type Dir = PathBuf;
    type Item = Handle;
    type Temp = Handle;

    fn user(&self) -> u32 {
        USER
    }

    fn walk(&self, path: &Path) -> io::Result<Walk<PathBuf>> {
        self.step("walk", path)?;
        let ancestors = self.ancestors.borrow().iter().map(Entry::meta).collect();
        if self.parent_missing.get() {
            return Ok(Walk {
                ancestors,
                last: Last::Missing(ENOENT),
            });
        }
        let last = match self.entries.borrow().get(path) {
            None => Last::Absent(path.parent().expect("a parent").to_path_buf()),
            Some(entry) if entry.kind == EntryKind::Directory => {
                Last::Dir(entry.meta(), path.to_path_buf())
            }
            Some(_) => Last::NotADirectory,
        };
        Ok(Walk { ancestors, last })
    }

    fn make_dir(&self, parent: &PathBuf, name: &OsStr) -> io::Result<()> {
        let path = parent.join(name);
        self.step("make_dir", &path)?;
        if self.makes_no_dir.get() {
            return Ok(());
        }
        self.entries
            .borrow_mut()
            .entry(path)
            .or_insert_with(|| Entry::dir(0o700));
        Ok(())
    }

    fn is_absent(&self, error: &io::Error) -> bool {
        error.raw_os_error() == Some(ENOENT)
    }

    fn exists(&self, dir: &PathBuf, name: &str) -> io::Result<bool> {
        let path = dir.join(name);
        self.step("exists", &path)?;
        Ok(self.entries.borrow().contains_key(&path))
    }

    fn open_item(&self, dir: &PathBuf, name: &str) -> io::Result<Opened<Handle>> {
        let path = dir.join(name);
        self.step("open_item", &path)?;
        Ok(match self.entries.borrow().get(&path) {
            None => Opened::Absent,
            Some(entry) if entry.kind == EntryKind::Link => Opened::NotAFile,
            Some(_) => Opened::Item(Handle { path, offset: 0 }),
        })
    }

    fn item_meta(&self, item: &Handle) -> io::Result<Meta> {
        self.step("item_meta", &item.path)?;
        self.entries
            .borrow()
            .get(&item.path)
            .map(Entry::meta)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn read_item(&self, item: &mut Handle, buf: &mut [u8]) -> io::Result<usize> {
        self.step("read_item", &item.path)?;
        let entries = self.entries.borrow();
        let bytes = &entries[&item.path].bytes;
        let rest = &bytes[item.offset.min(bytes.len())..];
        let n = rest.len().min(buf.len()).min(self.chunk.get());
        buf[..n].copy_from_slice(&rest[..n]);
        item.offset += n;
        Ok(n)
    }

    fn create_temp(&self, dir: &PathBuf, name: &str) -> io::Result<Option<Handle>> {
        let path = dir.join(name);
        self.step("create_temp", &path)?;
        if let Some((racer, entry)) = self.on_create_temp.borrow_mut().take() {
            self.entries.borrow_mut().insert(racer, entry);
        }
        let mut entries = self.entries.borrow_mut();
        if entries.contains_key(&path) {
            return Ok(None);
        }
        entries.insert(path.clone(), Entry::file(0o600, b""));
        Ok(Some(Handle { path, offset: 0 }))
    }

    fn write_temp(&self, temp: &mut Handle, bytes: &[u8]) -> io::Result<()> {
        self.step("write_temp", &temp.path)?;
        self.entries
            .borrow_mut()
            .get_mut(&temp.path)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?
            .bytes
            .extend_from_slice(bytes);
        Ok(())
    }

    fn sync_temp(&self, temp: &Handle) -> io::Result<()> {
        self.step("sync_temp", &temp.path)
    }

    fn publish(&self, dir: &PathBuf, from: &str, to: &str) -> io::Result<Published> {
        let (from, to) = (dir.join(from), dir.join(to));
        self.step("publish", &to)?;
        let mut entries = self.entries.borrow_mut();
        if entries.contains_key(&to) {
            return Ok(Published::Exists);
        }
        let entry = entries
            .remove(&from)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        entries.insert(to, entry);
        Ok(Published::Done)
    }

    fn remove(&self, dir: &PathBuf, name: &str) -> io::Result<()> {
        let path = dir.join(name);
        self.step("remove", &path)?;
        self.entries
            .borrow_mut()
            .remove(&path)
            .map(|_| ())
            .ok_or_else(|| io::Error::from_raw_os_error(ENOENT))
    }

    fn sync_dir(&self, dir: &PathBuf) -> io::Result<()> {
        self.step("sync_dir", dir)
    }

    fn names(&self, dir: &PathBuf) -> io::Result<Vec<String>> {
        self.step("names", dir)?;
        Ok(self
            .entries
            .borrow()
            .keys()
            .filter(|path| path.parent() == Some(dir.as_path()))
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect())
    }
}

struct Reversed;

impl Codec for Reversed {
    fn seal<'a>(&self, secret: &'a Secret) -> Result<Sealed<'a>, Fault> {
        if secret.expose().starts_with(b"unsealable") {
            return Err(Fault { code: 77 });
        }
        Ok(Sealed::Blob(
            secret.expose().iter().rev().copied().collect(),
        ))
    }

    fn open(&self, bytes: Vec<u8>) -> Result<Secret, Locked> {
        if bytes.starts_with(b"!") {
            return Err(Locked::Refused { code: 99 });
        }
        if bytes.is_empty() {
            return Err(Locked::Refused { code: 97 });
        }
        if bytes.len() > self.limit() {
            return Err(Locked::Refused { code: 98 });
        }
        Secret::new(bytes.into_iter().rev().collect()).ok_or(Locked::Size)
    }

    fn limit(&self) -> usize {
        MAX_SECRET_LEN + 16
    }
}

const DIR: &str = "/store";
const PATH: &str = "/store/account";

fn item(memory: Memory) -> Item<Memory, Plain> {
    Item {
        fs: memory,
        codec: Plain,
        rules: Rules::Private,
        dir: PathBuf::from(DIR),
        name: Name::new("account").expect("a name"),
    }
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

fn temp_paths(memory: &Memory) -> Vec<String> {
    memory
        .paths()
        .into_iter()
        .filter(|path| path.ends_with(".tmp"))
        .collect()
}

fn step_names(memory: &Memory) -> Vec<String> {
    memory
        .steps()
        .iter()
        .map(|step| step.split(' ').next().unwrap_or("").to_string())
        .collect()
}

fn class(code: i32) -> Result<Locked, Fault> {
    if REFUSALS.contains(&code) {
        Ok(Locked::Refused {
            code: i64::from(code),
        })
    } else if code == NO_CODE {
        Err(Fault { code: -1 })
    } else {
        Err(Fault {
            code: i64::from(code),
        })
    }
}

fn fault_of(code: i32) -> Fault {
    if code == NO_CODE {
        Fault { code: -1 }
    } else {
        Fault {
            code: i64::from(code),
        }
    }
}

fn write_class(code: i32) -> Write {
    match class(code) {
        Ok(locked) => Write::Locked(locked),
        Err(fault) => Write::Fault(fault),
    }
}

fn delete_class(code: i32) -> Delete {
    match class(code) {
        Ok(locked) => Delete::Locked(locked),
        Err(fault) => Delete::Fault(fault),
    }
}

fn stored() -> Memory {
    Memory::with(&[
        (DIR, Entry::dir(0o700)),
        (PATH, Entry::file(0o600, b"the stored secret")),
    ])
}

#[test]
fn the_codes_of_the_tests_have_the_kinds_that_the_tests_need() {
    let kind = |code| io::Error::from_raw_os_error(code).kind();
    assert_eq!(kind(ENOENT), io::ErrorKind::NotFound);
    assert_eq!(kind(EACCES), io::ErrorKind::PermissionDenied);
    assert_ne!(kind(EIO), io::ErrorKind::PermissionDenied);
    assert_ne!(kind(EIO), io::ErrorKind::NotFound);
    for code in REFUSALS {
        assert_eq!(kind(code), io::ErrorKind::PermissionDenied);
    }
    assert_eq!(
        io::Error::other("a failure with no code").raw_os_error(),
        None
    );
}

#[test]
fn a_write_answers_the_class_of_a_failed_second_look_at_its_directory() {
    for code in CODES {
        let store = item(Memory::new().failing_call("walk", 2, code));
        assert_eq!(store.write(&secret(b"secret")), write_class(code), "{code}");
        assert!(!step_names(&store.fs).contains(&"create_temp".to_string()));
        assert_eq!(store.fs.entry(PATH), None);
    }
}

#[test]
fn a_write_answers_the_class_of_a_failed_look_at_the_name() {
    for code in CODES {
        let memory = Memory::with(&[(DIR, Entry::dir(0o700))]).failing("exists", code);
        let store = item(memory);
        assert_eq!(store.write(&secret(b"secret")), write_class(code), "{code}");
        assert!(!step_names(&store.fs).contains(&"create_temp".to_string()));
        assert_eq!(store.fs.paths(), [DIR]);
    }
}

#[test]
fn a_write_whose_directory_is_absent_after_it_was_made_answers_a_fault() {
    let store = item(Memory::new());
    store.fs.makes_no_dir.set(true);
    assert_eq!(
        store.write(&secret(b"secret")),
        Write::Fault(Fault { code: -1 })
    );
    assert!(!step_names(&store.fs).contains(&"create_temp".to_string()));
    assert!(store.fs.paths().is_empty());
}

#[test]
fn a_write_that_finds_every_temporary_name_taken_answers_a_fault() {
    let memory = Memory::with(&[(DIR, Entry::dir(0o700))]);
    let leftover = Entry::file(0o600, b"a leftover");
    for n in 0..TEMP_TRIES {
        memory.put(
            &format!("{DIR}/.account.{}.{n}.tmp", std::process::id()),
            leftover.clone(),
        );
    }
    let store = item(memory);
    assert_eq!(
        store.write(&secret(b"secret")),
        Write::Fault(Fault { code: -1 })
    );
    assert_eq!(store.fs.entry(PATH), None);
    assert_eq!(temp_paths(&store.fs).len(), TEMP_TRIES as usize);
}

#[test]
fn a_store_writes_reads_and_deletes_one_item() {
    let store = item(Memory::new());
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.delete(), Delete::NotStored);
    assert_eq!(
        store.write(&secret(b"first secret, 32 bytes long....")),
        Write::Stored
    );
    assert_eq!(found(store.read()), b"first secret, 32 bytes long....");
    assert_eq!(store.write(&secret(b"second secret")), Write::Occupied);
    assert_eq!(found(store.read()), b"first secret, 32 bytes long....");
    assert_eq!(store.fs.paths(), [DIR, PATH]);
    assert_eq!(store.delete(), Delete::Deleted);
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.delete(), Delete::NotStored);
    assert_eq!(store.fs.paths(), [DIR]);
}

#[test]
fn a_write_makes_the_bytes_durable_before_it_publishes_the_name_and_syncs_the_name_after() {
    let store = item(Memory::new());
    assert_eq!(store.write(&secret(b"secret")), Write::Stored);
    let names = step_names(&store.fs);
    assert_eq!(
        names,
        [
            "walk",
            "make_dir",
            "walk",
            "exists",
            "create_temp",
            "write_temp",
            "sync_temp",
            "publish",
            "sync_dir"
        ]
    );
    let steps = store.fs.steps();
    let temp = format!("{DIR}/.account.{}.0.tmp", std::process::id());
    assert_eq!(steps[4], format!("create_temp {temp}"));
    assert_eq!(steps[7], format!("publish {PATH}"));
    assert_eq!(steps[8], format!("sync_dir {DIR}"));
    assert!(temp_paths(&store.fs).is_empty());
}

#[test]
fn a_write_never_replaces_what_stands_under_the_name() {
    for standing in [
        Entry::file(0o600, b"the stored secret"),
        Entry::file(0o644, b"a wider file"),
        Entry::file(0o600, b"").owned_by(OTHER),
        Entry::of(EntryKind::Link),
        Entry::of(EntryKind::Directory),
        Entry::of(EntryKind::Other),
    ] {
        let store = item(Memory::with(&[
            (DIR, Entry::dir(0o700)),
            (PATH, standing.clone()),
        ]));
        assert_eq!(
            store.write(&secret(b"new")),
            Write::Occupied,
            "{standing:?}"
        );
        assert_eq!(store.fs.entry(PATH), Some(standing.clone()));
        assert!(!step_names(&store.fs).contains(&"create_temp".to_string()));
        assert_eq!(store.fs.paths(), [DIR, PATH]);
    }
}

#[test]
fn a_write_that_loses_the_race_for_the_name_keeps_the_winner_and_its_own_temporary_file_goes() {
    let memory = Memory::with(&[(DIR, Entry::dir(0o700))]);
    let winner = Entry::file(0o600, b"the winner");
    *memory.on_create_temp.borrow_mut() = Some((PathBuf::from(PATH), winner.clone()));
    let store = item(memory);
    assert_eq!(store.write(&secret(b"the loser")), Write::Occupied);
    assert_eq!(store.fs.entry(PATH), Some(winner));
    assert_eq!(store.fs.paths(), [DIR, PATH]);
}

#[test]
fn a_write_passes_over_a_temporary_name_that_is_taken() {
    let leftover = format!("{DIR}/.account.{}.0.tmp", std::process::id());
    let kept = Entry::file(0o600, b"a leftover");
    let store = item(Memory::with(&[
        (DIR, Entry::dir(0o700)),
        (&leftover, kept.clone()),
    ]));
    assert_eq!(store.write(&secret(b"secret")), Write::Stored);
    assert_eq!(found(store.read()), b"secret");
    assert_eq!(store.fs.entry(&leftover), Some(kept));
}

#[test]
fn a_failed_step_of_a_write_answers_its_class_and_publishes_nothing() {
    for step in [
        "walk",
        "make_dir",
        "exists",
        "create_temp",
        "write_temp",
        "sync_temp",
        "publish",
    ] {
        for code in CODES {
            let store = item(Memory::new().failing(step, code));
            assert_eq!(
                store.write(&secret(b"secret")),
                write_class(code),
                "{step} {code}"
            );
            assert_eq!(store.fs.entry(PATH), None, "{step} {code}");
            assert!(temp_paths(&store.fs).is_empty(), "{step} {code}");
        }
    }
}

#[test]
fn a_write_whose_directory_sync_fails_after_the_name_is_published_answers_a_fault() {
    for code in CODES {
        let store = item(Memory::new().failing("sync_dir", code));
        assert_eq!(
            store.write(&secret(b"secret")),
            Write::Fault(fault_of(code)),
            "{code}"
        );
        assert_eq!(store.fs.entry(PATH), Some(Entry::file(0o600, b"secret")));
    }
}

#[test]
fn a_delete_whose_directory_sync_fails_after_the_name_is_removed_answers_a_fault() {
    for code in CODES {
        let store = item(stored().failing("sync_dir", code));
        assert_eq!(store.delete(), Delete::Fault(fault_of(code)), "{code}");
        assert_eq!(store.fs.entry(PATH), None);
    }
}

#[test]
fn a_failed_removal_of_the_temporary_file_keeps_the_answer_of_the_failed_step() {
    let memory = Memory::new()
        .failing("sync_temp", EIO)
        .failing("remove", EACCES);
    let store = item(memory);
    assert_eq!(
        store.write(&secret(b"secret")),
        Write::Fault(Fault {
            code: i64::from(EIO)
        })
    );
    assert_eq!(store.fs.entry(PATH), None);
    assert_eq!(temp_paths(&store.fs).len(), 1);
}

#[test]
fn a_write_that_cannot_seal_answers_the_fault_of_the_codec_and_makes_no_file() {
    let store = Item {
        fs: Memory::new(),
        codec: Reversed,
        rules: Rules::KindOnly,
        dir: PathBuf::from(DIR),
        name: Name::new("account").expect("a name"),
    };
    assert_eq!(
        store.write(&secret(b"unsealable secret")),
        Write::Fault(Fault { code: 77 })
    );
    assert_eq!(store.fs.entry(PATH), None);
    assert!(temp_paths(&store.fs).is_empty());
}

#[test]
fn a_sealed_store_keeps_the_sealed_bytes_and_reads_them_back_open() {
    let store = Item {
        fs: Memory::new(),
        codec: Reversed,
        rules: Rules::KindOnly,
        dir: PathBuf::from(DIR),
        name: Name::new("account").expect("a name"),
    };
    assert_eq!(store.write(&secret(b"abc")), Write::Stored);
    assert_eq!(
        store.fs.entry(PATH).map(|entry| entry.bytes),
        Some(b"cba".to_vec())
    );
    assert_eq!(found(store.read()), b"abc");
    store.fs.put(PATH, Entry::file(0o600, b"!refused"));
    assert!(matches!(
        store.read(),
        Read::Locked(Locked::Refused { code: 99 })
    ));
}

#[test]
fn a_read_that_fails_never_answers_not_stored() {
    for step in ["walk", "open_item", "item_meta", "read_item"] {
        for code in CODES {
            let store = item(stored().failing(step, code));
            let answer = store.read();
            let want = class(code);
            let right = match (&answer, want) {
                (Read::Locked(got), Ok(want)) => *got == want,
                (Read::Fault(got), Err(want)) => *got == want,
                _ => false,
            };
            assert!(right, "{step} {code}: {answer:?}");
        }
    }
}

#[test]
fn a_failed_step_of_a_delete_answers_its_class_and_keeps_the_item() {
    for step in ["walk", "open_item", "item_meta", "remove"] {
        for code in CODES {
            let store = item(stored().failing(step, code));
            let want = if step == "remove" && code == ENOENT {
                Delete::NotStored
            } else {
                delete_class(code)
            };
            assert_eq!(store.delete(), want, "{step} {code}");
            assert!(store.fs.entry(PATH).is_some(), "{step} {code}");
        }
    }
}

#[test]
fn a_delete_removes_the_one_item_and_syncs_its_directory_after() {
    let leftover = Entry::file(0o600, b"leftover");
    let other = Entry::file(0o600, b"other");
    let store = item(Memory::with(&[
        (DIR, Entry::dir(0o700)),
        (PATH, Entry::file(0o600, b"the stored secret")),
        ("/store/.account.1.0.tmp", leftover.clone()),
        ("/store/account.bak", other.clone()),
        ("/store/other", other.clone()),
    ]));
    assert_eq!(store.delete(), Delete::Deleted);
    assert_eq!(
        store.fs.paths(),
        [
            DIR,
            "/store/.account.1.0.tmp",
            "/store/account.bak",
            "/store/other"
        ]
    );
    let steps = store.fs.steps();
    let n = steps.len();
    assert_eq!(steps[n - 2], format!("remove {PATH}"));
    assert_eq!(steps[n - 1], format!("sync_dir {DIR}"));
}

#[test]
fn a_directory_of_a_wider_mode_another_owner_or_another_kind_is_refused_by_every_call() {
    let cases = [
        (Entry::dir(0o750), Locked::Mode),
        (Entry::dir(0o705), Locked::Mode),
        (Entry::dir(0o701), Locked::Mode),
        (Entry::dir(0o710), Locked::Mode),
        (Entry::dir(0o777), Locked::Mode),
        (Entry::dir(0o740), Locked::Mode),
        (Entry::dir(0o720), Locked::Mode),
        (Entry::dir(0o704), Locked::Mode),
        (Entry::dir(0o702), Locked::Mode),
        (Entry::dir(0o700).owned_by(OTHER), Locked::Owner),
        (Entry::dir(0o700).with_acl(), Locked::Acl),
        (Entry::of(EntryKind::Link), Locked::NotADirectory),
        (Entry::of(EntryKind::File), Locked::NotADirectory),
        (Entry::of(EntryKind::Other), Locked::NotADirectory),
    ];
    for (dir, locked) in cases {
        let entries = [(DIR, dir.clone()), (PATH, Entry::file(0o600, b"secret"))];
        let store = item(Memory::with(&entries));
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(got) if got == locked),
            "{dir:?}: {read:?}"
        );
        assert_eq!(
            store.write(&secret(b"new")),
            Write::Locked(locked),
            "{dir:?}"
        );
        assert_eq!(store.delete(), Delete::Locked(locked), "{dir:?}");
        assert_eq!(store.fs.entry(DIR), Some(dir.clone()));
        assert_eq!(store.fs.entry(PATH), Some(Entry::file(0o600, b"secret")));
        assert_eq!(store.fs.paths(), [DIR, PATH]);
    }
    let store = item(Memory::with(&[(DIR, Entry::of(EntryKind::Link))]));
    assert_eq!(
        store.write(&secret(b"new")),
        Write::Locked(Locked::NotADirectory)
    );
    assert!(!step_names(&store.fs).contains(&"make_dir".to_string()));
}

#[test]
fn a_directory_without_bits_for_the_group_and_others_is_taken() {
    for mode in [0o700, 0o500, 0o1700, 0o2700] {
        let entries = [
            (DIR, Entry::dir(mode)),
            (PATH, Entry::file(0o600, b"secret")),
        ];
        let store = item(Memory::with(&entries));
        assert_eq!(found(store.read()), b"secret", "{mode:o}");
    }
}

#[test]
fn an_item_of_a_wider_mode_another_owner_another_kind_or_size_is_refused_and_kept() {
    let too_long = vec![b'x'; MAX_SECRET_LEN + 1];
    let cases = [
        (Entry::file(0o640, b"secret"), Locked::Mode),
        (Entry::file(0o604, b"secret"), Locked::Mode),
        (Entry::file(0o620, b"secret"), Locked::Mode),
        (Entry::file(0o602, b"secret"), Locked::Mode),
        (Entry::file(0o700, b"secret"), Locked::Mode),
        (Entry::file(0o610, b"secret"), Locked::Mode),
        (Entry::file(0o601, b"secret"), Locked::Mode),
        (Entry::file(0o666, b"secret"), Locked::Mode),
        (Entry::file(0o4600, b"secret"), Locked::Mode),
        (Entry::file(0o2600, b"secret"), Locked::Mode),
        (Entry::file(0o1600, b"secret"), Locked::Mode),
        (Entry::file(0o600, b"secret").owned_by(OTHER), Locked::Owner),
        (Entry::file(0o600, b"secret").with_acl(), Locked::Acl),
        (
            Entry::file(0o600, b"secret").with_links(2),
            Locked::HardLink,
        ),
        (
            Entry::file(0o600, b"secret").with_links(0),
            Locked::HardLink,
        ),
        (Entry::of(EntryKind::Directory), Locked::NotAFile),
        (Entry::of(EntryKind::Other), Locked::NotAFile),
        (Entry::of(EntryKind::Link), Locked::NotAFile),
        (Entry::file(0o600, b""), Locked::Size),
        (Entry::file(0o600, &too_long), Locked::Size),
    ];
    for (standing, locked) in cases {
        let entries = [(DIR, Entry::dir(0o700)), (PATH, standing.clone())];
        let store = item(Memory::with(&entries));
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(got) if got == locked),
            "{:o}: {read:?}",
            standing.mode
        );
        assert_eq!(
            store.delete(),
            Delete::Locked(locked),
            "{:o}",
            standing.mode
        );
        assert_eq!(store.fs.entry(PATH), Some(standing.clone()));
        assert_eq!(store.write(&secret(b"new")), Write::Occupied);
        assert_eq!(store.fs.entry(PATH), Some(standing));
    }
}

#[test]
fn a_read_refuses_an_item_whose_bytes_differ_in_number_from_its_size() {
    for (bytes, shown) in [
        (10, 100),
        (20, 10),
        (MAX_SECRET_LEN, 1),
        (1, MAX_SECRET_LEN as u64),
    ] {
        let entry = Entry::file(0o600, &vec![b'x'; bytes]).shown(shown);
        let entries = [(DIR, Entry::dir(0o700)), (PATH, entry)];
        let read = item(Memory::with(&entries)).read();
        assert!(
            matches!(read, Read::Locked(Locked::Size)),
            "{bytes} {shown}: {read:?}"
        );
    }
}

#[test]
fn an_item_of_mode_0400_is_read() {
    let entries = [
        (DIR, Entry::dir(0o700)),
        (PATH, Entry::file(0o400, b"secret")),
    ];
    assert_eq!(found(item(Memory::with(&entries)).read()), b"secret");
}

#[test]
fn an_item_of_the_largest_size_is_read_whole() {
    let largest: Vec<u8> = (0..MAX_SECRET_LEN).map(|i| (i % 253) as u8).collect();
    let entries = [
        (DIR, Entry::dir(0o700)),
        (PATH, Entry::file(0o600, &largest)),
    ];
    assert_eq!(found(item(Memory::with(&entries)).read()), largest);
}

#[test]
fn a_read_stops_at_one_byte_past_the_limit_when_the_item_grew_after_its_size_was_read() {
    for chunk in [1, 7, 8, 4096, MAX_SECRET_LEN, MAX_SECRET_LEN + 1] {
        let grown =
            Entry::file(0o600, &vec![b'x'; MAX_SECRET_LEN + 100]).shown(MAX_SECRET_LEN as u64);
        let entries = [(DIR, Entry::dir(0o700)), (PATH, grown)];
        let store = item(Memory::with(&entries));
        store.fs.chunk.set(chunk);
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(Locked::Size)),
            "{chunk}: {read:?}"
        );
        let reads = step_names(&store.fs)
            .iter()
            .filter(|step| *step == "read_item")
            .count();
        assert_eq!(reads, (MAX_SECRET_LEN + 1).div_ceil(chunk), "{chunk}");
    }
}

fn sealed_item(memory: Memory) -> Item<Memory, Reversed> {
    Item {
        fs: memory,
        codec: Reversed,
        rules: Rules::KindOnly,
        dir: PathBuf::from(DIR),
        name: Name::new("account").expect("a name"),
    }
}

#[test]
fn a_read_refuses_more_bytes_than_the_codec_keeps_before_the_codec_sees_them() {
    let mut grown = Entry::file(0o600, &vec![b'x'; MAX_SECRET_LEN + 100]);
    grown.shown_len = Some(10);
    let store = sealed_item(Memory::with(&[(DIR, Entry::dir(0o700)), (PATH, grown)]));
    assert!(matches!(store.read(), Read::Locked(Locked::Size)));
}

#[test]
fn a_read_refuses_an_item_that_gave_no_byte_before_the_codec_sees_it() {
    let mut emptied = Entry::file(0o600, b"");
    emptied.shown_len = Some(5);
    let store = sealed_item(Memory::with(&[(DIR, Entry::dir(0o700)), (PATH, emptied)]));
    assert!(matches!(store.read(), Read::Locked(Locked::Size)));
}

#[test]
fn the_kind_only_rules_refuse_another_kind_and_take_any_mode_and_owner() {
    let rules = Rules::KindOnly;
    let file = Entry::file(0o777, b"x")
        .owned_by(OTHER)
        .with_acl()
        .with_links(3)
        .meta();
    assert_eq!(rules.item(&file, USER, 10), None);
    let dir = Entry::dir(0o777).with_acl().meta();
    assert_eq!(rules.dir(&dir, USER), None);
    let foreign = Entry::dir(0o700).owned_by(OTHER).meta();
    assert_eq!(rules.dir(&foreign, USER), Some(Locked::Owner));
    for kind in [EntryKind::Link, EntryKind::Directory, EntryKind::Other] {
        assert_eq!(
            rules.item(&Entry::of(kind).meta(), USER, 10),
            Some(Locked::NotAFile)
        );
    }
    for kind in [EntryKind::Link, EntryKind::File, EntryKind::Other] {
        assert_eq!(
            rules.dir(&Entry::of(kind).meta(), USER),
            Some(Locked::NotADirectory)
        );
    }
    assert_eq!(
        rules.item(&Entry::file(0o600, b"").meta(), USER, 10),
        Some(Locked::Size)
    );
    assert_eq!(
        rules.item(&Entry::file(0o600, &[b'x'; 11]).meta(), USER, 10),
        Some(Locked::Size)
    );
}

fn with_ancestor(memory: Memory, ancestor: Entry) -> Memory {
    *memory.ancestors.borrow_mut() = vec![Entry::dir(0o755).owned_by(0), ancestor];
    memory
}

#[test]
fn an_ancestor_that_someone_else_may_change_refuses_every_call() {
    let cases = [
        Entry::dir(0o777).owned_by(0),
        Entry::dir(0o775),
        Entry::dir(0o757),
        Entry::dir(0o722),
        Entry::dir(0o2775),
        Entry::dir(0o755).owned_by(OTHER),
        Entry::dir(0o1777).owned_by(OTHER),
        Entry::dir(0o755).with_acl_write(),
        Entry::of(EntryKind::File),
        Entry::of(EntryKind::Link).owned_by(OTHER),
        Entry::of(EntryKind::Other),
    ];
    for ancestor in cases {
        let store = item(with_ancestor(stored(), ancestor.clone()));
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(Locked::Ancestor)),
            "{ancestor:?}: {read:?}"
        );
        assert_eq!(
            store.write(&secret(b"new")),
            Write::Locked(Locked::Ancestor)
        );
        assert_eq!(store.delete(), Delete::Locked(Locked::Ancestor));
        assert_eq!(
            store.fs.entry(PATH),
            Some(Entry::file(0o600, b"the stored secret"))
        );
        let steps = step_names(&store.fs);
        assert!(
            steps.iter().all(|step| step == "walk"),
            "{ancestor:?}: {steps:?}"
        );

        let fresh = item(with_ancestor(Memory::new(), ancestor.clone()));
        assert_eq!(
            fresh.write(&secret(b"new")),
            Write::Locked(Locked::Ancestor)
        );
        assert!(fresh.fs.paths().is_empty(), "{ancestor:?}");
    }
}

#[test]
fn an_ancestor_of_root_or_of_the_user_that_others_may_not_change_is_taken() {
    let cases = [
        Entry::dir(0o755).owned_by(0),
        Entry::dir(0o700),
        Entry::dir(0o750),
        Entry::dir(0o1777).owned_by(0),
        Entry::dir(0o1777),
        Entry::dir(0o3777),
        Entry::dir(0o755).with_acl(),
        Entry::of(EntryKind::Link),
        Entry::of(EntryKind::Link).owned_by(0),
    ];
    for ancestor in cases {
        let store = item(with_ancestor(stored(), ancestor.clone()));
        assert_eq!(found(store.read()), b"the stored secret", "{ancestor:?}");
    }
}

#[test]
fn a_link_on_the_path_is_judged_by_its_owner_alone() {
    let rules = Rules::Private;
    for owner in [0, USER] {
        let link = Entry::of(EntryKind::Link).owned_by(owner);
        assert_eq!(rules.ancestor(&link.meta(), USER), None, "{owner}");
        let wide = Entry {
            mode: 0o777,
            acl: true,
            acl_write: true,
            ..link
        };
        assert_eq!(rules.ancestor(&wide.meta(), USER), None, "{owner}");
    }
    assert_eq!(
        rules.ancestor(&Entry::of(EntryKind::Link).owned_by(OTHER).meta(), USER),
        Some(Locked::Ancestor)
    );
}

#[test]
fn a_missing_step_above_the_store_directory_answers_a_fault_and_makes_nothing() {
    let store = item(Memory::new());
    store.fs.parent_missing.set(true);
    let fault = Fault {
        code: i64::from(ENOENT),
    };
    assert!(matches!(store.read(), Read::Fault(f) if f == fault));
    assert_eq!(store.write(&secret(b"secret")), Write::Fault(fault));
    assert_eq!(store.delete(), Delete::Fault(fault));
    assert!(store.fs.paths().is_empty());
    let steps = step_names(&store.fs);
    assert!(steps.iter().all(|step| step == "walk"), "{steps:?}");
}

#[test]
fn a_missing_step_under_an_ancestor_that_others_may_change_answers_the_ancestor() {
    let store = item(with_ancestor(Memory::new(), Entry::dir(0o777)));
    store.fs.parent_missing.set(true);
    assert!(matches!(store.read(), Read::Locked(Locked::Ancestor)));
    assert_eq!(
        store.write(&secret(b"secret")),
        Write::Locked(Locked::Ancestor)
    );
}

#[test]
fn a_write_makes_the_store_directory_in_the_parent_that_the_walk_handed_on_and_nothing_else() {
    let store = item(Memory::new());
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.write(&secret(b"secret")), Write::Stored);
    assert_eq!(store.fs.paths(), [DIR, PATH]);
    let made: Vec<String> = store
        .fs
        .steps()
        .into_iter()
        .filter(|step| step.starts_with("make_dir"))
        .collect();
    assert_eq!(made, [format!("make_dir {DIR}")]);
}

#[test]
fn a_delete_whose_removal_fails_with_an_error_that_the_system_does_not_call_absent_answers_a_fault()
{
    let store = item(stored().failing("remove", FOLDED));
    assert_eq!(store.delete(), Delete::Fault(Fault { code: -1 }));
    let store = item(stored().failing("remove", ENOENT));
    assert_eq!(store.delete(), Delete::NotStored);
}

#[test]
fn the_kind_only_rules_refuse_an_ancestor_by_its_kind_and_its_access_list_alone() {
    let rules = Rules::KindOnly;
    assert_eq!(
        rules.ancestor(&Entry::dir(0o777).owned_by(OTHER).meta(), USER),
        None
    );
    assert_eq!(
        rules.ancestor(&Entry::dir(0o700).with_acl_write().meta(), USER),
        Some(Locked::Ancestor)
    );
    for kind in [EntryKind::File, EntryKind::Link, EntryKind::Other] {
        assert_eq!(
            rules.ancestor(&Entry::of(kind).meta(), USER),
            Some(Locked::Ancestor)
        );
    }
}

#[test]
fn an_error_that_carries_a_code_answers_a_fault_with_that_code_and_never_locked() {
    for code in [2, 5, 13, 1314] {
        let error = fault_with(code);
        assert!(
            matches!(Stop::of(&error), Stop::Fault(Fault { code: got }) if got == i64::from(code))
        );
    }
    assert_eq!(
        Stop::fault(&io::Error::other("no code")),
        Fault { code: -1 }
    );
}

fn claimed_paths(memory: &Memory) -> Vec<String> {
    memory
        .paths()
        .into_iter()
        .filter(|path| path.ends_with(".claim"))
        .collect()
}

fn bytes_at(memory: &Memory, path: &str) -> Option<Vec<u8>> {
    memory.entry(path).map(|entry| entry.bytes)
}

#[test]
fn a_removal_for_a_move_claims_the_file_compares_it_and_only_then_removes_it() {
    let store = item(stored());
    assert_eq!(store.remove_if_holds(b"the stored secret"), Claim::Removed);
    assert_eq!(bytes_at(&store.fs, PATH), None);
    assert!(claimed_paths(&store.fs).is_empty());
    let steps = step_names(&store.fs);
    let at = |name: &str| steps.iter().position(|step| step == name);
    assert!(at("publish") < at("read_item"), "{steps:?}");
    assert!(at("read_item") < at("remove"), "{steps:?}");
    assert!(at("remove") < at("sync_dir"), "{steps:?}");
}

#[test]
fn a_removal_for_a_move_names_another_file_back_and_removes_nothing() {
    let store = item(stored());
    assert_eq!(store.remove_if_holds(b"the moved secret!"), Claim::Other);
    assert_eq!(
        bytes_at(&store.fs, PATH),
        Some(b"the stored secret".to_vec())
    );
    assert!(claimed_paths(&store.fs).is_empty());
    assert!(!step_names(&store.fs).contains(&"remove".to_string()));
}

#[test]
fn a_removal_for_a_move_finds_no_file_and_answers_absent() {
    let store = item(Memory::with(&[(DIR, Entry::dir(0o700))]));
    assert_eq!(store.remove_if_holds(b"x"), Claim::Absent);
    let store = item(Memory::new());
    assert_eq!(store.remove_if_holds(b"x"), Claim::Absent);
}

#[test]
fn a_claimed_file_that_cannot_be_named_back_stays_under_its_private_name() {
    let store = item(stored().failing_call("publish", 2, EIO));
    assert_eq!(store.remove_if_holds(b"the moved secret!"), Claim::Stranded);
    assert_eq!(bytes_at(&store.fs, PATH), None);
    let private = claimed_paths(&store.fs);
    assert_eq!(private.len(), 1, "{private:?}");
    assert!(private[0].starts_with("/store/.account."), "{private:?}");
    assert!(matches!(store.read(), Read::NotStored));
}

#[test]
fn a_claimed_file_whose_removal_fails_is_named_back_with_the_failure() {
    let store = item(stored().failing("remove", EIO));
    assert_eq!(
        store.remove_if_holds(b"the stored secret"),
        Claim::Fault(fault_of(EIO))
    );
    assert_eq!(
        bytes_at(&store.fs, PATH),
        Some(b"the stored secret".to_vec())
    );
    assert!(claimed_paths(&store.fs).is_empty());
}

#[test]
fn a_removal_for_a_move_whose_directory_sync_fails_answers_a_fault() {
    let store = item(stored().failing("sync_dir", EIO));
    assert_eq!(
        store.remove_if_holds(b"the stored secret"),
        Claim::Fault(fault_of(EIO))
    );
    assert_eq!(bytes_at(&store.fs, PATH), None);
}

#[test]
fn a_claim_that_the_system_refuses_answers_its_class_and_moves_nothing() {
    for code in REFUSALS {
        let store = item(stored().failing("publish", code));
        let answer = match class(code) {
            Ok(locked) => Claim::Locked(locked),
            Err(fault) => Claim::Fault(fault),
        };
        assert_eq!(store.remove_if_holds(b"the stored secret"), answer);
        assert_eq!(
            bytes_at(&store.fs, PATH),
            Some(b"the stored secret".to_vec())
        );
    }
}

#[test]
fn a_file_that_breaks_a_rule_is_not_claimed() {
    let store = item(Memory::with(&[
        (DIR, Entry::dir(0o700)),
        (PATH, Entry::file(0o644, b"the stored secret")),
    ]));
    assert_eq!(
        store.remove_if_holds(b"the stored secret"),
        Claim::Locked(Locked::Mode)
    );
    assert!(!step_names(&store.fs).contains(&"publish".to_string()));
}

const CLAIMED_PATH: &str = "/store/.account.7.0.claim";

fn claimed(bytes: &[u8]) -> Memory {
    Memory::with(&[
        (DIR, Entry::dir(0o700)),
        (CLAIMED_PATH, Entry::file(0o600, bytes)),
    ])
}

#[test]
fn a_claimed_file_that_holds_the_keychain_bytes_is_removed() {
    let store = item(claimed(b"the moved secret"));
    assert_eq!(store.settle_claimed(Some(b"the moved secret")), Ok(()));
    assert!(claimed_paths(&store.fs).is_empty());
    assert_eq!(bytes_at(&store.fs, PATH), None);
    assert!(step_names(&store.fs).contains(&"sync_dir".to_string()));
}

#[test]
fn a_claimed_file_beside_other_keychain_bytes_or_none_is_put_back_under_the_name() {
    for keychain in [Some(&b"another secret"[..]), None] {
        let store = item(claimed(b"the moved secret"));
        assert_eq!(store.settle_claimed(keychain), Ok(()));
        assert!(claimed_paths(&store.fs).is_empty());
        assert_eq!(
            bytes_at(&store.fs, PATH),
            Some(b"the moved secret".to_vec())
        );
    }
}

#[test]
fn a_claimed_file_that_cannot_be_put_back_or_read_or_removed_answers_the_failure() {
    let store = item(claimed(b"the moved secret"));
    store.fs.put(PATH, Entry::file(0o600, b"a later secret"));
    assert_eq!(store.settle_claimed(None), Err(Err(Fault { code: -1 })));
    assert_eq!(claimed_paths(&store.fs).len(), 1);
    assert_eq!(bytes_at(&store.fs, PATH), Some(b"a later secret".to_vec()));

    let store = item(Memory::with(&[
        (DIR, Entry::dir(0o700)),
        (CLAIMED_PATH, Entry::file(0o644, b"the moved secret")),
    ]));
    assert_eq!(store.settle_claimed(None), Err(Ok(Locked::Mode)));
    assert_eq!(claimed_paths(&store.fs).len(), 1);

    let store = item(claimed(b"the moved secret").failing("read_item", EIO));
    assert_eq!(store.settle_claimed(None), Err(Err(fault_of(EIO))));
    assert_eq!(claimed_paths(&store.fs).len(), 1);
    assert_eq!(bytes_at(&store.fs, PATH), None);

    let store = item(claimed(b"the moved secret").failing("remove", EIO));
    assert_eq!(
        store.settle_claimed(Some(b"the moved secret")),
        Err(Err(fault_of(EIO)))
    );
    assert_eq!(claimed_paths(&store.fs).len(), 1);

    let store = item(claimed(b"the moved secret").failing("sync_dir", EIO));
    assert_eq!(
        store.settle_claimed(Some(b"the moved secret")),
        Err(Err(fault_of(EIO)))
    );

    let store = item(claimed(b"the moved secret").failing("names", EIO));
    assert_eq!(store.settle_claimed(None), Err(Err(fault_of(EIO))));
    assert_eq!(claimed_paths(&store.fs).len(), 1);

    let store = item(claimed(b"the moved secret").failing("publish", EACCES));
    let answer = match class(EACCES) {
        Ok(locked) => Err(Ok(locked)),
        Err(fault) => Err(Err(fault)),
    };
    assert_eq!(store.settle_claimed(None), answer);
}

#[test]
fn claimed_files_are_settled_one_by_one_and_no_other_name_is_touched() {
    let others = [
        "/store/.account.7.0.tmp",
        "/store/.account.x.0.claim",
        "/store/.account.7.claim",
        "/store/.account.7.0.1.claim",
        "/store/.other.7.0.claim",
        "/store/account.7.0.claim",
        "/store/.account..0.claim",
    ];
    let memory = claimed(b"the moved secret");
    memory.put(
        "/store/.account.8.3.claim",
        Entry::file(0o600, b"another secret"),
    );
    for other in others {
        memory.put(other, Entry::file(0o600, b"not a claim"));
    }
    let store = item(memory);
    assert_eq!(store.settle_claimed(Some(b"the moved secret")), Ok(()));
    assert!(claimed_paths(&store.fs)
        .iter()
        .all(|path| others.contains(&path.as_str())));
    assert_eq!(bytes_at(&store.fs, PATH), Some(b"another secret".to_vec()));
    for other in others {
        assert_eq!(
            bytes_at(&store.fs, other),
            Some(b"not a claim".to_vec()),
            "{other}"
        );
    }
}

#[test]
fn a_store_directory_that_is_absent_has_no_claimed_file() {
    let store = item(Memory::new());
    assert_eq!(store.settle_claimed(None), Ok(()));
    assert!(!step_names(&store.fs).contains(&"names".to_string()));
}
