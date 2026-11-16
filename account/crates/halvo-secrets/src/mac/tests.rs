use super::*;
use crate::answer::{Fault, Locked};
use crate::keychain::{AUTH_FAILED, DUPLICATE_ITEM, ITEM_NOT_FOUND};
use crate::Name;
use std::cell::RefCell;
use std::rc::Rc;

const NOT_AVAILABLE: i32 = -25291;

type Items = Rc<RefCell<Vec<Vec<u8>>>>;

struct SharedKeys {
    items: Items,
}

impl Keychain for SharedKeys {
    fn count(&self, _service: &str, _account: &str) -> Result<usize, i32> {
        match self.items.borrow().len() {
            0 => Err(ITEM_NOT_FOUND),
            n => Ok(n),
        }
    }

    fn data(&self, _service: &str, _account: &str) -> Result<Vec<u8>, i32> {
        self.items.borrow().first().cloned().ok_or(ITEM_NOT_FOUND)
    }

    fn add(&self, _service: &str, _account: &str, bytes: &[u8]) -> Result<(), i32> {
        if !self.items.borrow().is_empty() {
            return Err(DUPLICATE_ITEM);
        }
        self.items.borrow_mut().push(bytes.to_vec());
        Ok(())
    }

    fn delete(&self, _service: &str, _account: &str) -> Result<(), i32> {
        if self.items.borrow().is_empty() {
            return Err(ITEM_NOT_FOUND);
        }
        self.items.borrow_mut().clear();
        Ok(())
    }
}

struct MovedUnderTheRead {
    bytes: RefCell<Option<Vec<u8>>>,
    keys: Items,
}

impl FileSide for MovedUnderTheRead {
    fn read(&self) -> Read {
        if let Some(moved) = self.bytes.borrow_mut().take() {
            self.keys.borrow_mut().push(moved);
        }
        Read::NotStored
    }

    fn write(&self, _secret: &Secret) -> Write {
        Write::Occupied
    }

    fn delete(&self) -> Delete {
        Delete::NotStored
    }

    fn settle(&self, _keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
        Ok(())
    }

    fn remove_if_holds(&self, _bytes: &[u8]) -> Claim {
        Claim::Absent
    }
}

fn moved_under_the_read(keeps: Keeps, bytes: &[u8]) -> (Mac<MovedUnderTheRead, SharedKeys>, Items) {
    let items: Items = Rc::new(RefCell::new(Vec::new()));
    let store = Mac {
        keeps,
        file: MovedUnderTheRead {
            bytes: RefCell::new(Some(bytes.to_vec())),
            keys: Rc::clone(&items),
        },
        keychain: KeychainItem {
            keychain: SharedKeys {
                items: Rc::clone(&items),
            },
            service: Name::new("dev.halvo.test").expect("a service"),
            account: Name::new("account").expect("an account"),
        },
    };
    (store, items)
}

#[derive(Clone, Copy)]
enum Fails {
    Locked(Locked),
    Fault(Fault),
    Absent,
    Stranded,
}

struct File {
    bytes: RefCell<Option<Vec<u8>>>,
    read_fails: Option<Fails>,
    delete_fails: Option<Fails>,
    claim_fails: Option<Fails>,
    settle_fails: Option<Fails>,
    log: RefCell<Vec<&'static str>>,
}

impl File {
    fn empty() -> File {
        File {
            bytes: RefCell::new(None),
            read_fails: None,
            delete_fails: None,
            claim_fails: None,
            settle_fails: None,
            log: RefCell::new(Vec::new()),
        }
    }

    fn failing_settle(mut self, fails: Fails) -> File {
        self.settle_fails = Some(fails);
        self
    }

    fn failing_claim(mut self, fails: Fails) -> File {
        self.claim_fails = Some(fails);
        self
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
            Some(Fails::Absent | Fails::Stranded) => return Read::NotStored,
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
            Some(Fails::Absent | Fails::Stranded) => return Delete::NotStored,
            None => {}
        }
        match self.bytes.borrow_mut().take() {
            Some(_) => Delete::Deleted,
            None => Delete::NotStored,
        }
    }

    fn settle(&self, _keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
        match self.settle_fails {
            Some(Fails::Locked(locked)) => Err(Ok(locked)),
            Some(Fails::Fault(fault)) => Err(Err(fault)),
            _ => Ok(()),
        }
    }

    fn remove_if_holds(&self, bytes: &[u8]) -> Claim {
        self.log.borrow_mut().push("claim");
        match self.claim_fails {
            Some(Fails::Locked(locked)) => return Claim::Locked(locked),
            Some(Fails::Fault(fault)) => return Claim::Fault(fault),
            Some(Fails::Absent) => return Claim::Absent,
            Some(Fails::Stranded) => return Claim::Stranded,
            None => {}
        }
        let mut held = self.bytes.borrow_mut();
        match held.as_deref() {
            Some(stored) if stored == bytes => {
                *held = None;
                Claim::Removed
            }
            Some(_) => Claim::Other,
            None => Claim::Absent,
        }
    }
}

