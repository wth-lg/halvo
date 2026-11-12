use super::*;
use crate::answer::{Fault, Locked};
use crate::keychain::{AUTH_FAILED, DUPLICATE_ITEM, ITEM_NOT_FOUND};
use crate::Name;
use std::cell::RefCell;

const NOT_AVAILABLE: i32 = -25291;

#[derive(Clone, Copy)]
enum Fails {
    Locked(Locked),
    Fault(Fault),
    Absent,
}

struct File {
    bytes: RefCell<Option<Vec<u8>>>,
    read_fails: Option<Fails>,
    delete_fails: Option<Fails>,
    log: RefCell<Vec<&'static str>>,
}

impl File {
    fn empty() -> File {
        File {
            bytes: RefCell::new(None),
            read_fails: None,
            delete_fails: None,
            log: RefCell::new(Vec::new()),
        }
    }

    fn holding(bytes: &[u8]) -> File {
        let file = File::empty();
        *file.bytes.borrow_mut() = Some(bytes.to_vec());
        file
    }

    fn failing_read(mut self, fails: Fails) -> File {
        self.read_fails = Some(fails);
        self
    }

    fn failing_delete(mut self, fails: Fails) -> File {
        self.delete_fails = Some(fails);
        self
    }

    fn bytes(&self) -> Option<Vec<u8>> {
        self.bytes.borrow().clone()
    }

    fn ops(&self) -> Vec<&'static str> {
        self.log.borrow().clone()
    }
}

impl FileSide for File {
    fn read(&self) -> Read {
        self.log.borrow_mut().push("read");
        match self.read_fails {
            Some(Fails::Locked(locked)) => return Read::Locked(locked),
            Some(Fails::Fault(fault)) => return Read::Fault(fault),
            Some(Fails::Absent) => return Read::NotStored,
            None => {}
        }
        match self.bytes() {
            Some(bytes) => Read::Found(Secret::new(bytes).expect("a secret")),
            None => Read::NotStored,
        }
    }

    fn write(&self, secret: &Secret) -> Write {
        self.log.borrow_mut().push("write");
        let mut bytes = self.bytes.borrow_mut();
        if bytes.is_some() {
            return Write::Occupied;
        }
        *bytes = Some(secret.expose().to_vec());
        Write::Stored
    }

    fn delete(&self) -> Delete {
        self.log.borrow_mut().push("delete");
        match self.delete_fails {
            Some(Fails::Locked(locked)) => return Delete::Locked(locked),
            Some(Fails::Fault(fault)) => return Delete::Fault(fault),
            Some(Fails::Absent) => return Delete::NotStored,
            None => {}
        }
        match self.bytes.borrow_mut().take() {
            Some(_) => Delete::Deleted,
            None => Delete::NotStored,
        }
    }
}

struct Keys {
    items: RefCell<Vec<Vec<u8>>>,
    fail: Vec<(&'static str, i32)>,
    gives_back_other_bytes: bool,
    drops_adds: bool,
    log: RefCell<Vec<&'static str>>,
}

impl Keys {
    fn holding(items: &[&[u8]]) -> Keys {
        Keys {
            items: RefCell::new(items.iter().map(|bytes| bytes.to_vec()).collect()),
            fail: Vec::new(),
            gives_back_other_bytes: false,
            drops_adds: false,
            log: RefCell::new(Vec::new()),
        }
    }

    fn giving_back_other_bytes(mut self) -> Keys {
        self.gives_back_other_bytes = true;
        self
    }

    fn dropping_adds(mut self) -> Keys {
        self.drops_adds = true;
        self
    }

    fn empty() -> Keys {
        Keys::holding(&[])
    }

    fn failing(mut self, op: &'static str, code: i32) -> Keys {
        self.fail.push((op, code));
        self
    }

    fn check(&self, op: &'static str) -> Result<(), i32> {
        self.log.borrow_mut().push(op);
        match self.fail.iter().find(|(name, _)| *name == op) {
            Some((_, code)) => Err(*code),
            None => Ok(()),
        }
    }

    fn items(&self) -> Vec<Vec<u8>> {
        self.items.borrow().clone()
    }

    fn ops(&self) -> Vec<&'static str> {
        self.log.borrow().clone()
    }
}

