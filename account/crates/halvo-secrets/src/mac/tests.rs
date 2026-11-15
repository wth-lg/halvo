use super::*;
use crate::answer::{Fault, Locked};
use crate::keychain::{ItemRef, AUTH_FAILED, DUPLICATE_ITEM, ITEM_NOT_FOUND};
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

    fn find(&self, service: &str, account: &str) -> Result<(Vec<u8>, ItemRef), i32> {
        Ok((self.data(service, account)?, ItemRef::Fake(0)))
    }

    fn delete_item(&self, _item: ItemRef) {
        if !self.items.borrow().is_empty() {
            self.items.borrow_mut().remove(0);
        }
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

    fn remove_at_word(&self, keychain: Option<&[u8]>) -> Delete {
        let _ = keychain;
        self.delete()
    }

    fn delete(&self) -> Delete {
        Delete::NotStored
    }

    fn delete_if_holds(&self, _held: &[u8]) -> DeleteIfHolds {
        DeleteIfHolds::NotStored
    }

    fn settle(&self, _keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
        Ok(())
    }

    fn claimed_or_named(&self) -> Result<bool, Result<Locked, Fault>> {
        Ok(false)
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
    after_read: RefCell<Option<Vec<u8>>>,
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
            after_read: RefCell::new(None),
            log: RefCell::new(Vec::new()),
        }
    }

    fn replaced_after_its_read(self, with: &[u8]) -> File {
        *self.after_read.borrow_mut() = Some(with.to_vec());
        self
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
        let read = match self.bytes() {
            Some(bytes) => Read::Found(Secret::new(bytes).expect("a secret")),
            None => Read::NotStored,
        };
        if let Some(with) = self.after_read.borrow_mut().take() {
            *self.bytes.borrow_mut() = Some(with);
        }
        read
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

    fn remove_at_word(&self, keychain: Option<&[u8]>) -> Delete {
        match self.settle(keychain) {
            Ok(()) => self.delete(),
            Err(Ok(locked)) => Delete::Locked(locked),
            Err(Err(fault)) => Delete::Fault(fault),
        }
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

    fn delete_if_holds(&self, held: &[u8]) -> DeleteIfHolds {
        self.log.borrow_mut().push("delete_if_holds");
        match self.delete_fails {
            Some(Fails::Locked(locked)) => return DeleteIfHolds::Locked(locked),
            Some(Fails::Fault(fault)) => return DeleteIfHolds::Fault(fault),
            Some(Fails::Absent) => return DeleteIfHolds::NotStored,
            Some(Fails::Stranded) => return DeleteIfHolds::Fault(Fault { code: -1 }),
            None => {}
        }
        let mut bytes = self.bytes.borrow_mut();
        match bytes.as_deref() {
            Some(stored) if stored == held => {
                *bytes = None;
                DeleteIfHolds::Deleted
            }
            Some(stored) => DeleteIfHolds::Other(secret(stored)),
            None => DeleteIfHolds::NotStored,
        }
    }

    fn settle(&self, _keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
        match self.settle_fails {
            Some(Fails::Locked(locked)) => Err(Ok(locked)),
            Some(Fails::Fault(fault)) => Err(Err(fault)),
            _ => Ok(()),
        }
    }

    fn claimed_or_named(&self) -> Result<bool, Result<Locked, Fault>> {
        Ok(false)
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
    after_data: RefCell<Option<Vec<u8>>>,
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
            after_data: RefCell::new(None),
            log: RefCell::new(Vec::new()),
        }
    }

    fn replaced_after_its_read(self, with: &[u8]) -> Keys {
        *self.after_data.borrow_mut() = Some(with.to_vec());
        self
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
        if let Some(with) = self.after_data.borrow_mut().take() {
            *self.items.borrow_mut() = vec![with];
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

    fn find(&self, _service: &str, _account: &str) -> Result<(Vec<u8>, ItemRef), i32> {
        self.check("find")?;
        let bytes = self.items.borrow().first().cloned().ok_or(ITEM_NOT_FOUND)?;
        Ok((bytes, ItemRef::Fake(0)))
    }

    fn delete_item(&self, _item: ItemRef) {
        self.log.borrow_mut().push("delete_item");
        if !self.items.borrow().is_empty() {
            self.items.borrow_mut().remove(0);
        }
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

    fn remove_at_word(&self, keychain: Option<&[u8]>) -> Delete {
        let _ = keychain;
        self.delete()
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

    fn delete_if_holds(&self, _held: &[u8]) -> DeleteIfHolds {
        self.delete().into()
    }

    fn settle(&self, _keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
        Ok(())
    }

    fn claimed_or_named(&self) -> Result<bool, Result<Locked, Fault>> {
        Ok(false)
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

const HELD: &[u8] = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn held_one_byte_off(at: usize) -> Vec<u8> {
    let mut other = HELD.to_vec();
    other[at] ^= 1;
    other
}

fn other_bytes(answer: DeleteIfHolds) -> Vec<u8> {
    match answer {
        DeleteIfHolds::Other(secret) => secret.expose().to_vec(),
        answer => panic!("the delete answered {answer:?}"),
    }
}

fn shown(answer: DeleteIfHolds) -> String {
    format!("{answer:?}")
}

#[test]
fn a_delete_if_holds_removes_the_bytes_read_from_the_store_that_the_build_keeps() {
    let store = mac(Keeps::File, File::holding(HELD), Keys::empty());
    assert_eq!(shown(store.delete_if_holds(HELD)), "Deleted");
    assert_eq!(store.file.bytes(), None);
    assert!(!store.keychain.keychain.ops().contains(&"delete_item"));

    let store = mac(Keeps::Keychain, File::empty(), Keys::holding(&[HELD]));
    assert_eq!(shown(store.delete_if_holds(HELD)), "Deleted");
    assert!(store.keychain.keychain.items().is_empty());
    assert!(!store.file.ops().contains(&"delete_if_holds"));

    for keeps in [Keeps::File, Keeps::Keychain] {
        let store = mac(keeps, File::empty(), Keys::empty());
        assert_eq!(shown(store.delete_if_holds(HELD)), "NotStored", "{keeps:?}");
    }
}

#[test]
fn a_delete_if_holds_keeps_other_bytes_of_the_same_length_and_gives_them() {
    for at in [0, HELD.len() - 1] {
        let other = held_one_byte_off(at);
        let store = mac(Keeps::File, File::holding(&other), Keys::empty());
        assert_eq!(other_bytes(store.delete_if_holds(HELD)), other);
        assert_eq!(store.file.bytes(), Some(other.clone()));
        assert!(!store.file.ops().contains(&"delete_if_holds"));

        let store = mac(Keeps::Keychain, File::empty(), Keys::holding(&[&other]));
        assert_eq!(other_bytes(store.delete_if_holds(HELD)), other);
        assert_eq!(
            store.keychain.keychain.items(),
            std::slice::from_ref(&other)
        );
        assert!(!store.keychain.keychain.ops().contains(&"delete_item"));
    }
}

#[test]
fn a_delete_if_holds_never_removes_what_another_process_put_in_place_after_its_read() {
    let later = held_one_byte_off(9);
    let store = mac(
        Keeps::File,
        File::holding(HELD).replaced_after_its_read(&later),
        Keys::empty(),
    );
    assert_eq!(other_bytes(store.delete_if_holds(HELD)), later);
    assert_eq!(store.file.bytes(), Some(later.clone()));
    assert!(!store.file.ops().contains(&"delete"));

    let store = mac(
        Keeps::Keychain,
        File::empty(),
        Keys::holding(&[HELD]).replaced_after_its_read(&later),
    );
    assert_eq!(other_bytes(store.delete_if_holds(HELD)), later);
    assert_eq!(
        store.keychain.keychain.items(),
        std::slice::from_ref(&later)
    );
    assert!(!store.keychain.keychain.ops().contains(&"delete"));
}

#[test]
fn a_delete_if_holds_answers_from_both_places() {
    let (store, items) = moved_under_the_delete(true);
    assert_eq!(
        shown(store.delete_if_holds(FILE)),
        "Fault(Fault { code: -1 })"
    );
    assert_eq!(*items.borrow(), [FILE.to_vec()]);
    let (store, items) = moved_under_the_delete(false);
    assert_eq!(shown(store.delete_if_holds(FILE)), "Locked(OtherStore)");
    assert_eq!(*items.borrow(), [FILE.to_vec()]);

    for (code, answer) in [
        (AUTH_FAILED, "Fault(Fault { code: -25293 })"),
        (NOT_AVAILABLE, "Fault(Fault { code: -25291 })"),
    ] {
        let store = mac(
            Keeps::File,
            File::holding(HELD),
            Keys::empty().failing_from("count", 2, code),
        );
        assert_eq!(shown(store.delete_if_holds(HELD)), answer);
        assert_eq!(store.file.bytes(), None);
    }
    for (code, answer) in [
        (AUTH_FAILED, "Locked(Refused { code: -25293 })"),
        (NOT_AVAILABLE, "Fault(Fault { code: -25291 })"),
    ] {
        let store = mac(
            Keeps::File,
            File::empty(),
            Keys::empty().failing_from("count", 3, code),
        );
        assert_eq!(shown(store.delete_if_holds(HELD)), answer);
    }
    for (fails, answer) in [
        (Fails::Locked(refused(13)), "Fault(Fault { code: 13 })"),
        (Fails::Locked(Locked::Ancestor), "Fault(Fault { code: -1 })"),
        (Fails::Fault(Fault { code: 5 }), "Fault(Fault { code: 5 })"),
    ] {
        let store = mac(
            Keeps::Keychain,
            File::holding(HELD).failing_read(fails),
            Keys::holding(&[HELD]),
        );
        assert_eq!(shown(store.delete_if_holds(HELD)), answer);
        assert!(store.keychain.keychain.items().is_empty());
        assert_eq!(store.file.bytes(), Some(HELD.to_vec()));
    }
}

#[test]
fn a_delete_if_holds_passes_on_the_answer_of_its_read_and_of_its_own_step() {
    let other = held_one_byte_off(31);
    for keeps in [Keeps::File, Keeps::Keychain] {
        let store = mac(keeps, File::holding(HELD), Keys::holding(&[&other]));
        assert_eq!(shown(store.delete_if_holds(HELD)), "Locked(OtherStore)");
        assert_eq!(store.file.bytes(), Some(HELD.to_vec()));
        assert_eq!(
            store.keychain.keychain.items(),
            std::slice::from_ref(&other)
        );
    }
    let store = mac(
        Keeps::File,
        File::holding(HELD).failing_read(Fails::Fault(Fault { code: 5 })),
        Keys::empty(),
    );
    assert_eq!(
        shown(store.delete_if_holds(HELD)),
        "Fault(Fault { code: 5 })"
    );
    for (fails, answer) in [
        (Fails::Locked(refused(13)), "Locked(Refused { code: 13 })"),
        (Fails::Fault(Fault { code: 5 }), "Fault(Fault { code: 5 })"),
    ] {
        let store = mac(
            Keeps::File,
            File::holding(HELD).failing_delete(fails),
            Keys::empty(),
        );
        assert_eq!(shown(store.delete_if_holds(HELD)), answer);
        assert_eq!(store.file.bytes(), Some(HELD.to_vec()));
    }
    let store = mac(
        Keeps::Keychain,
        File::empty(),
        Keys::holding(&[HELD]).failing("find", AUTH_FAILED),
    );
    assert_eq!(
        shown(store.delete_if_holds(HELD)),
        "Locked(Refused { code: -25293 })"
    );
    assert_eq!(store.keychain.keychain.items(), [HELD.to_vec()]);
}

#[test]
fn a_removal_at_the_person_s_word_leaves_the_state_of_other_bytes_in_both_stores() {
    let other = held_one_byte_off(0);
    let both = |keeps| mac(keeps, File::holding(HELD), Keys::holding(&[&other]));

    let store = both(Keeps::File);
    assert_eq!(store.remove(StoreKind::Keychain), Some(Delete::Deleted));
    assert!(store.keychain.keychain.items().is_empty());
    assert_eq!(found(store.read()), HELD);

    let store = both(Keeps::File);
    assert_eq!(store.remove(StoreKind::File), Some(Delete::Deleted));
    assert_eq!(store.file.bytes(), None);
    assert_eq!(
        store.keychain.keychain.items(),
        std::slice::from_ref(&other)
    );
    let read = store.read();
    assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");

    let store = both(Keeps::Keychain);
    assert_eq!(store.remove(StoreKind::File), Some(Delete::Deleted));
    assert_eq!(store.file.bytes(), None);
    assert_eq!(found(store.read()), other);

    let store = both(Keeps::Keychain);
    assert_eq!(store.remove(StoreKind::Keychain), Some(Delete::Deleted));
    assert_eq!(store.file.bytes(), Some(HELD.to_vec()));
    assert_eq!(found(store.read()), HELD);
    assert_eq!(store.file.bytes(), None);
    assert_eq!(store.keychain.keychain.items(), [HELD.to_vec()]);
}

#[test]
fn a_removal_touches_the_named_store_alone() {
    for keeps in [Keeps::File, Keeps::Keychain] {
        let store = mac(keeps, File::holding(HELD), Keys::holding(&[HELD]));
        assert_eq!(store.remove(StoreKind::Dpapi), None);
        assert!(store.file.ops().is_empty());
        assert!(store.keychain.keychain.ops().is_empty());

        let store = mac(keeps, File::holding(HELD), Keys::empty());
        assert_eq!(store.remove(StoreKind::Keychain), Some(Delete::NotStored));
        assert_eq!(store.file.bytes(), Some(HELD.to_vec()));
        assert!(!store.file.ops().contains(&"delete"));

        let store = mac(keeps, File::empty(), Keys::holding(&[HELD]));
        assert_eq!(store.remove(StoreKind::File), Some(Delete::NotStored));
        assert_eq!(store.keychain.keychain.items(), [HELD.to_vec()]);
        assert!(!store.keychain.keychain.ops().contains(&"delete"));
    }
}

#[test]
fn a_removal_answers_the_failure_of_the_named_store_and_keeps_its_item() {
    for (fails, answer) in [
        (Fails::Locked(refused(13)), Delete::Locked(refused(13))),
        (
            Fails::Fault(Fault { code: 5 }),
            Delete::Fault(Fault { code: 5 }),
        ),
    ] {
        let store = mac(
            Keeps::Keychain,
            File::holding(HELD).failing_settle(fails),
            Keys::empty(),
        );
        assert_eq!(store.remove(StoreKind::File), Some(answer));
        assert_eq!(store.file.bytes(), Some(HELD.to_vec()));
        assert!(!store.file.ops().contains(&"delete"));

        let store = mac(
            Keeps::File,
            File::holding(HELD).failing_delete(fails),
            Keys::empty(),
        );
        assert_eq!(store.remove(StoreKind::File), Some(answer));
        assert_eq!(store.file.bytes(), Some(HELD.to_vec()));
    }
    for (code, answer) in [
        (AUTH_FAILED, Delete::Locked(refused(AUTH_FAILED))),
        (NOT_AVAILABLE, Delete::Fault(fault(NOT_AVAILABLE))),
    ] {
        let store = mac(
            Keeps::File,
            File::empty(),
            Keys::holding(&[HELD]).failing("delete", code),
        );
        assert_eq!(store.remove(StoreKind::Keychain), Some(answer));
        assert_eq!(store.keychain.keychain.items(), [HELD.to_vec()]);
    }
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

        fn find(&self, service: &str, account: &str) -> Result<(Vec<u8>, ItemRef), i32> {
            self.inner.find(service, account)
        }

        fn delete_item(&self, item: ItemRef) {
            self.inner.delete_item(item)
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

        fn remove_at_word(&self, keychain: Option<&[u8]>) -> Delete {
            self.inner.remove_at_word(keychain)
        }

        fn delete(&self) -> Delete {
            self.inner.delete()
        }

        fn delete_if_holds(&self, held: &[u8]) -> DeleteIfHolds {
            self.inner.delete_if_holds(held)
        }

        fn settle(&self, keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
            self.inner.settle_claimed(keychain)
        }

        fn claimed_or_named(&self) -> Result<bool, Result<Locked, Fault>> {
            self.inner.claimed_or_named()
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
        let store = parent(label).join("store");
        if store.exists() {
            // A write beside a claim answers a fault, so the next file is made by hand.
            use std::io::Write as _;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(store.join("account"))
                .and_then(|mut file| file.write_all(bytes))
                .expect("the file of a later write");
        } else {
            assert_eq!(file_item(label).write(&secret(bytes)), Write::Stored);
        }
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
        assert_eq!(store.delete(), Delete::Locked(Locked::Mode));
        assert_eq!(store.keychain.keychain.items(), [X.to_vec()]);
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

    #[test]
    fn a_file_that_another_process_replaces_with_bytes_of_the_same_length_during_the_move_is_kept()
    {
        let label = "replaced-same-length";
        let y: &[u8] = b"credential Y of an earlier build";
        assert_eq!((X.len(), y.len()), (32, 32));
        assert_eq!(X.iter().zip(y).filter(|(a, b)| a != b).count(), 1);
        let items: Items = Rc::new(RefCell::new(Vec::new()));
        assert_eq!(file_item(label).write(&secret(X)), Write::Stored);
        let later = y.to_vec();
        let hook_label = label.to_string();
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            assert_eq!(file_item(&hook_label).delete(), Delete::Deleted);
            assert_eq!(file_item(&hook_label).write(&secret(&later)), Write::Stored);
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
        assert_eq!(*items.borrow(), [X.to_vec()]);
        assert_eq!(names_in(label), ["account"]);
    }

    struct AfterRead {
        inner: Item<UnixFs, Plain>,
        hook: Hook,
    }

    impl FileSide for AfterRead {
        fn read(&self) -> Read {
            let read = self.inner.read();
            if let Some(hook) = self.hook.borrow_mut().take() {
                hook();
            }
            read
        }

        fn write(&self, secret: &Secret) -> Write {
            self.inner.write(secret)
        }

        fn remove_at_word(&self, keychain: Option<&[u8]>) -> Delete {
            self.inner.remove_at_word(keychain)
        }

        fn delete(&self) -> Delete {
            self.inner.delete()
        }

        fn delete_if_holds(&self, held: &[u8]) -> DeleteIfHolds {
            self.inner.delete_if_holds(held)
        }

        fn settle(&self, keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
            self.inner.settle_claimed(keychain)
        }

        fn claimed_or_named(&self) -> Result<bool, Result<Locked, Fault>> {
            self.inner.claimed_or_named()
        }

        fn remove_if_holds(&self, bytes: &[u8]) -> Claim {
            self.inner.remove_if_holds(bytes)
        }
    }

    #[test]
    fn an_unsigned_delete_if_holds_keeps_a_file_that_another_process_wrote_after_its_read() {
        let label = "replaced-after-the-read";
        let y: &[u8] = b"credential Y of an earlier build";
        assert_eq!(file_item(label).write(&secret(X)), Write::Stored);
        let hook_label = label.to_string();
        let later = y.to_vec();
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            assert_eq!(file_item(&hook_label).delete(), Delete::Deleted);
            assert_eq!(file_item(&hook_label).write(&secret(&later)), Write::Stored);
        });
        let unsigned = Mac {
            keeps: Keeps::File,
            file: AfterRead {
                inner: file_item(label),
                hook: RefCell::new(Some(hook)),
            },
            keychain: keys(Keys::empty()),
        };
        assert_eq!(other_bytes(unsigned.delete_if_holds(X)), y);
        assert_eq!(found(file_item(label).read()), y);
        assert_eq!(names_in(label), ["account"]);
        assert_eq!(shown(unsigned.delete_if_holds(y)), "Deleted");
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
    }

    #[test]
    fn a_delete_if_holds_ends_a_claim_that_a_killed_move_left() {
        let label = "claim-delete-if-holds";
        stopped_after_the_claim(label, X, 4243);
        let store = signed(label, Keys::holding(&[X]));
        assert_eq!(shown(store.delete_if_holds(X)), "Deleted");
        assert!(store.keychain.keychain.items().is_empty());
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
    }

    #[test]
    fn a_removal_of_the_file_ends_a_claim_that_a_killed_move_left_first() {
        let label = "claim-removal-same";
        stopped_after_the_claim(label, X, 4243);
        let store = signed(label, Keys::holding(&[X]));
        assert_eq!(store.remove(StoreKind::File), Some(Delete::NotStored));
        assert_eq!(store.keychain.keychain.items(), [X.to_vec()]);
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));

        let label = "claim-removal-other";
        let mut other = X.to_vec();
        other[5] ^= 1;
        stopped_after_the_claim(label, X, 4243);
        let store = signed(label, Keys::holding(&[&other]));
        assert_eq!(store.remove(StoreKind::File), Some(Delete::Deleted));
        assert_eq!(store.keychain.keychain.items(), [other.clone()]);
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
        assert_eq!(found(store.read()), other);
    }

    #[test]
    fn a_removal_of_the_keychain_item_ends_the_claim_of_a_killed_move_of_the_same_bytes() {
        let label = "claim-removal-keychain";
        stopped_after_the_claim(label, X, 4243);
        let store = signed(label, Keys::holding(&[X]));
        assert_eq!(store.remove(StoreKind::Keychain), Some(Delete::Deleted));
        assert!(store.keychain.keychain.items().is_empty());
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
        assert!(matches!(store.read(), Read::NotStored));
    }

    struct ClaimedAfterTheSettle {
        inner: Item<UnixFs, Plain>,
        hooks: RefCell<Vec<Box<dyn FnOnce()>>>,
    }

    impl FileSide for ClaimedAfterTheSettle {
        fn read(&self) -> Read {
            self.inner.read()
        }

        fn write(&self, secret: &Secret) -> Write {
            self.inner.write(secret)
        }

        fn remove_at_word(&self, keychain: Option<&[u8]>) -> Delete {
            self.inner.remove_at_word(keychain)
        }

        fn delete(&self) -> Delete {
            self.inner.delete()
        }

        fn delete_if_holds(&self, held: &[u8]) -> DeleteIfHolds {
            self.inner.delete_if_holds(held)
        }

        fn settle(&self, keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
            let settled = self.inner.settle_claimed(keychain);
            let next = {
                let mut hooks = self.hooks.borrow_mut();
                (!hooks.is_empty()).then(|| hooks.remove(0))
            };
            if let Some(hook) = next {
                hook();
            }
            settled
        }

        fn claimed_or_named(&self) -> Result<bool, Result<Locked, Fault>> {
            self.inner.claimed_or_named()
        }

        fn remove_if_holds(&self, bytes: &[u8]) -> Claim {
            self.inner.remove_if_holds(bytes)
        }
    }

    fn claim_of_another_process(label: &str) -> (Box<dyn FnOnce()>, std::path::PathBuf) {
        let store = parent(label).join("store");
        let claim = store.join(format!(".account.{}.0.claim", std::process::id() + 1));
        let (from, to) = (store.join("account"), claim.clone());
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            std::fs::rename(&from, &to).expect("the claim of another process");
        });
        (hook, claim)
    }

    fn named_back(label: &str, claim: &std::path::Path) {
        if claim.exists() {
            std::fs::rename(claim, parent(label).join("store").join("account"))
                .expect("the file named back");
        }
    }

    fn stands(label: &str, keychain: &Items, bytes: &[u8]) -> bool {
        let in_file =
            matches!(file_item(label).read(), Read::Found(held) if held.expose() == bytes);
        in_file || keychain.borrow().iter().any(|item| item == bytes)
    }

    #[test]
    fn a_delete_if_holds_during_another_process_claim_never_answers_not_stored() {
        for keeps in [Keeps::File, Keeps::Keychain] {
            let label = match keeps {
                Keeps::File => "unsigned-delete-if-holds-during-a-claim",
                Keeps::Keychain => "signed-delete-if-holds-during-a-claim",
            };
            assert_eq!(file_item(label).write(&secret(X)), Write::Stored);
            let (hook, claim) = claim_of_another_process(label);
            let items: Items = Rc::new(RefCell::new(Vec::new()));
            let this = Mac {
                keeps,
                file: ClaimedAfterTheSettle {
                    inner: file_item(label),
                    hooks: RefCell::new(vec![hook]),
                },
                keychain: keys(SharedKeys {
                    items: Rc::clone(&items),
                }),
            };
            let answer = this.delete_if_holds(X);
            named_back(label, &claim);
            let stands = stands(label, &items, X);
            match answer {
                DeleteIfHolds::Deleted => assert!(!stands, "{keeps:?}: Deleted while X stands"),
                DeleteIfHolds::NotStored => panic!("{keeps:?}: NotStored while X stood"),
                answer => assert!(stands, "{keeps:?}: {answer:?} and X is gone"),
            }
        }
    }

    #[test]
    fn a_read_or_a_delete_during_another_process_claim_never_answers_not_stored() {
        for (keeps, op) in [
            (Keeps::File, "read"),
            (Keeps::Keychain, "read"),
            (Keeps::File, "delete"),
            (Keeps::Keychain, "delete"),
        ] {
            let label = format!("{op}-{keeps:?}-during-a-claim").to_lowercase();
            assert_eq!(file_item(&label).write(&secret(X)), Write::Stored);
            let (hook, claim) = claim_of_another_process(&label);
            let items: Items = Rc::new(RefCell::new(Vec::new()));
            let this = Mac {
                keeps,
                file: ClaimedAfterTheSettle {
                    inner: file_item(&label),
                    hooks: RefCell::new(vec![hook]),
                },
                keychain: keys(SharedKeys {
                    items: Rc::clone(&items),
                }),
            };
            let answer = match op {
                "read" => format!("{:?}", this.read()),
                _ => format!("{:?}", this.delete()),
            };
            named_back(&label, &claim);
            let stands = stands(&label, &items, X);
            assert_ne!(answer, "NotStored", "{keeps:?} {op}");
            assert_eq!(answer == "Deleted", !stands, "{keeps:?} {op}: {answer}");
        }
    }

    #[test]
    fn an_unsigned_delete_whose_file_is_claimed_after_its_read_answers_a_fault() {
        let label = "unsigned-delete-claimed-after-the-read";
        assert_eq!(file_item(label).write(&secret(X)), Write::Stored);
        let (hook, claim) = claim_of_another_process(label);
        let unsigned = Mac {
            keeps: Keeps::File,
            file: AfterRead {
                inner: file_item(label),
                hook: RefCell::new(Some(hook)),
            },
            keychain: keys(Keys::empty()),
        };
        let answer = unsigned.delete();
        named_back(label, &claim);
        let stands = matches!(file_item(label).read(), Read::Found(held) if held.expose() == X);
        match answer {
            Delete::Deleted => assert!(!stands, "Deleted while X stands"),
            Delete::Fault(Fault { code: -1 }) => assert!(stands, "Fault and X is gone"),
            answer => panic!("the delete answered {answer:?}"),
        }
    }

    #[test]
    fn a_read_that_finds_the_file_claimed_twice_by_another_process_answers_a_fault() {
        for keeps in [Keeps::File, Keeps::Keychain] {
            let label = format!("read-{keeps:?}-claimed-twice").to_lowercase();
            assert_eq!(file_item(&label).write(&secret(X)), Write::Stored);
            let (first, claim) = claim_of_another_process(&label);
            let (second, _) = claim_of_another_process(&label);
            let this = Mac {
                keeps,
                file: ClaimedAfterTheSettle {
                    inner: file_item(&label),
                    hooks: RefCell::new(vec![first, second]),
                },
                keychain: keys(Keys::empty()),
            };
            let read = this.read();
            named_back(&label, &claim);
            assert!(
                matches!(read, Read::Fault(Fault { code: -1 })),
                "{keeps:?}: {read:?}"
            );
            assert_eq!(found(file_item(&label).read()), X, "{keeps:?}");
        }
    }

    use crate::place::{Fs, Meta, Opened, Published, Walk};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum At {
        Claim,
        BeforeClaim,
        OpenName,
        RemoveName,
        OpenClaim,
        MetaOfClaim,
        RenameBack,
        RemoveClaim,
        Exists,
        RemoveTemp,
        Names,
        Walk,
        ReadClaim,
    }

    type Hooks = Vec<(At, Box<dyn FnOnce()>)>;

    struct HookedFs {
        hooks: RefCell<Hooks>,
        opened: RefCell<String>,
        faults: RefCell<Vec<(At, i32)>>,
    }

    impl HookedFs {
        /// Runs the first hook left for `at`, once.
        fn fire(&self, at: At) {
            let next = {
                let mut hooks = self.hooks.borrow_mut();
                hooks
                    .iter()
                    .position(|(when, _)| *when == at)
                    .map(|first| hooks.remove(first).1)
            };
            if let Some(hook) = next {
                hook();
            }
        }

        /// The first fault left for `at`, once: the call fails with that errno.
        fn fault(&self, at: At) -> std::io::Result<()> {
            let mut faults = self.faults.borrow_mut();
            match faults.iter().position(|(when, _)| *when == at) {
                Some(first) => Err(std::io::Error::from_raw_os_error(faults.remove(first).1)),
                None => Ok(()),
            }
        }
    }

    impl Fs for HookedFs {
        type Dir = <UnixFs as Fs>::Dir;
        type Item = <UnixFs as Fs>::Item;
        type Temp = <UnixFs as Fs>::Temp;

        fn user(&self) -> u32 {
            Fs::user(&UnixFs)
        }

        fn walk(&self, path: &std::path::Path) -> std::io::Result<Walk<Self::Dir>> {
            self.fault(At::Walk)?;
            Fs::walk(&UnixFs, path)
        }

        fn make_dir(&self, parent: &Self::Dir, name: &std::ffi::OsStr) -> std::io::Result<()> {
            Fs::make_dir(&UnixFs, parent, name)
        }

        fn is_absent(&self, error: &std::io::Error) -> bool {
            Fs::is_absent(&UnixFs, error)
        }

        fn exists(&self, dir: &Self::Dir, name: &str) -> std::io::Result<bool> {
            self.fire(At::Exists);
            Fs::exists(&UnixFs, dir, name)
        }

        fn open_item(&self, dir: &Self::Dir, name: &str) -> std::io::Result<Opened<Self::Item>> {
            if name == "account" {
                self.fire(At::OpenName);
            } else if name.ends_with(".claim") {
                self.fire(At::OpenClaim);
            }
            *self.opened.borrow_mut() = name.to_string();
            Fs::open_item(&UnixFs, dir, name)
        }

        fn item_meta(&self, item: &Self::Item) -> std::io::Result<Meta> {
            if self.opened.borrow().ends_with(".claim") {
                self.fire(At::MetaOfClaim);
            }
            Fs::item_meta(&UnixFs, item)
        }

        fn read_item(&self, item: &mut Self::Item, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.opened.borrow().ends_with(".claim") {
                self.fault(At::ReadClaim)?;
            }
            Fs::read_item(&UnixFs, item, buf)
        }

        fn create_temp(&self, dir: &Self::Dir, name: &str) -> std::io::Result<Option<Self::Temp>> {
            Fs::create_temp(&UnixFs, dir, name)
        }

        fn write_temp(&self, temp: &mut Self::Temp, bytes: &[u8]) -> std::io::Result<()> {
            Fs::write_temp(&UnixFs, temp, bytes)
        }

        fn sync_temp(&self, temp: &Self::Temp) -> std::io::Result<()> {
            Fs::sync_temp(&UnixFs, temp)
        }

        fn publish(&self, dir: &Self::Dir, from: &str, to: &str) -> std::io::Result<Published> {
            if from.ends_with(".claim") && to == "account" {
                self.fire(At::RenameBack);
                self.fault(At::RenameBack)?;
            }
            if from == "account" && to.ends_with(".claim") {
                self.fire(At::BeforeClaim);
            }
            let published = Fs::publish(&UnixFs, dir, from, to)?;
            if matches!(published, Published::Done) && to.ends_with(".claim") {
                self.fire(At::Claim);
            }
            Ok(published)
        }

        fn remove(&self, dir: &Self::Dir, name: &str) -> std::io::Result<()> {
            if name == "account" {
                self.fire(At::RemoveName);
            } else if name.ends_with(".claim") {
                self.fire(At::RemoveClaim);
            } else if name.ends_with(".tmp") {
                self.fire(At::RemoveTemp);
            }
            Fs::remove(&UnixFs, dir, name)
        }

        fn sync_dir(&self, dir: &Self::Dir) -> std::io::Result<()> {
            Fs::sync_dir(&UnixFs, dir)
        }

        fn names(&self, dir: &Self::Dir) -> std::io::Result<Vec<String>> {
            self.fire(At::Names);
            self.fault(At::Names)?;
            Fs::names(&UnixFs, dir)
        }
    }

    fn hooked_item(label: &str, at: At, hook: Box<dyn FnOnce()>) -> Item<HookedFs, Plain> {
        hooked_items(label, at, vec![hook])
    }

    fn hooked_items(label: &str, at: At, hooks: Vec<Box<dyn FnOnce()>>) -> Item<HookedFs, Plain> {
        hooked_at(label, hooks.into_iter().map(|hook| (at, hook)).collect())
    }

    fn hooked_at(label: &str, hooks: Hooks) -> Item<HookedFs, Plain> {
        hooked_and_faulted(label, hooks, Vec::new())
    }

    fn hooked_and_faulted(
        label: &str,
        hooks: Hooks,
        faults: Vec<(At, i32)>,
    ) -> Item<HookedFs, Plain> {
        Item {
            fs: HookedFs {
                hooks: RefCell::new(hooks),
                opened: RefCell::new(String::new()),
                faults: RefCell::new(faults),
            },
            codec: Plain,
            rules: Rules::Private,
            dir: parent(label).join("store"),
            name: Name::new("account").expect("a name"),
        }
    }

    fn signed_delete_if_holds_beside(label: &str, items: &Items) -> Box<dyn FnOnce()> {
        let other_items = Rc::clone(items);
        let label = label.to_string();
        Box::new(move || {
            let keychain = keys(SharedKeys { items: other_items });
            assert!(matches!(
                keychain.delete_if_holds(X),
                DeleteIfHolds::Deleted
            ));
            assert_eq!(file_item(&label).settle_claimed(None), Ok(()));
        })
    }

    fn in_keychain(items: &Items) -> Vec<String> {
        items
            .borrow()
            .iter()
            .map(|held| String::from_utf8_lossy(held).into_owned())
            .collect()
    }

    #[test]
    fn a_write_whose_move_claim_another_process_puts_back_stores_no_second_credential() {
        let label = "write-beside-a-put-back-claim";
        assert_eq!(file_item(label).write(&secret(X)), Write::Stored);
        let items: Items = Rc::new(RefCell::new(Vec::new()));
        let this = Mac {
            keeps: Keeps::Keychain,
            file: hooked_item(
                label,
                At::Claim,
                signed_delete_if_holds_beside(label, &items),
            ),
            keychain: keys(SharedKeys {
                items: Rc::clone(&items),
            }),
        };
        let y: &[u8] = b"credential Y of a second sign-in";
        let wrote = this.write(&secret(y));
        let file_x = matches!(file_item(label).read(), Read::Found(held) if held.expose() == X);
        let keychain_y = items.borrow().iter().any(|held| held.as_slice() == y);
        println!(
            "write-beside-a-put-back-claim wrote={wrote:?} file_x={file_x} keychain={:?} names={:?}",
            in_keychain(&items),
            names_in(label)
        );
        assert!(
            !(matches!(wrote, Write::Stored) && file_x),
            "the write stored a second credential while X stands in the file"
        );
        assert!(
            !(file_x && keychain_y),
            "X in the file and Y in the Keychain"
        );
    }

    #[test]
    fn a_move_whose_claim_another_process_puts_back_never_reads_not_stored_while_the_file_stands() {
        let label = "read-beside-a-put-back-claim";
        assert_eq!(file_item(label).write(&secret(X)), Write::Stored);
        let items: Items = Rc::new(RefCell::new(Vec::new()));
        let this = Mac {
            keeps: Keeps::Keychain,
            file: hooked_item(
                label,
                At::Claim,
                signed_delete_if_holds_beside(label, &items),
            ),
            keychain: keys(SharedKeys {
                items: Rc::clone(&items),
            }),
        };
        let read = this.read();
        let stands = stands(label, &items, X);
        println!(
            "read-beside-a-put-back-claim read={read:?} stands={stands} keychain={:?} names={:?}",
            in_keychain(&items),
            names_in(label)
        );
        assert!(stands, "X stands after the read");
        assert!(
            !matches!(read, Read::NotStored),
            "the move read NotStored while X stands"
        );
        assert_eq!(found(read), X, "the move's second look moves X");
        assert_eq!(*items.borrow(), [X.to_vec()]);
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
    }

    #[test]
    fn a_move_whose_claim_is_put_back_twice_answers_a_fault_and_keeps_the_file() {
        let label = "read-beside-two-put-back-claims";
        assert_eq!(file_item(label).write(&secret(X)), Write::Stored);
        let items: Items = Rc::new(RefCell::new(Vec::new()));
        let this = Mac {
            keeps: Keeps::Keychain,
            file: hooked_items(
                label,
                At::Claim,
                vec![
                    signed_delete_if_holds_beside(label, &items),
                    signed_delete_if_holds_beside(label, &items),
                ],
            ),
            keychain: keys(SharedKeys {
                items: Rc::clone(&items),
            }),
        };
        let read = this.read();
        assert!(matches!(read, Read::Fault(Fault { code: -1 })), "{read:?}");
        assert_eq!(found(file_item(label).read()), X);
        assert!(items.borrow().is_empty());
    }

    fn claim_name_of_another_process(label: &str) -> std::path::PathBuf {
        parent(label)
            .join("store")
            .join(format!(".account.{}.0.claim", std::process::id() + 1))
    }

    #[test]
    fn a_move_that_finds_its_file_claimed_by_another_process_looks_once_more() {
        for at in [At::OpenName, At::BeforeClaim] {
            let label = format!("move-file-claimed-{at:?}").to_lowercase();
            assert_eq!(file_item(&label).write(&secret(X)), Write::Stored);
            let (from, to) = (
                parent(&label).join("store").join("account"),
                claim_name_of_another_process(&label),
            );
            let another: Box<dyn FnOnce()> = Box::new(move || {
                std::fs::rename(&from, &to).expect("another process's claim");
            });
            let hooks: Vec<Box<dyn FnOnce()>> = if at == At::OpenName {
                vec![Box::new(|| {}), another]
            } else {
                vec![another]
            };
            let items: Items = Rc::new(RefCell::new(Vec::new()));
            let this = Mac {
                keeps: Keeps::Keychain,
                file: hooked_items(&label, at, hooks),
                keychain: keys(SharedKeys {
                    items: Rc::clone(&items),
                }),
            };
            assert_eq!(found(this.read()), X, "{at:?}");
            assert_eq!(*items.borrow(), [X.to_vec()], "{at:?}");
            assert!(
                names_in(&label).is_empty(),
                "{at:?}: {:?}",
                names_in(&label)
            );
        }
    }

    #[test]
    fn a_move_whose_claim_another_process_removes_before_its_unlink_reads_found() {
        let label = "move-claim-removed-before-its-unlink";
        assert_eq!(file_item(label).write(&secret(X)), Write::Stored);
        let store = parent(label).join("store");
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            let claim = std::fs::read_dir(&store)
                .expect("the store directory")
                .map(|entry| entry.expect("an entry").path())
                .find(|path| path.to_string_lossy().ends_with(".claim"))
                .expect("the move's claim");
            std::fs::remove_file(claim).expect("another process ends the move");
        });
        let items: Items = Rc::new(RefCell::new(Vec::new()));
        let this = Mac {
            keeps: Keeps::Keychain,
            file: hooked_item(label, At::RemoveClaim, hook),
            keychain: keys(SharedKeys {
                items: Rc::clone(&items),
            }),
        };
        assert_eq!(found(this.read()), X);
        assert_eq!(*items.borrow(), [X.to_vec()]);
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
    }

    #[test]
    fn a_removal_of_the_file_during_another_process_claim_never_answers_not_stored() {
        for (keeps, at) in [
            (Keeps::File, At::OpenName),
            (Keeps::Keychain, At::OpenName),
            (Keeps::File, At::RemoveName),
            (Keeps::Keychain, At::RemoveName),
        ] {
            let label = format!("removal-{keeps:?}-{at:?}-during-a-claim").to_lowercase();
            assert_eq!(file_item(&label).write(&secret(X)), Write::Stored);
            let (hook, claim) = claim_of_another_process(&label);
            let items: Items = Rc::new(RefCell::new(Vec::new()));
            let this = Mac {
                keeps,
                file: hooked_item(&label, at, hook),
                keychain: keys(SharedKeys {
                    items: Rc::clone(&items),
                }),
            };
            let removed = this.remove(StoreKind::File);
            named_back(&label, &claim);
            let stands = stands(&label, &items, X);
            println!("removal {keeps:?} {at:?} removed={removed:?} stands={stands}");
            assert_ne!(
                removed,
                Some(Delete::NotStored),
                "{keeps:?} {at:?}: the removal answered NotStored while X stood in a claim"
            );
            assert_eq!(
                removed == Some(Delete::Deleted),
                !stands,
                "{keeps:?} {at:?}: {removed:?}"
            );
        }
    }

    fn vanishing(label: &str, at: At) -> (Box<dyn FnOnce()>, std::path::PathBuf) {
        let store = parent(label).join("store");
        let claim = store.join(format!(".account.{}.0.claim", std::process::id() + 1));
        let gone = claim.clone();
        let name = store.join("account");
        let hook: Box<dyn FnOnce()> = Box::new(move || match at {
            At::RenameBack => std::fs::rename(&gone, &name).expect("the owner names it back"),
            _ => std::fs::remove_file(&gone).expect("the owner removes its claim"),
        });
        (hook, claim)
    }

    #[test]
    fn a_read_beside_another_process_s_claim_that_goes_answers_no_hard_link_and_no_enoent() {
        for (keeps, at, keychain) in [
            (Keeps::File, At::OpenClaim, false),
            (Keeps::File, At::MetaOfClaim, false),
            (Keeps::File, At::RenameBack, false),
            (Keeps::Keychain, At::OpenClaim, false),
            (Keeps::Keychain, At::MetaOfClaim, false),
            (Keeps::Keychain, At::RenameBack, false),
            (Keeps::Keychain, At::RemoveClaim, true),
        ] {
            let label = format!("settle-{keeps:?}-{at:?}-{keychain}").to_lowercase();
            assert_eq!(file_item(&label).write(&secret(X)), Write::Stored);
            let store = parent(&label).join("store");
            std::fs::rename(
                store.join("account"),
                store.join(format!(".account.{}.0.claim", std::process::id() + 1)),
            )
            .expect("another process's live claim");
            let items: Items = Rc::new(RefCell::new(Vec::new()));
            if keychain {
                items.borrow_mut().push(X.to_vec());
            }
            let (hook, _claim) = vanishing(&label, at);
            let this = Mac {
                keeps,
                file: hooked_item(&label, at, hook),
                keychain: keys(SharedKeys {
                    items: Rc::clone(&items),
                }),
            };
            let read = this.read();
            let stands = stands(&label, &items, X);
            println!("settle {keeps:?} {at:?} read={read:?} stands={stands}");
            match (at, read) {
                (At::RenameBack | At::RemoveClaim, Read::Found(held)) => {
                    assert_eq!(held.expose(), X, "{keeps:?} {at:?}");
                    assert!(stands, "{keeps:?} {at:?}");
                }
                (At::OpenClaim | At::MetaOfClaim, Read::NotStored) => {
                    assert!(!stands, "{keeps:?} {at:?}: NotStored while X stands")
                }
                (at, read) => panic!("{keeps:?} {at:?}: the read answered {read:?}"),
            }
        }
    }

    #[test]
    fn a_move_whose_claim_another_process_removes_after_its_open_reads_found() {
        let label = "move-claim-removed-after-its-open";
        assert_eq!(file_item(label).write(&secret(X)), Write::Stored);
        let store = parent(label).join("store");
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            let claim = std::fs::read_dir(&store)
                .expect("the store directory")
                .map(|entry| entry.expect("an entry").path())
                .find(|path| path.to_string_lossy().ends_with(".claim"))
                .expect("the move's claim");
            std::fs::remove_file(claim).expect("another process ends the move");
        });
        let items: Items = Rc::new(RefCell::new(Vec::new()));
        let this = Mac {
            keeps: Keeps::Keychain,
            file: hooked_item(label, At::MetaOfClaim, hook),
            keychain: keys(SharedKeys {
                items: Rc::clone(&items),
            }),
        };
        let read = this.read();
        assert_eq!(found(read), X);
        assert_eq!(*items.borrow(), [X.to_vec()]);
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
    }

    const Y: &[u8] = b"credential Y of a second sign-in";

    /// Puts `bytes` under `name` in the store directory, made with mode 0700 when absent, as
    /// another process's file of mode 0600.
    fn put(label: &str, name: &str, bytes: &[u8]) {
        use std::io::Write as _;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let store = parent(label).join("store");
        if !store.exists() {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&store)
                .expect("the store directory");
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(store.join(name))
            .and_then(|mut file| file.write_all(bytes))
            .expect("a file of another process");
    }

    fn bytes_in(label: &str, name: &str) -> Option<Vec<u8>> {
        std::fs::read(parent(label).join("store").join(name)).ok()
    }

    fn another_claim(n: u32) -> String {
        format!(".account.{}.0.claim", std::process::id() + n)
    }

    fn another_temp(n: u32) -> String {
        format!(".account.{}.0.tmp", std::process::id() + n)
    }

    /// G1 (b), the verifier's H interleaving: another process claims the file that holds X
    /// while the write looks, so the write publishes Y beside X's claim.
    #[test]
    fn a_write_that_looks_while_another_process_claims_the_file_never_answers_stored() {
        let label = "write-looks-during-a-claim";
        put(label, "account", X);
        let store = parent(label).join("store");
        let (from, to) = (store.join("account"), store.join(another_claim(1)));
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            std::fs::rename(&from, &to).expect("another process's claim of X");
        });
        let wrote =
            hooked_item(label, At::Exists, hook).after_claims(|item| item.write(&secret(Y)));
        println!(
            "write-looks-during-a-claim wrote={wrote:?} names={:?}",
            names_in(label)
        );
        assert_eq!(wrote, Write::Fault(Fault { code: -1 }));
        assert_eq!(bytes_in(label, &another_claim(1)), Some(X.to_vec()));
        assert_eq!(bytes_in(label, "account"), Some(Y.to_vec()));
    }

    /// G1 (c), the replace window: the caller read X, the file is replaced by W before the
    /// claim, and a write of Y falls inside the claim.
    #[test]
    fn a_file_replaced_between_the_read_and_the_claim_of_a_delete_if_holds_answers_by_the_limit() {
        let label = "replace-window";
        put(label, "account", X);
        let w: &[u8] = b"credential W that replaced X....";
        let name = parent(label).join("store").join("account");
        let replace: Box<dyn FnOnce()> = Box::new(move || {
            use std::io::Write as _;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::remove_file(&name).expect("another process's delete of X");
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&name)
                .and_then(|mut file| file.write_all(w))
                .expect("another process's write of W");
        });
        let wrote = Rc::new(RefCell::new(None));
        let answer = Rc::clone(&wrote);
        let writer = label.to_string();
        let write_inside: Box<dyn FnOnce()> = Box::new(move || {
            *answer.borrow_mut() = Some(file_item(&writer).write(&secret(Y)));
        });
        let item = hooked_at(
            label,
            vec![(At::BeforeClaim, replace), (At::Claim, write_inside)],
        );
        let deleted = item.after_claims(|item| item.delete_if_holds(X));
        let wrote = wrote.borrow_mut().take();
        let claims: Vec<String> = names_in(label)
            .into_iter()
            .filter(|entry| entry.ends_with(".claim"))
            .collect();
        println!(
            "replace-window wrote={wrote:?} deleted={deleted:?} names={:?}",
            names_in(label)
        );
        assert_eq!(wrote, Some(Write::Fault(Fault { code: -1 })));
        assert!(
            matches!(deleted, DeleteIfHolds::Fault(Fault { code: -1 })),
            "{deleted:?}"
        );
        assert_eq!(claims.len(), 1, "{claims:?}");
        assert_eq!(bytes_in(label, &claims[0]), Some(w.to_vec()));
        assert_eq!(bytes_in(label, "account"), Some(Y.to_vec()));
        let later = file_item(label);
        assert!(matches!(
            later.after_claims(Item::read),
            Read::Fault(Fault { code: -1 })
        ));
        assert_eq!(
            later.after_claims(|item| item.write(&secret(X))),
            Write::Fault(Fault { code: -1 })
        );
        assert_eq!(
            later.after_claims(Item::delete),
            Delete::Fault(Fault { code: -1 })
        );
        assert!(matches!(
            later.after_claims(|item| item.delete_if_holds(Y)),
            DeleteIfHolds::Fault(Fault { code: -1 })
        ));
        assert_eq!(names_in(label).len(), 2, "{:?}", names_in(label));
        assert_eq!(later.remove_at_word(None), Delete::Deleted);
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
        assert!(matches!(later.after_claims(Item::read), Read::NotStored));
    }

    /// G2: in the build with the Keychain, a claim beside a taken name that the settle can
    /// neither put back nor give is the answer of every read, write and delete, and nothing
    /// is removed, moved or stored. The cases 23, 28, 34, 39 and 42 of verifier-38.
    #[test]
    fn a_claim_that_cannot_be_settled_beside_a_keychain_item_is_the_answer_and_nothing_changes() {
        let a: &[u8] = b"AAAAAAAAAAAAAAAA";
        let ab: &[u8] = b"AAAAAAAAAAAAAAAB";
        let bb: &[u8] = b"BBBBBBBB";
        type Op = fn(&Mac<Item<UnixFs, Plain>, Keys>) -> String;
        let read: Op = |store| format!("{:?}", store.read());
        let delete: Op = |store| format!("{:?}", store.delete());
        let write: Op = |store| format!("{:?}", store.write(&secret(Y)));
        let dih_a: Op = |store| format!("{:?}", store.delete_if_holds(b"AAAAAAAAAAAAAAAA"));
        let dih_ab: Op = |store| format!("{:?}", store.delete_if_holds(b"AAAAAAAAAAAAAAAB"));
        for (at, (case, file, keychain, claim, temp, op)) in [
            (42, a, ab, a, false, read),
            (23, ab, bb, ab, false, delete),
            (34, bb, a, ab, true, delete),
            (28, a, ab, a, false, dih_a),
            (39, a, a, ab, false, dih_ab),
            (42, a, ab, a, false, write),
        ]
        .into_iter()
        .enumerate()
        {
            let label = format!("unsettled-claim-{case}-{at}");
            put(&label, "account", file);
            put(&label, &another_claim(1), claim);
            if temp {
                put(&label, &another_temp(2), b"a write that was stopped");
            }
            let before = names_in(&label);
            let store = signed(&label, Keys::holding(&[keychain]));
            let answer = op(&store);
            println!("unsettled claim case {case}: {answer}");
            assert_eq!(answer, "Fault(Fault { code: -1 })", "case {case}");
            assert_eq!(
                store.keychain.keychain.items(),
                [keychain.to_vec()],
                "case {case}"
            );
            assert_eq!(names_in(&label), before, "case {case}");
            assert_eq!(
                bytes_in(&label, "account"),
                Some(file.to_vec()),
                "case {case}"
            );
            assert_eq!(
                bytes_in(&label, &another_claim(1)),
                Some(claim.to_vec()),
                "case {case}"
            );
            assert_eq!(
                store.remove(StoreKind::File),
                Some(Delete::Deleted),
                "case {case}"
            );
            assert_eq!(found(store.read()), keychain, "case {case}: what stands");
            assert!(
                names_in(&label).is_empty(),
                "case {case}: {:?}",
                names_in(&label)
            );
        }
        let label = "unsettled-claim-none-beside-an-item";
        let store = signed(label, Keys::holding(&[a]));
        assert_eq!(
            found(store.read()),
            a,
            "NotStored from the file place reads Found"
        );
    }

    /// G4 (b): a live write takes or renames its temporary file between the removal's
    /// listing and its removal of that file; the removal goes on.
    #[test]
    fn a_temporary_file_that_goes_during_a_removal_at_the_person_s_word_is_passed_over() {
        for renamed in [false, true] {
            let label = format!("temp-goes-during-removal-{renamed}");
            put(&label, &another_temp(1), Y);
            let store = parent(&label).join("store");
            let (temp, name) = (store.join(another_temp(1)), store.join("account"));
            let hook: Box<dyn FnOnce()> = Box::new(move || {
                if renamed {
                    std::fs::rename(&temp, &name).expect("the write publishes Y");
                } else {
                    std::fs::remove_file(&temp).expect("the write removes its file");
                }
            });
            let item = hooked_item(&label, At::RemoveTemp, hook);
            let removed = item.remove_at_word(None);
            let then = file_item(&label).after_claims(Item::read);
            println!("temp-goes renamed={renamed} removed={removed:?} then={then:?}");
            assert!(
                matches!(removed, Delete::Deleted | Delete::NotStored),
                "{renamed}: {removed:?}"
            );
            assert!(
                matches!(&then, Read::NotStored)
                    || matches!(&then, Read::Found(held) if held.expose() == Y),
                "{renamed}: {then:?}"
            );
        }
    }

    /// G4 (c): another process renames the move's claim back to the store's name before the
    /// move's unlink, so the listing is not empty: the move looks once more.
    #[test]
    fn a_move_whose_claim_is_named_back_before_its_unlink_looks_once_more() {
        let label = "move-claim-named-back-before-its-unlink";
        put(label, "account", X);
        let store = parent(label).join("store");
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            let claim = std::fs::read_dir(&store)
                .expect("the store directory")
                .map(|entry| entry.expect("an entry").path())
                .find(|path| path.to_string_lossy().ends_with(".claim"))
                .expect("the move's claim");
            std::fs::rename(claim, store.join("account")).expect("another process names it back");
        });
        let items: Items = Rc::new(RefCell::new(Vec::new()));
        let this = Mac {
            keeps: Keeps::Keychain,
            file: hooked_item(label, At::RemoveClaim, hook),
            keychain: keys(SharedKeys {
                items: Rc::clone(&items),
            }),
        };
        assert_eq!(found(this.read()), X);
        assert_eq!(*items.borrow(), [X.to_vec()]);
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
    }

    /// G4 (d): only the removal at the person's word removes a temporary file.
    #[test]
    fn no_operation_but_the_removal_at_the_person_s_word_removes_a_temporary_file() {
        for keeps in [Keeps::File, Keeps::Keychain] {
            let label = format!("temp-kept-{keeps:?}").to_lowercase();
            put(&label, "account", X);
            put(&label, &another_temp(1), Y);
            let this = Mac {
                keeps,
                file: file_item(&label),
                keychain: keys(Keys::empty()),
            };
            assert!(matches!(this.read(), Read::Found(_)), "{keeps:?}");
            assert_eq!(this.write(&secret(Y)), Write::Occupied, "{keeps:?}");
            assert!(
                matches!(this.delete_if_holds(Y), DeleteIfHolds::Other(_)),
                "{keeps:?}"
            );
            assert_eq!(this.delete(), Delete::Deleted, "{keeps:?}");
            assert_eq!(names_in(&label), [another_temp(1)], "{keeps:?}");
            assert_eq!(
                this.remove(StoreKind::File),
                Some(Delete::NotStored),
                "{keeps:?}"
            );
            assert!(names_in(&label).is_empty(), "{keeps:?}");
        }
    }

    /// G4 (f): a claim that appears between the removal's first settle and its delete. One of
    /// other bytes than the Keychain's is a credential: the removal answers Deleted. One of
    /// the Keychain's bytes is a killed move's leftover: NotStored. And a claim of other bytes
    /// that the removal took away beside a name that then goes leaves Deleted, not NotStored.
    #[test]
    fn a_removal_at_the_person_s_word_that_removed_a_credential_never_answers_not_stored() {
        let k: &[u8] = b"the Keychain item's credential..";
        for (bytes, want) in [(Y, Delete::Deleted), (k, Delete::NotStored)] {
            let label = format!("claim-before-the-delete-{}", bytes == Y);
            put(&label, &another_temp(3), b"a write that was stopped");
            let claim = parent(&label).join("store").join(another_claim(1));
            let owned = bytes.to_vec();
            let make: Box<dyn FnOnce()> = Box::new(move || {
                use std::io::Write as _;
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&claim)
                    .and_then(|mut file| file.write_all(&owned))
                    .expect("another process's claim");
            });
            let item = hooked_at(
                &label,
                vec![(At::Names, Box::new(|| {})), (At::Names, make)],
            );
            let removed = item.remove_at_word(Some(k));
            println!(
                "claim-before-the-delete other={} removed={removed:?}",
                bytes == Y
            );
            assert_eq!(removed, want, "{}", bytes == Y);
            assert!(names_in(&label).is_empty(), "{:?}", names_in(&label));
        }

        let label = "stranded-claim-then-the-name-goes";
        put(label, "account", X);
        put(label, &another_claim(1), Y);
        let name = parent(label).join("store").join("account");
        let goes: Box<dyn FnOnce()> = Box::new(move || {
            std::fs::remove_file(&name).expect("another process's delete");
        });
        let removed = hooked_item(label, At::OpenName, goes).remove_at_word(Some(k));
        assert_eq!(removed, Delete::Deleted);
        assert!(names_in(label).is_empty(), "{:?}", names_in(label));
    }

    /// G2: which failures of the file place come from a claim. A claim that cannot be read or
    /// put back, or one that is still there at the second listing, is the answer; a failed
    /// walk or listing of the store directory is no claim's, and the Keychain item stands in.
    #[test]
    fn a_failure_of_a_claim_is_the_answer_and_a_failure_of_the_place_gives_the_item() {
        const EIO: i32 = 5;
        let k: &[u8] = b"the Keychain item's credential..";
        for (case, fault, claim, want) in [
            (
                "read-of-the-claim",
                At::ReadClaim,
                true,
                "Fault(Fault { code: 5 })",
            ),
            (
                "rename-back",
                At::RenameBack,
                true,
                "Fault(Fault { code: 5 })",
            ),
            ("listing", At::Names, true, "Found(..)"),
            ("walk", At::Walk, false, "Found(..)"),
        ] {
            let label = format!("failure-of-{case}");
            put(&label, "keep", b"the store directory");
            if claim {
                put(&label, &another_claim(1), Y);
            }
            let store = Mac {
                keeps: Keeps::Keychain,
                file: hooked_and_faulted(&label, Vec::new(), vec![(fault, EIO)]),
                keychain: keys(Keys::holding(&[k])),
            };
            let read = store.read();
            println!("failure of the {case}: {read:?}");
            assert_eq!(format!("{read:?}"), want, "{case}");
            if let Read::Found(held) = &read {
                assert_eq!(held.expose(), k, "{case}");
            }
            assert_eq!(store.keychain.keychain.items(), [k.to_vec()], "{case}");
            if claim && want != "Found(..)" {
                assert_eq!(
                    bytes_in(&label, &another_claim(1)),
                    Some(Y.to_vec()),
                    "{case}"
                );
            }
        }

        let label = "claim-at-the-second-listing";
        put(label, "keep", b"the store directory");
        let store_dir = parent(label).join("store");
        let make = |n: u32| -> Box<dyn FnOnce()> {
            let path = store_dir.join(another_claim(n));
            Box::new(move || {
                use std::io::Write as _;
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)
                    .and_then(|mut file| file.write_all(b"the Keychain item's credential.."))
                    .expect("another process's claim");
            })
        };
        let store = Mac {
            keeps: Keeps::Keychain,
            file: hooked_at(
                label,
                vec![
                    (At::Names, Box::new(|| {})),
                    (At::Names, make(1)),
                    (At::Names, Box::new(|| {})),
                    (At::Names, make(2)),
                ],
            ),
            keychain: keys(Keys::holding(&[k])),
        };
        let read = store.read();
        println!("claim at the second listing: {read:?}");
        assert!(matches!(read, Read::Fault(Fault { code: -1 })), "{read:?}");
        assert_eq!(store.keychain.keychain.items(), [k.to_vec()]);
    }
}