struct Keys {
    items: RefCell<Vec<Vec<u8>>>,
    fail: Vec<(&'static str, usize, i32)>,
    gives_back_other_bytes: bool,
    drops_adds: bool,
    racing: Option<Vec<u8>>,
    log: RefCell<Vec<&'static str>>,
}

impl Keys {
    fn holding(items: &[&[u8]]) -> Keys {
        Keys {
            items: RefCell::new(items.iter().map(|bytes| bytes.to_vec()).collect()),
            fail: Vec::new(),
            gives_back_other_bytes: false,
            drops_adds: false,
            racing: None,
            log: RefCell::new(Vec::new()),
        }
    }

    fn racing(mut self, bytes: &[u8]) -> Keys {
        self.racing = Some(bytes.to_vec());
        self
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

    fn failing(self, op: &'static str, code: i32) -> Keys {
        self.failing_from(op, 1, code)
    }

    fn failing_from(mut self, op: &'static str, call: usize, code: i32) -> Keys {
        self.fail.push((op, call, code));
        self
    }

    fn check(&self, op: &'static str) -> Result<(), i32> {
        self.log.borrow_mut().push(op);
        let calls = self.log.borrow().iter().filter(|done| **done == op).count();
        match self
            .fail
            .iter()
            .find(|(name, from, _)| *name == op && calls >= *from)
        {
            Some((_, _, code)) => Err(*code),
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
            if let Some(last) = bytes.last_mut() {
                *last ^= 1;
            }
        }
        Ok(bytes)
    }

    fn add(&self, _service: &str, _account: &str, bytes: &[u8]) -> Result<(), i32> {
        self.check("add")?;
        if let Some(winner) = &self.racing {
            self.items.borrow_mut().push(winner.clone());
        }
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
            Fails::Absent | Fails::Stranded => unreachable!(),
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
    assert_eq!(
        store.write(&secret(b"new")),
        Write::Locked(Locked::OtherStore)
    );
    assert_eq!(store.file.bytes(), None);
    assert_eq!(store.delete(), Delete::Locked(Locked::OtherStore));
    assert_eq!(store.file.bytes(), None);
    assert!(store
        .keychain
        .keychain
        .ops()
        .iter()
        .all(|op| *op == "count"));
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
        !store.file.ops().contains(&"delete") && !store.file.ops().contains(&"claim"),
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
    assert_eq!(store.file.ops(), ["read", "claim"]);
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
fn a_move_that_loses_the_race_for_the_keychain_item_reads_again_and_finishes_it() {
    let store = mac(
        Keeps::Keychain,
        File::holding(FILE),
        Keys::empty().racing(FILE),
    );
    assert_eq!(found(store.read()), FILE);
    assert_eq!(store.file.bytes(), None);
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
    assert_eq!(adds(&store.keychain.keychain), 1);
    assert_eq!(found(store.read()), FILE);
    assert_eq!(adds(&store.keychain.keychain), 1);

    let store = mac(
        Keeps::Keychain,
        File::holding(FILE),
        Keys::empty().racing(b"another credential"),
    );
    let read = store.read();
    assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
    assert_file_kept(&store);
    assert_eq!(adds(&store.keychain.keychain), 1);
}

#[test]
fn a_second_lost_race_for_the_keychain_item_answers_locked_with_move() {
    let store = mac(
        Keeps::Keychain,
        File::holding(FILE),
        Keys::empty().failing("add", DUPLICATE_ITEM),
    );
    let read = store.read();
    assert!(
        matches!(read, Read::Locked(locked) if locked == moved(-1)),
        "{read:?}"
    );
    assert_file_kept(&store);
    assert_eq!(adds(&store.keychain.keychain), 2);
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
        (Fails::Locked(refused(13)), moved(13)),
        (Fails::Locked(Locked::Mode), moved(-1)),
    ] {
        let mut store = mac(
            Keeps::Keychain,
            File::holding(FILE).failing_claim(fails),
            Keys::empty(),
        );
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(locked) if locked == answer),
            "{read:?}"
        );
        assert_eq!(store.file.ops(), ["read", "claim"]);
        assert_eq!(store.file.bytes(), Some(FILE.to_vec()));
        assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
        store.file.claim_fails = None;
        assert_eq!(found(store.read()), FILE);
        assert_eq!(store.file.bytes(), None);
        assert_eq!(adds(&store.keychain.keychain), 1);
    }
}

#[test]
fn a_move_whose_removal_of_the_file_faults_answers_the_fault_and_the_next_read_finishes_it() {
    let mut store = mac(
        Keeps::Keychain,
        File::holding(FILE).failing_claim(Fails::Fault(Fault { code: 5 })),
        Keys::empty(),
    );
    let read = store.read();
    assert!(matches!(read, Read::Fault(Fault { code: 5 })), "{read:?}");
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
    store.file.claim_fails = None;
    assert_eq!(found(store.read()), FILE);
    assert_eq!(store.file.bytes(), None);
    assert_eq!(adds(&store.keychain.keychain), 1);
}

#[test]
fn with_both_places_full_a_build_with_the_feature_answers_locked_and_changes_nothing() {
    let mut one_byte_off = FILE.to_vec();
    one_byte_off[FILE.len() / 2] ^= 1;
    for in_keychain in [&b"in the keychain"[..], &one_byte_off] {
        assert!(in_keychain != FILE);
        let store = mac(
            Keeps::Keychain,
            File::holding(FILE),
            Keys::holding(&[in_keychain]),
        );
        for _ in 0..2 {
            let read = store.read();
            assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
        }
        assert_eq!(store.file.bytes(), Some(FILE.to_vec()));
        assert_eq!(store.keychain.keychain.items(), [in_keychain.to_vec()]);
        assert_eq!(adds(&store.keychain.keychain), 0);
        assert!(!store.keychain.keychain.ops().contains(&"delete"));
        assert!(!store.file.ops().contains(&"delete"));
        assert!(!store.file.ops().contains(&"claim"));
    }
}

#[test]
fn credentials_of_one_length_that_differ_in_one_byte_are_never_taken_for_equal() {
    let mut one_byte_off = FILE.to_vec();
    one_byte_off[0] ^= 1;
    assert_eq!(one_byte_off.len(), FILE.len());
    let store = mac(
        Keeps::Keychain,
        File::holding(FILE),
        Keys::holding(&[&one_byte_off]),
    );
    let read = store.read();
    assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
    assert_eq!(store.file.bytes(), Some(FILE.to_vec()));
    assert_eq!(store.keychain.keychain.items(), [one_byte_off.clone()]);

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
}

#[test]
fn both_places_holding_equal_bytes_finish_the_move_that_stopped_before_the_file_was_removed() {
    let store = mac(Keeps::Keychain, File::holding(FILE), Keys::holding(&[FILE]));
    assert_eq!(found(store.read()), FILE);
    assert_eq!(store.file.bytes(), None);
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
    assert_eq!(found(store.read()), FILE);
    assert_eq!(adds(&store.keychain.keychain), 0);

    let store = mac(
        Keeps::Keychain,
        File::holding(FILE).failing_claim(Fails::Locked(refused(13))),
        Keys::holding(&[FILE]),
    );
    let read = store.read();
    assert!(
        matches!(read, Read::Locked(locked) if locked == moved(13)),
        "{read:?}"
    );
    assert_eq!(store.file.bytes(), Some(FILE.to_vec()));

    let unsigned = mac(Keeps::File, File::holding(FILE), Keys::holding(&[FILE]));
    let read = unsigned.read();
    assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
    assert_eq!(unsigned.file.bytes(), Some(FILE.to_vec()));
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
        assert!(!store.file.ops().contains(&"claim"));
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
            Fails::Absent | Fails::Stranded => unreachable!(),
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
    let store = mac(Keeps::Keychain, File::empty(), Keys::empty());
    assert_eq!(store.write(&secret(b"new")), Write::Stored);
    assert_eq!(store.keychain.keychain.items(), [b"new".to_vec()]);
    assert_eq!(store.delete(), Delete::Deleted);
    assert!(store.keychain.keychain.items().is_empty());
    assert!(!store.file.ops().contains(&"write"));
    assert!(!store.file.ops().contains(&"delete"));
    assert_eq!(store.file.bytes(), None);
}

#[test]
fn a_delete_removes_the_item_that_a_read_gives_and_nothing_when_a_read_gives_none() {
    let store = mac(Keeps::Keychain, File::holding(FILE), Keys::empty());
    assert_eq!(store.delete(), Delete::Deleted);
    assert_eq!(store.file.bytes(), None);
    assert!(store.keychain.keychain.items().is_empty());
    assert!(matches!(store.read(), Read::NotStored));

    let store = mac(Keeps::Keychain, File::holding(FILE), Keys::holding(&[FILE]));
    assert_eq!(store.delete(), Delete::Deleted);
    assert_eq!(store.file.bytes(), None);
    assert!(store.keychain.keychain.items().is_empty());

    let store = mac(
        Keeps::Keychain,
        File::holding(FILE),
        Keys::holding(&[b"in the keychain"]),
    );
    assert_eq!(store.delete(), Delete::Locked(Locked::OtherStore));
    assert_eq!(store.file.bytes(), Some(FILE.to_vec()));
    assert_eq!(
        store.keychain.keychain.items(),
        [b"in the keychain".to_vec()]
    );

    let store = mac(Keeps::File, File::holding(FILE), Keys::empty());
    assert_eq!(store.delete(), Delete::Deleted);
    assert_eq!(store.file.bytes(), None);
    assert!(matches!(store.read(), Read::NotStored));

    let store = mac(Keeps::File, File::empty(), Keys::holding(&[FILE]));
    assert_eq!(store.delete(), Delete::Locked(Locked::OtherStore));
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
    assert!(!store.keychain.keychain.ops().contains(&"delete"));

    let store = mac(
        Keeps::File,
        File::empty(),
        Keys::empty().failing("count", AUTH_FAILED),
    );
    assert_eq!(store.delete(), Delete::Locked(refused(AUTH_FAILED)));

    let store = mac(
        Keeps::File,
        File::empty(),
        Keys::empty().failing("count", NOT_AVAILABLE),
    );
    assert_eq!(store.delete(), Delete::Fault(fault(NOT_AVAILABLE)));

    for keeps in [Keeps::File, Keeps::Keychain] {
        let store = mac(keeps, File::empty(), Keys::empty());
        assert_eq!(store.delete(), Delete::NotStored, "{keeps:?}");
        assert!(!store.file.ops().contains(&"delete"), "{keeps:?}");
    }

    let store = mac(
        Keeps::Keychain,
        File::holding(FILE),
        Keys::empty().failing("add", NOT_AVAILABLE),
    );
    assert_eq!(
        store.delete(),
        Delete::Locked(moved(i64::from(NOT_AVAILABLE)))
    );
    assert_eq!(store.file.bytes(), Some(FILE.to_vec()));
}

#[test]
fn a_read_never_answers_not_stored_when_another_store_moves_the_file_between_its_looks() {
    let (store, items) = moved_under_the_read(Keeps::Keychain, FILE);
    assert_eq!(found(store.read()), FILE);
    assert_eq!(*items.borrow(), [FILE.to_vec()]);

    let (store, items) = moved_under_the_read(Keeps::File, FILE);
    let read = store.read();
    assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
    assert_eq!(*items.borrow(), [FILE.to_vec()]);
}

struct MovedUnderTheDelete {
    bytes: RefCell<Option<Vec<u8>>>,
    keys: Items,
    removes: bool,
}

impl FileSide for MovedUnderTheDelete {
    fn read(&self) -> Read {
        match self.bytes.borrow().clone() {
            Some(bytes) => Read::Found(Secret::new(bytes).expect("a secret")),
            None => Read::NotStored,
        }
    }