impl Keychain for Keys {
    fn count(&self, _service: &str, _account: &str) -> Result<usize, i32> {
        self.check("count")?;
        match self.items.borrow().len() {
            0 => Err(ITEM_NOT_FOUND),
            n => Ok(n),
        }
    }

    fn data(&self, _service: &str, _account: &str) -> Result<Vec<u8>, i32> {
        self.check("data")?;
        let mut bytes = self.items.borrow().first().cloned().ok_or(ITEM_NOT_FOUND)?;
        if self.gives_back_other_bytes {
            bytes.push(b'!');
        }
        Ok(bytes)
    }

    fn add(&self, _service: &str, _account: &str, bytes: &[u8]) -> Result<(), i32> {
        self.check("add")?;
        if !self.items.borrow().is_empty() {
            return Err(DUPLICATE_ITEM);
        }
        if !self.drops_adds {
            self.items.borrow_mut().push(bytes.to_vec());
        }
        Ok(())
    }

    fn delete(&self, _service: &str, _account: &str) -> Result<(), i32> {
        self.check("delete")?;
        if self.items.borrow().is_empty() {
            return Err(ITEM_NOT_FOUND);
        }
        self.items.borrow_mut().clear();
        Ok(())
    }
}

fn mac(keeps: Keeps, file: File, keys: Keys) -> Mac<File, Keys> {
    Mac {
        keeps,
        file,
        keychain: KeychainItem {
            keychain: keys,
            service: Name::new("dev.halvo.test").expect("a service"),
            account: Name::new("account").expect("an account"),
        },
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

fn refused(code: i32) -> Locked {
    Locked::Refused {
        code: i64::from(code),
    }
}

fn fault(code: i32) -> Fault {
    Fault {
        code: i64::from(code),
    }
}

#[test]
fn a_build_without_the_feature_that_finds_a_keychain_item_answers_other_store() {
    for file in [&b""[..], b"in the file"] {
        for items in [vec![&b"in the keychain"[..]], vec![b"one", b"two"]] {
            let held = if file.is_empty() {
                File::empty()
            } else {
                File::holding(file)
            };
            let store = mac(Keeps::File, held, Keys::holding(&items));
            let read = store.read();
            assert!(
                matches!(read, Read::Locked(Locked::OtherStore)),
                "{read:?} with {} items",
                items.len()
            );
            assert_eq!(store.keychain.keychain.ops(), ["count"]);
            assert_eq!(store.keychain.keychain.items().len(), items.len());
            let expected = (!file.is_empty()).then(|| file.to_vec());
            assert_eq!(store.file.bytes(), expected);
        }
    }
}

#[test]
fn a_build_without_the_feature_reads_the_file_when_the_keychain_holds_nothing() {
    let store = mac(Keeps::File, File::holding(b"in the file"), Keys::empty());
    assert_eq!(found(store.read()), b"in the file");
    assert_eq!(store.keychain.keychain.ops(), ["count"]);

    let store = mac(Keeps::File, File::empty(), Keys::empty());
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.file.ops(), ["read"]);
}

#[test]
fn a_build_without_the_feature_never_answers_not_stored_when_the_keychain_cannot_be_searched() {
    let store = mac(
        Keeps::File,
        File::empty(),
        Keys::empty().failing("count", AUTH_FAILED),
    );
    let read = store.read();
    assert!(
        matches!(read, Read::Locked(locked) if locked == refused(AUTH_FAILED)),
        "{read:?}"
    );

    let store = mac(
        Keeps::File,
        File::empty(),
        Keys::empty().failing("count", NOT_AVAILABLE),
    );
    let read = store.read();
    assert!(
        matches!(read, Read::Fault(code) if code == fault(NOT_AVAILABLE)),
        "{read:?}"
    );

    for code in [AUTH_FAILED, NOT_AVAILABLE] {
        let store = mac(
            Keeps::File,
            File::holding(b"in the file"),
            Keys::empty().failing("count", code),
        );
        assert_eq!(found(store.read()), b"in the file", "{code}");
    }
}

#[test]
fn a_build_without_the_feature_answers_the_failure_of_its_file_when_the_keychain_holds_nothing() {
    for fails in [
        Fails::Locked(Locked::Ancestor),
        Fails::Fault(Fault { code: 5 }),
    ] {
        let store = mac(
            Keeps::File,
            File::empty().failing_read(fails),
            Keys::empty(),
        );
        let read = store.read();
        match fails {
            Fails::Locked(locked) => {
                assert!(matches!(read, Read::Locked(l) if l == locked), "{read:?}")
            }
            Fails::Fault(fault) => {
                assert!(matches!(read, Read::Fault(f) if f == fault), "{read:?}")
            }
            Fails::Absent => unreachable!(),
        }
    }
}

#[test]
fn a_build_without_the_feature_never_writes_or_deletes_a_keychain_item() {
    let store = mac(
        Keeps::File,
        File::empty(),
        Keys::holding(&[b"in the keychain"]),
    );
    assert_eq!(store.write(&secret(b"new")), Write::Stored);
    assert_eq!(store.file.bytes(), Some(b"new".to_vec()));
    assert_eq!(store.delete(), Delete::Deleted);
    assert_eq!(store.file.bytes(), None);
    assert!(store.keychain.keychain.ops().is_empty());
    assert_eq!(
        store.keychain.keychain.items(),
        [b"in the keychain".to_vec()]
    );
}

const FILE: &[u8] = b"kept by an unsigned build";

fn moved(code: i64) -> Locked {
    Locked::Move { code }
}

fn adds(keys: &Keys) -> usize {
    keys.ops().iter().filter(|op| **op == "add").count()
}

fn assert_file_kept(store: &Mac<File, Keys>) {
    assert_eq!(store.file.bytes(), Some(FILE.to_vec()));
    assert!(
        !store.file.ops().contains(&"delete"),
        "{:?}",
        store.file.ops()
    );
}

#[test]
fn a_file_of_an_earlier_build_moves_once_into_the_keychain_and_reads_found_after() {
    let store = mac(Keeps::Keychain, File::holding(FILE), Keys::empty());
    assert_eq!(found(store.read()), FILE);
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
    assert_eq!(store.file.bytes(), None);
    assert_eq!(store.file.ops(), ["read", "delete"]);
    assert_eq!(
        store.keychain.keychain.ops(),
        ["count", "count", "add", "count", "data"]
    );
    assert_eq!(found(store.read()), FILE);
    assert_eq!(found(store.read()), FILE);
    assert_eq!(adds(&store.keychain.keychain), 1);
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
}

#[test]
fn a_failed_add_leaves_the_file_and_answers_locked_with_its_code() {
    for (code, answer) in [
        (AUTH_FAILED, moved(i64::from(AUTH_FAILED))),
        (NOT_AVAILABLE, moved(i64::from(NOT_AVAILABLE))),
        (DUPLICATE_ITEM, moved(-1)),
    ] {
        let store = mac(
            Keeps::Keychain,
            File::holding(FILE),
            Keys::empty().failing("add", code),
        );
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(locked) if locked == answer),
            "{code}: {read:?}"
        );
        assert_file_kept(&store);
        assert!(store.keychain.keychain.items().is_empty());
        assert!(!store.keychain.keychain.ops().contains(&"data"));
    }
}

