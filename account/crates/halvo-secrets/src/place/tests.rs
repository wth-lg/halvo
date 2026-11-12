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
            links: self.links,
        }
    }
}

fn shown(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

struct Memory {
    entries: RefCell<BTreeMap<PathBuf, Entry>>,
    log: RefCell<Vec<String>>,
    fail: RefCell<Vec<(&'static str, usize, i32)>>,
    calls: RefCell<BTreeMap<&'static str, usize>>,
    makes_no_dir: Cell<bool>,
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
            log: RefCell::new(Vec::new()),
            fail: RefCell::new(Vec::new()),
            calls: RefCell::new(BTreeMap::new()),
            makes_no_dir: Cell::new(false),
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
    type Item = Handle;
    type Temp = Handle;

    fn user(&self) -> u32 {
        USER
    }

    fn lstat(&self, path: &Path) -> io::Result<Option<Meta>> {
        self.step("lstat", path)?;
        Ok(self.entries.borrow().get(path).map(Entry::meta))
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        self.step("create_dir", path)?;
        if self.makes_no_dir.get() {
            return Ok(());
        }
        self.entries
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| Entry::dir(0o700));
        Ok(())
    }

    fn open_item(&self, path: &Path) -> io::Result<Opened<Handle>> {
        self.step("open_item", path)?;
        Ok(match self.entries.borrow().get(path) {
            None => Opened::Absent,
            Some(entry) if entry.kind == EntryKind::Link => Opened::NotAFile,
            Some(_) => Opened::Item(Handle {
                path: path.to_path_buf(),
                offset: 0,
            }),
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

    fn create_temp(&self, path: &Path) -> io::Result<Option<Handle>> {
        self.step("create_temp", path)?;
        if let Some((racer, entry)) = self.on_create_temp.borrow_mut().take() {
            self.entries.borrow_mut().insert(racer, entry);
        }
        let mut entries = self.entries.borrow_mut();
        if entries.contains_key(path) {
            return Ok(None);
        }
        entries.insert(path.to_path_buf(), Entry::file(0o600, b""));
        Ok(Some(Handle {
            path: path.to_path_buf(),
            offset: 0,
        }))
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

    fn publish(&self, from: &Path, to: &Path) -> io::Result<Published> {
        self.step("publish", to)?;
        let mut entries = self.entries.borrow_mut();
        if entries.contains_key(to) {
            return Ok(Published::Exists);
        }
        let entry = entries
            .remove(from)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        entries.insert(to.to_path_buf(), entry);
        Ok(Published::Done)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        self.step("remove", path)?;
        self.entries
            .borrow_mut()
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.step("sync_dir", path)
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
        let store = item(Memory::new().failing_call("lstat", 2, code));
        assert_eq!(store.write(&secret(b"secret")), write_class(code), "{code}");
        assert!(!step_names(&store.fs).contains(&"create_temp".to_string()));
        assert_eq!(store.fs.entry(PATH), None);
    }
}

#[test]
fn a_write_answers_the_class_of_a_failed_look_at_the_name() {
    for code in CODES {
        let memory = Memory::with(&[(DIR, Entry::dir(0o700))]).failing_call("lstat", 2, code);
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
            "lstat",
            "create_dir",
            "lstat",
            "lstat",
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
        "lstat",
        "create_dir",
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
    for step in ["lstat", "open_item", "item_meta", "read_item"] {
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
    for step in ["lstat", "open_item", "item_meta", "remove"] {
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
    assert!(!step_names(&store.fs).contains(&"create_dir".to_string()));
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
    let dir = Entry::dir(0o777).owned_by(OTHER).with_acl().meta();
    assert_eq!(rules.dir(&dir, USER), None);
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