    fn write(&self, _secret: &Secret) -> Write {
        Write::Occupied
    }

    fn delete(&self) -> Delete {
        let held = self.bytes.borrow_mut().take();
        if let Some(bytes) = held {
            self.keys.borrow_mut().push(bytes);
        }
        if self.removes {
            Delete::Deleted
        } else {
            Delete::NotStored
        }
    }

    fn settle(&self, _keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
        Ok(())
    }

    fn remove_if_holds(&self, _bytes: &[u8]) -> Claim {
        Claim::Absent
    }
}

fn moved_under_the_delete(removes: bool) -> (Mac<MovedUnderTheDelete, SharedKeys>, Items) {
    let items: Items = Rc::new(RefCell::new(Vec::new()));
    let store = Mac {
        keeps: Keeps::File,
        file: MovedUnderTheDelete {
            bytes: RefCell::new(Some(FILE.to_vec())),
            keys: Rc::clone(&items),
            removes,
        },
        keychain: KeychainItem {
            keychain: SharedKeys {
                items: Rc::clone(&items),
            },
            service: Name::new("dev.halvo.test").expect("a service"),
            account: Name::new("account").expect("an account"),
        },
    };
    (store, items)
}

#[test]
fn a_delete_that_a_move_overtakes_answers_locked_while_the_keychain_holds_the_credential() {
    let (store, items) = moved_under_the_delete(false);
    assert_eq!(store.delete(), Delete::Locked(Locked::OtherStore));
    assert_eq!(*items.borrow(), [FILE.to_vec()]);
}

#[test]
fn a_delete_whose_removal_took_effect_while_the_keychain_gained_the_credential_answers_a_fault() {
    let (store, items) = moved_under_the_delete(true);
    assert_eq!(store.delete(), Delete::Fault(Fault { code: -1 }));
    assert_eq!(*items.borrow(), [FILE.to_vec()]);
}

#[test]
fn a_delete_of_a_build_with_the_feature_beside_a_file_it_cannot_read_answers_a_fault() {
    for (fails, code) in [
        (Fails::Locked(refused(13)), 13),
        (Fails::Locked(Locked::Ancestor), -1),
        (Fails::Fault(Fault { code: 5 }), 5),
    ] {
        let store = mac(
            Keeps::Keychain,
            File::holding(FILE).failing_read(fails),
            Keys::holding(&[FILE]),
        );
        assert_eq!(store.delete(), Delete::Fault(Fault { code }));
        assert!(store.keychain.keychain.items().is_empty());
        assert_eq!(store.file.bytes(), Some(FILE.to_vec()));
    }
}

#[test]
fn a_delete_answers_from_both_places() {
    let store = mac(Keeps::File, File::holding(FILE), Keys::empty());
    assert_eq!(store.delete(), Delete::Deleted);
    let store = mac(Keeps::Keychain, File::empty(), Keys::holding(&[FILE]));
    assert_eq!(store.delete(), Delete::Deleted);

    for code in [AUTH_FAILED, NOT_AVAILABLE] {
        let store = mac(
            Keeps::File,
            File::holding(FILE),
            Keys::empty().failing_from("count", 2, code),
        );
        assert_eq!(
            store.delete(),
            Delete::Fault(Fault {
                code: i64::from(code)
            })
        );
        assert_eq!(store.file.bytes(), None);
    }

    for keeps in [Keeps::File, Keeps::Keychain] {
        let store = mac(keeps, File::empty(), Keys::empty());
        assert_eq!(store.delete(), Delete::NotStored, "{keeps:?}");
    }

    let store = mac(
        Keeps::File,
        File::holding(FILE).failing_delete(Fails::Absent),
        Keys::empty().failing_from("count", 2, AUTH_FAILED),
    );
    assert_eq!(store.delete(), Delete::Locked(refused(AUTH_FAILED)));
    let store = mac(
        Keeps::File,
        File::holding(FILE).failing_delete(Fails::Absent),
        Keys::empty().failing_from("count", 2, NOT_AVAILABLE),
    );
    assert_eq!(store.delete(), Delete::Fault(fault(NOT_AVAILABLE)));

    let store = mac(
        Keeps::Keychain,
        File::empty(),
        Keys::holding(&[FILE]).failing("delete", ITEM_NOT_FOUND),
    );
    assert_eq!(store.delete(), Delete::NotStored);

    let store = mac(
        Keeps::File,
        File::holding(FILE).failing_delete(Fails::Locked(Locked::Mode)),
        Keys::empty(),
    );
    assert_eq!(store.delete(), Delete::Locked(Locked::Mode));
}

#[test]
fn a_write_stores_only_where_a_read_of_the_same_build_answers_not_stored() {
    let store = mac(Keeps::File, File::empty(), Keys::holding(&[FILE]));
    assert_eq!(
        store.write(&secret(b"new")),
        Write::Locked(Locked::OtherStore)
    );
    assert_eq!(store.file.bytes(), None);
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);