#[test]
fn a_read_back_that_differs_leaves_the_file_and_answers_locked() {
    let store = mac(
        Keeps::Keychain,
        File::holding(FILE),
        Keys::empty().giving_back_other_bytes(),
    );
    let read = store.read();
    assert!(
        matches!(read, Read::Locked(locked) if locked == moved(-1)),
        "{read:?}"
    );
    assert_file_kept(&store);
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
    let next = store.read();
    assert!(matches!(next, Read::Locked(Locked::OtherStore)), "{next:?}");
    assert_file_kept(&store);
    assert_eq!(adds(&store.keychain.keychain), 1);
}

#[test]
fn a_read_back_that_fails_or_finds_nothing_leaves_the_file_and_answers_locked() {
    for (keys, answer) in [
        (
            Keys::empty().failing("data", AUTH_FAILED),
            moved(i64::from(AUTH_FAILED)),
        ),
        (
            Keys::empty().failing("data", NOT_AVAILABLE),
            moved(i64::from(NOT_AVAILABLE)),
        ),
        (Keys::empty().dropping_adds(), moved(-1)),
    ] {
        let store = mac(Keeps::Keychain, File::holding(FILE), keys);
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(locked) if locked == answer),
            "{read:?}"
        );
        assert_file_kept(&store);
    }
}

