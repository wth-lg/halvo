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
    log: RefCell<Vec<&'static str>>,
}

impl Keys {
    fn holding(items: &[&[u8]]) -> Keys {
        Keys {
            items: RefCell::new(items.iter().map(|bytes| bytes.to_vec()).collect()),
            fail: Vec::new(),
            gives_back_other_bytes: false,
            log: RefCell::new(Vec::new()),
        }
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
        self.items.borrow_mut().push(bytes.to_vec());
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