    let store = mac(Keeps::Keychain, File::holding(FILE), Keys::empty());
    assert_eq!(store.write(&secret(b"new")), Write::Occupied);
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
    assert_eq!(store.file.bytes(), None);
    assert_eq!(found(store.read()), FILE);

    let store = mac(
        Keeps::Keychain,
        File::holding(FILE).failing_read(Fails::Locked(refused(13))),
        Keys::empty(),
    );
    assert_eq!(store.write(&secret(b"new")), Write::Locked(refused(13)));
    assert!(store.keychain.keychain.items().is_empty());
    assert_eq!(adds(&store.keychain.keychain), 0);

    let store = mac(
        Keeps::File,
        File::empty(),
        Keys::empty().failing("count", NOT_AVAILABLE),
    );
    assert_eq!(
        store.write(&secret(b"new")),
        Write::Fault(fault(NOT_AVAILABLE))
    );
    assert_eq!(store.file.bytes(), None);
}

#[test]
fn a_write_that_answers_stored_reads_back_what_it_stored() {
    for keeps in [Keeps::File, Keeps::Keychain] {
        let store = mac(keeps, File::empty(), Keys::empty());
        assert_eq!(store.write(&secret(b"new sign-in")), Write::Stored);
        assert_eq!(found(store.read()), b"new sign-in", "{keeps:?}");
    }

    let store = mac(
        Keeps::Keychain,
        File::holding(b"the person's"),
        Keys::empty(),
    );
    assert_eq!(store.write(&secret(b"a new sign-in")), Write::Occupied);
    assert_eq!(found(store.read()), b"the person's");

    let store = mac(
        Keeps::File,
        File::empty(),
        Keys::holding(&[b"the person's"]),
    );
    assert_eq!(
        store.write(&secret(b"a new sign-in")),
        Write::Locked(Locked::OtherStore)
    );
    assert!(matches!(store.read(), Read::Locked(Locked::OtherStore)));
}