#[test]
fn a_file_that_is_not_removed_after_an_equal_read_back_answers_locked() {
    for (fails, answer) in [
        (Fails::Fault(Fault { code: 5 }), moved(5)),
        (Fails::Locked(refused(13)), moved(13)),
        (Fails::Locked(Locked::Mode), moved(-1)),
        (Fails::Absent, moved(-1)),
    ] {
        let store = mac(
            Keeps::Keychain,
            File::holding(FILE).failing_delete(fails),
            Keys::empty(),
        );
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(locked) if locked == answer),
            "{read:?}"
        );
        assert_eq!(store.file.ops(), ["read", "delete"]);
        assert_eq!(store.file.bytes(), Some(FILE.to_vec()));
        assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
    }
}

#[test]
fn with_both_places_full_a_build_with_the_feature_answers_locked_and_changes_nothing() {
    for in_file in [FILE, b"in the keychain"] {
        let store = mac(
            Keeps::Keychain,
            File::holding(in_file),
            Keys::holding(&[b"in the keychain"]),
        );
        for _ in 0..2 {
            let read = store.read();
            assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
        }
        assert_eq!(store.file.bytes(), Some(in_file.to_vec()));
        assert_eq!(
            store.keychain.keychain.items(),
            [b"in the keychain".to_vec()]
        );
        assert_eq!(adds(&store.keychain.keychain), 0);
        assert!(!store.keychain.keychain.ops().contains(&"delete"));
        assert!(!store.file.ops().contains(&"delete"));
    }
}

#[test]
fn a_build_with_the_feature_reads_its_keychain_item_when_the_file_holds_none_or_cannot_be_read() {
    for file in [
        File::empty(),
        File::empty().failing_read(Fails::Locked(Locked::Ancestor)),
        File::empty().failing_read(Fails::Fault(Fault { code: 5 })),
    ] {
        let store = mac(Keeps::Keychain, file, Keys::holding(&[b"in the keychain"]));
        assert_eq!(found(store.read()), b"in the keychain");
        assert_eq!(adds(&store.keychain.keychain), 0);
        assert!(!store.file.ops().contains(&"delete"));
    }
}

#[test]
fn a_build_with_the_feature_answers_its_file_when_the_keychain_holds_none() {
    let store = mac(Keeps::Keychain, File::empty(), Keys::empty());
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.file.ops(), ["read"]);
    for fails in [
        Fails::Locked(Locked::Ancestor),
        Fails::Fault(Fault { code: 5 }),
    ] {
        let store = mac(
            Keeps::Keychain,
            File::empty().failing_read(fails),
            Keys::empty(),
        );
        let read = store.read();
        match fails {
            Fails::Locked(locked) => {
                assert!(matches!(read, Read::Locked(l) if l == locked), "{read:?}")
            }
            Fails::Fault(fault) => {
                assert!(matches!(read, Read::Fault(f) if f == fault), "{read:?}")
            }
            Fails::Absent => unreachable!(),
        }
        assert_eq!(adds(&store.keychain.keychain), 0);
    }
}

#[test]
fn a_build_with_the_feature_answers_a_failed_keychain_and_keeps_the_file() {
    for (keys, answer) in [
        (
            Keys::empty().failing("count", AUTH_FAILED),
            Read::Locked(refused(AUTH_FAILED)),
        ),
        (
            Keys::empty().failing("count", NOT_AVAILABLE),
            Read::Fault(fault(NOT_AVAILABLE)),
        ),
        (
            Keys::holding(&[b"one", b"two"]),
            Read::Locked(Locked::Duplicate),
        ),
    ] {
        let store = mac(Keeps::Keychain, File::holding(FILE), keys);
        let read = store.read();
        assert_eq!(format!("{read:?}"), format!("{answer:?}"));
        assert_file_kept(&store);
        assert_eq!(adds(&store.keychain.keychain), 0);
    }
}

#[test]
fn a_build_with_the_feature_writes_and_deletes_the_keychain_item_alone() {
    let store = mac(Keeps::Keychain, File::holding(FILE), Keys::empty());
    assert_eq!(store.write(&secret(b"new")), Write::Stored);
    assert_eq!(store.keychain.keychain.items(), [b"new".to_vec()]);
    assert_eq!(store.delete(), Delete::Deleted);
    assert!(store.keychain.keychain.items().is_empty());
    assert!(store.file.ops().is_empty());
    assert_eq!(store.file.bytes(), Some(FILE.to_vec()));
}