#[test]
fn a_removal_that_finds_the_file_gone_answers_what_the_keychain_holds() {
    let store = mac(
        Keeps::Keychain,
        File::holding(FILE).failing_claim(Fails::Absent),
        Keys::empty(),
    );
    assert_eq!(found(store.read()), FILE);
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
    assert_eq!(adds(&store.keychain.keychain), 1);
}

#[test]
fn a_file_left_under_its_private_name_answers_a_fault() {
    let store = mac(
        Keeps::Keychain,
        File::holding(FILE).failing_claim(Fails::Stranded),
        Keys::empty(),
    );
    let read = store.read();
    assert!(matches!(read, Read::Fault(Fault { code: -1 })), "{read:?}");
    assert_eq!(store.keychain.keychain.items(), [FILE.to_vec()]);
}

fn assert_a_failed_settling_is_the_answer(fails: Fails) {
    let (read_answer, write_answer, delete_answer) = match fails {
        Fails::Locked(locked) => (
            Read::Locked(locked),
            Write::Locked(locked),
            Delete::Locked(locked),
        ),
        Fails::Fault(fault) => (
            Read::Fault(fault),
            Write::Fault(fault),
            Delete::Fault(fault),
        ),
        Fails::Absent | Fails::Stranded => unreachable!(),
    };
    for keeps in [Keeps::File, Keeps::Keychain] {
        let store = mac(keeps, File::empty().failing_settle(fails), Keys::empty());
        let read = store.read();
        assert_eq!(format!("{read:?}"), format!("{read_answer:?}"), "{keeps:?}");
        assert_eq!(
            store.write(&secret(b"a new sign-in")),
            write_answer,
            "{keeps:?}"
        );
        assert_eq!(store.delete(), delete_answer, "{keeps:?}");
        assert_eq!(store.file.bytes(), None, "{keeps:?}");
        assert!(store.keychain.keychain.items().is_empty(), "{keeps:?}");
        assert!(
            store.file.ops().is_empty(),
            "{keeps:?}: {:?}",
            store.file.ops()
        );
        assert!(
            !store
                .keychain
                .keychain
                .ops()
                .iter()
                .any(|op| *op == "add" || *op == "delete"),
            "{keeps:?}"
        );
    }
}

#[test]
fn a_settling_that_answers_locked_is_the_answer_of_read_write_and_delete() {
    assert_a_failed_settling_is_the_answer(Fails::Locked(Locked::Mode));
    assert_a_failed_settling_is_the_answer(Fails::Locked(refused(13)));
}

#[test]
fn a_settling_that_answers_a_fault_is_the_answer_of_read_write_and_delete() {
    assert_a_failed_settling_is_the_answer(Fails::Fault(Fault { code: 5 }));
    assert_a_failed_settling_is_the_answer(Fails::Fault(Fault { code: -1 }));
}

#[cfg(target_os = "macos")]
mod real_files {
    use super::*;
    use crate::place::{Item, Plain, Rules};
    use crate::unix::UnixFs;

    type Hook = RefCell<Option<Box<dyn FnOnce()>>>;

    struct BeforeAdd {
        inner: SharedKeys,
        hook: Hook,
    }

    impl Keychain for BeforeAdd {
        fn count(&self, service: &str, account: &str) -> Result<usize, i32> {
            self.inner.count(service, account)
        }

        fn data(&self, service: &str, account: &str) -> Result<Vec<u8>, i32> {
            self.inner.data(service, account)
        }

        fn add(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), i32> {
            if let Some(hook) = self.hook.borrow_mut().take() {
                hook();
            }
            self.inner.add(service, account, bytes)
        }

        fn delete(&self, service: &str, account: &str) -> Result<(), i32> {
            self.inner.delete(service, account)
        }
    }

    fn parent(label: &str) -> std::path::PathBuf {
        let parent =
            std::env::temp_dir().join(format!("halvo-secrets-mac-{}-{label}", std::process::id()));
        std::fs::create_dir_all(&parent).expect("the parent of the store directory");
        parent
    }

    fn file_item(label: &str) -> Item<UnixFs, Plain> {
        Item {
            fs: UnixFs,
            codec: Plain,
            rules: Rules::Private,
            dir: parent(label).join("store"),
            name: Name::new("account").expect("a name"),
        }
    }

    fn keys<K: Keychain>(keychain: K) -> KeychainItem<K> {
        KeychainItem {
            keychain,
            service: Name::new("dev.halvo.test").expect("a service"),
            account: Name::new("account").expect("an account"),
        }
    }

    fn names_in(label: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(parent(label).join("store"))
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

    struct BeforeClaim {
        inner: Item<UnixFs, Plain>,
        hook: Hook,
    }

    impl FileSide for BeforeClaim {
        fn read(&self) -> Read {
            self.inner.read()
        }

        fn write(&self, secret: &Secret) -> Write {
            self.inner.write(secret)
        }

        fn delete(&self) -> Delete {
            self.inner.delete()
        }

        fn settle(&self, keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
            self.inner.settle_claimed(keychain)
        }

        fn remove_if_holds(&self, bytes: &[u8]) -> Claim {
            if let Some(hook) = self.hook.borrow_mut().take() {
                hook();
            }
            self.inner.remove_if_holds(bytes)
        }
    }

    #[test]
    fn a_move_that_another_process_finishes_first_reads_found() {
        let label = "finished";
        let x: &[u8] = b"credential X of an earlier build";
        let items: Items = Rc::new(RefCell::new(vec![x.to_vec()]));
        assert_eq!(file_item(label).write(&secret(x)), Write::Stored);
        let other = Mac {
            keeps: Keeps::Keychain,
            file: file_item(label),
            keychain: keys(SharedKeys {
                items: Rc::clone(&items),
            }),
        };
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            assert_eq!(found(other.read()), b"credential X of an earlier build");
        });
        let first = Mac {
            keeps: Keeps::Keychain,
            file: BeforeClaim {
                inner: file_item(label),
                hook: RefCell::new(Some(hook)),
            },
            keychain: keys(SharedKeys {
                items: Rc::clone(&items),
            }),
        };
        assert_eq!(found(first.read()), x);
        assert!(matches!(file_item(label).read(), Read::NotStored));
        assert_eq!(*items.borrow(), [x.to_vec()]);
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
    }

    #[test]
    fn a_file_that_another_process_replaces_during_the_move_is_kept() {
        let label = "replaced";
        let x: &[u8] = b"credential X of an earlier build";
        let y: &[u8] = b"credential Y of a later sign-in";
        let items: Items = Rc::new(RefCell::new(Vec::new()));
        assert_eq!(file_item(label).write(&secret(x)), Write::Stored);
        let unsigned = Mac {
            keeps: Keeps::File,
            file: file_item(label),
            keychain: keys(SharedKeys {
                items: Rc::clone(&items),
            }),
        };
        let later = y.to_vec();
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            assert_eq!(unsigned.delete(), Delete::Deleted);
            assert_eq!(unsigned.write(&secret(&later)), Write::Stored);
        });
        let signed = Mac {
            keeps: Keeps::Keychain,
            file: file_item(label),
            keychain: keys(BeforeAdd {
                inner: SharedKeys {
                    items: Rc::clone(&items),
                },
                hook: RefCell::new(Some(hook)),
            }),
        };
        let read = signed.read();
        assert!(
            matches!(read, Read::Locked(locked) if locked == moved(-1)),
            "{read:?}"
        );
        assert_eq!(found(file_item(label).read()), y);
        assert_eq!(*items.borrow(), [x.to_vec()]);
        assert_eq!(names_in(label), ["account"]);
    }

    const X: &[u8] = b"credential X of an earlier build";

    fn stopped_after_the_claim(label: &str, bytes: &[u8], pid: u32) {
        assert_eq!(file_item(label).write(&secret(bytes)), Write::Stored);
        let store = parent(label).join("store");
        std::fs::rename(
            store.join("account"),
            store.join(format!(".account.{pid}.0.claim")),
        )
        .expect("the claim of a move that was stopped");
    }

    fn signed(label: &str, keychain: Keys) -> Mac<Item<UnixFs, Plain>, Keys> {
        Mac {
            keeps: Keeps::Keychain,
            file: file_item(label),
            keychain: keys(keychain),
        }
    }

    #[test]
    fn a_move_stopped_after_its_claim_is_ended_by_the_next_operation() {
        let mut other = X.to_vec();
        other[0] ^= 1;
        let none: &[&[u8]] = &[];
        for (held, keychain) in [
            ("same", vec![X]),
            ("other", vec![&other[..]]),
            ("none", none.to_vec()),
        ] {
            for op in ["read", "write", "delete", "move"] {
                let label = format!("claimed-{held}-{op}");
                stopped_after_the_claim(&label, X, 4241);
                let store = signed(&label, Keys::holding(&keychain));
                let answer = match op {
                    "read" | "move" => format!("{:?}", store.read()),
                    "write" => format!("{:?}", store.write(&secret(b"a new sign-in"))),
                    _ => format!("{:?}", store.delete()),
                };
                let names = names_in(&label);
                assert!(
                    !names.iter().any(|name| name.ends_with(".claim")),
                    "{held} {op}: {names:?}"
                );
                let items = store.keychain.keychain.items();
                let (want, file, kept) = match (held, op) {
                    ("same" | "none", "read" | "move") => ("Found(..)", false, vec![X.to_vec()]),
                    ("same" | "none", "write") => ("Occupied", false, vec![X.to_vec()]),
                    ("same" | "none", _) => ("Deleted", false, Vec::new()),
                    ("other", "write") => ("Locked(OtherStore)", true, vec![other.clone()]),
                    ("other", _) => ("Locked(OtherStore)", true, vec![other.clone()]),
                    _ => unreachable!(),
                };
                assert_eq!(answer, want, "{held} {op}");
                assert_eq!(names == ["account"], file, "{held} {op}: {names:?}");
                assert_eq!(items, kept, "{held} {op}");
                if file {
                    assert_eq!(found(file_item(&label).read()), X, "{held} {op}");
                }
            }
        }
    }

    #[test]
    fn a_claimed_file_beside_other_bytes_or_no_item_is_put_back_under_its_name() {
        let mut other = X.to_vec();
        other[0] ^= 1;
        let label = "claimed-other";
        stopped_after_the_claim(label, X, 4241);
        let store = signed(label, Keys::holding(&[&other]));
        let read = store.read();
        assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
        assert_eq!(names_in(label), ["account"]);
        assert_eq!(found(file_item(label).read()), X);

        let label = "claimed-unsigned";
        stopped_after_the_claim(label, X, 4241);
        let unsigned = Mac {
            keeps: Keeps::File,
            file: file_item(label),
            keychain: keys(Keys::empty()),
        };
        assert_eq!(found(unsigned.read()), X);
        assert_eq!(names_in(label), ["account"]);
    }

    #[test]
    fn two_claimed_files_are_settled_one_by_one_and_other_names_stay() {
        let label = "claimed-two";
        let y = b"credential Y of a later sign-in.";
        stopped_after_the_claim(label, X, 4241);
        stopped_after_the_claim(label, y, 4242);
        let store_dir = parent(label).join("store");
        std::fs::write(
            store_dir.join(".account.4243.0.tmp"),
            b"a write that was stopped",
        )
        .expect("a temporary file of a write");
        std::fs::write(store_dir.join("notes"), b"not of the store").expect("another file");
        let store = signed(label, Keys::holding(&[X]));
        let read = store.read();
        assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
        assert_eq!(names_in(label), [".account.4243.0.tmp", "account", "notes"]);
        assert_eq!(found(file_item(label).read()), y);
        assert_eq!(store.keychain.keychain.items(), [X.to_vec()]);
    }

    #[test]
    fn a_signed_delete_beside_a_claim_that_cannot_be_read_never_answers_deleted() {
        use std::os::unix::fs::PermissionsExt;
        let label = "unreadable-claim-delete";
        stopped_after_the_claim(label, X, 4241);
        let claim = parent(label).join("store").join(".account.4241.0.claim");
        std::fs::set_permissions(&claim, std::fs::Permissions::from_mode(0o644))
            .expect("the mode of the claim");
        let store = signed(label, Keys::holding(&[X]));
        assert_eq!(store.delete(), Delete::Fault(Fault { code: -1 }));
        assert!(store.keychain.keychain.items().is_empty());
        assert_eq!(names_in(label), [".account.4241.0.claim"]);

        let label = "readable-claim-same-delete";
        stopped_after_the_claim(label, X, 4241);
        let store = signed(label, Keys::holding(&[X]));
        assert_eq!(store.delete(), Delete::Deleted);
        assert!(store.keychain.keychain.items().is_empty());
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));

        let label = "readable-claim-other-delete";
        let mut other = X.to_vec();
        other[0] ^= 1;
        stopped_after_the_claim(label, X, 4241);
        let store = signed(label, Keys::holding(&[&other]));
        assert_eq!(store.delete(), Delete::Locked(Locked::OtherStore));
        assert_eq!(store.keychain.keychain.items(), [other]);
        assert_eq!(names_in(label), ["account"]);
        assert_eq!(found(file_item(label).read()), X);
    }
}
