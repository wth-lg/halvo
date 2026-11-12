use super::*;
use crate::MAX_SECRET_LEN;
use std::cell::RefCell;

const SERVICE: &str = "dev.halvo.test";
const ACCOUNT: &str = "account";
const NOT_AVAILABLE: i32 = -25291;
const REFUSALS: [i32; 3] = [AUTH_FAILED, INTERACTION_NOT_ALLOWED, USER_CANCELED];
const CLASSES: [i32; 6] = [
    ITEM_NOT_FOUND,
    DUPLICATE_ITEM,
    AUTH_FAILED,
    INTERACTION_NOT_ALLOWED,
    USER_CANCELED,
    NOT_AVAILABLE,
];

type Stored = ((String, String), Vec<u8>);

struct Fake {
    items: RefCell<Vec<Stored>>,
    fail: RefCell<Vec<(&'static str, i32)>>,
    log: RefCell<Vec<&'static str>>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            items: RefCell::new(Vec::new()),
            fail: RefCell::new(Vec::new()),
            log: RefCell::new(Vec::new()),
        }
    }

    fn holding(items: &[(&str, &[u8])]) -> Fake {
        let fake = Fake::new();
        for (account, bytes) in items {
            fake.items
                .borrow_mut()
                .push(((SERVICE.to_string(), account.to_string()), bytes.to_vec()));
        }
        fake
    }

    fn failing(self, op: &'static str, code: i32) -> Fake {
        self.fail.borrow_mut().push((op, code));
        self
    }

    fn check(&self, op: &'static str) -> Result<(), i32> {
        self.log.borrow_mut().push(op);
        match self.fail.borrow().iter().find(|(name, _)| *name == op) {
            Some((_, code)) => Err(*code),
            None => Ok(()),
        }
    }

    fn matching(&self, service: &str, account: &str) -> usize {
        self.items
            .borrow()
            .iter()
            .filter(|((s, a), _)| s == service && a == account)
            .count()
    }

    fn all(&self) -> Vec<Stored> {
        self.items.borrow().clone()
    }

    fn ops(&self) -> Vec<&'static str> {
        self.log.borrow().clone()
    }
}

impl Keychain for Fake {
    fn count(&self, service: &str, account: &str) -> Result<usize, i32> {
        self.check("count")?;
        match self.matching(service, account) {
            0 => Err(ITEM_NOT_FOUND),
            n => Ok(n),
        }
    }

    fn data(&self, service: &str, account: &str) -> Result<Vec<u8>, i32> {
        self.check("data")?;
        self.items
            .borrow()
            .iter()
            .find(|((s, a), _)| s == service && a == account)
            .map(|(_, bytes)| bytes.clone())
            .ok_or(ITEM_NOT_FOUND)
    }

    fn add(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), i32> {
        self.check("add")?;
        if self.matching(service, account) > 0 {
            return Err(DUPLICATE_ITEM);
        }
        self.items
            .borrow_mut()
            .push(((service.to_string(), account.to_string()), bytes.to_vec()));
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), i32> {
        self.check("delete")?;
        if self.matching(service, account) == 0 {
            return Err(ITEM_NOT_FOUND);
        }
        self.items
            .borrow_mut()
            .retain(|((s, a), _)| !(s == service && a == account));
        Ok(())
    }
}

fn item(fake: Fake) -> KeychainItem<Fake> {
    KeychainItem {
        keychain: fake,
        service: Name::new(SERVICE).expect("a service"),
        account: Name::new(ACCOUNT).expect("an account"),
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

fn is_refusal(code: i32) -> bool {
    REFUSALS.contains(&code)
}

#[test]
fn the_codes_are_the_os_status_values_of_the_keychain() {
    assert_eq!(
        [
            ITEM_NOT_FOUND,
            DUPLICATE_ITEM,
            AUTH_FAILED,
            INTERACTION_NOT_ALLOWED,
            USER_CANCELED
        ],
        [-25300, -25299, -25293, -25308, -128]
    );
}

#[test]
fn a_keychain_item_is_written_read_and_deleted_once() {
    let store = item(Fake::new());
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.delete(), Delete::NotStored);
    assert_eq!(store.write(&secret(b"first")), Write::Stored);
    assert_eq!(found(store.read()), b"first");
    assert_eq!(store.write(&secret(b"second")), Write::Occupied);
    assert_eq!(found(store.read()), b"first");
    assert_eq!(store.keychain.all().len(), 1);
    assert_eq!(store.delete(), Delete::Deleted);
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.delete(), Delete::NotStored);
    assert!(store.keychain.all().is_empty());
}

#[test]
fn a_write_adds_nothing_when_an_item_answers_to_the_name() {
    for holding in [
        vec![(ACCOUNT, &b"kept"[..])],
        vec![(ACCOUNT, b"one"), (ACCOUNT, b"two")],
    ] {
        let store = item(Fake::holding(&holding));
        let before = store.keychain.all();
        assert_eq!(store.write(&secret(b"new")), Write::Occupied);
        assert!(!store.keychain.ops().contains(&"add"));
        assert_eq!(store.keychain.all(), before);
    }
}

#[test]
fn a_write_that_loses_the_race_answers_occupied_and_keeps_the_winner() {
    let store = item(Fake::holding(&[(ACCOUNT, b"winner")]).failing("count", ITEM_NOT_FOUND));
    assert_eq!(store.write(&secret(b"loser")), Write::Occupied);
    assert_eq!(store.keychain.all().len(), 1);
    assert_eq!(store.keychain.all()[0].1, b"winner");
}

#[test]
fn a_failed_step_of_a_write_answers_its_class() {
    for code in CLASSES {
        let store = item(Fake::new().failing("count", code));
        let want = match code {
            ITEM_NOT_FOUND => Write::Stored,
            code if is_refusal(code) => Write::Locked(refused(code)),
            code => Write::Fault(fault(code)),
        };
        assert_eq!(store.write(&secret(b"new")), want, "count {code}");

        let store = item(Fake::new().failing("add", code));
        let want = match code {
            DUPLICATE_ITEM => Write::Occupied,
            code if is_refusal(code) => Write::Locked(refused(code)),
            code => Write::Fault(fault(code)),
        };
        assert_eq!(store.write(&secret(b"new")), want, "add {code}");
        assert!(store.keychain.all().is_empty(), "add {code}");
    }
}

#[test]
fn a_read_answers_not_stored_only_when_the_keychain_says_it_holds_no_item() {
    for op in ["count", "data"] {
        for code in CLASSES {
            let store = item(Fake::holding(&[(ACCOUNT, b"kept")]).failing(op, code));
            let read = store.read();
            let right = match code {
                ITEM_NOT_FOUND => matches!(read, Read::NotStored),
                code if is_refusal(code) => {
                    matches!(read, Read::Locked(locked) if locked == refused(code))
                }
                code => matches!(read, Read::Fault(got) if got == fault(code)),
            };
            assert!(right, "{op} {code}: {read:?}");
        }
    }
}

#[test]
fn two_items_under_one_name_are_locked_for_a_read_and_a_delete() {
    let store = item(Fake::holding(&[(ACCOUNT, b"one"), (ACCOUNT, b"two")]));
    assert!(matches!(store.read(), Read::Locked(Locked::Duplicate)));
    assert_eq!(store.delete(), Delete::Locked(Locked::Duplicate));
    assert!(!store.keychain.ops().contains(&"data"));
    assert!(!store.keychain.ops().contains(&"delete"));
    assert_eq!(store.keychain.all().len(), 2);
}

#[test]
fn a_delete_removes_the_one_item_of_its_name() {
    let store = item(Fake::holding(&[
        ("other", b"other"),
        (ACCOUNT, b"mine"),
        ("account2", b"another"),
    ]));
    assert_eq!(store.delete(), Delete::Deleted);
    let left: Vec<String> = store
        .keychain
        .all()
        .into_iter()
        .map(|((_, account), _)| account)
        .collect();
    assert_eq!(left, ["other", "account2"]);
}

#[test]
fn a_failed_step_of_a_delete_answers_its_class_and_keeps_the_item() {
    for op in ["count", "delete"] {
        for code in CLASSES {
            let store = item(Fake::holding(&[(ACCOUNT, b"kept")]).failing(op, code));
            let want = match code {
                ITEM_NOT_FOUND => Delete::NotStored,
                code if is_refusal(code) => Delete::Locked(refused(code)),
                code => Delete::Fault(fault(code)),
            };
            assert_eq!(store.delete(), want, "{op} {code}");
            assert_eq!(store.keychain.all().len(), 1, "{op} {code}");
        }
    }
}

#[test]
fn an_item_of_no_byte_or_of_too_many_is_locked_by_its_size() {
    let largest = vec![b'x'; MAX_SECRET_LEN];
    assert_eq!(
        found(item(Fake::holding(&[(ACCOUNT, &largest)])).read()),
        largest
    );
    let too_long = vec![b'x'; MAX_SECRET_LEN + 1];
    for bytes in [&b""[..], &too_long] {
        let store = item(Fake::holding(&[(ACCOUNT, bytes)]));
        assert!(matches!(store.read(), Read::Locked(Locked::Size)));
    }
}

struct Holder {
    held: RefCell<Option<Secret>>,
}

impl Keychain for Holder {
    fn count(&self, _service: &str, _account: &str) -> Result<usize, i32> {
        match self.held.borrow().as_ref() {
            Some(_) => Ok(1),
            None => Err(ITEM_NOT_FOUND),
        }
    }

    fn data(&self, _service: &str, _account: &str) -> Result<Vec<u8>, i32> {
        let held = self.held.borrow();
        let secret = held.as_ref().ok_or(ITEM_NOT_FOUND)?;
        let mut bytes = Vec::with_capacity(secret.expose().len());
        bytes.extend_from_slice(secret.expose());
        Ok(bytes)
    }

    fn add(&self, _service: &str, _account: &str, bytes: &[u8]) -> Result<(), i32> {
        let mut copy = Vec::with_capacity(bytes.len());
        copy.extend_from_slice(bytes);
        *self.held.borrow_mut() = Secret::new(copy);
        Ok(())
    }

    fn delete(&self, _service: &str, _account: &str) -> Result<(), i32> {
        self.held
            .borrow_mut()
            .take()
            .map(drop)
            .ok_or(ITEM_NOT_FOUND)
    }
}

#[test]
fn a_keychain_round_trip_frees_no_copy_of_the_secret() {
    use crate::secret::tests::{seen, CANARIES};
    let store = KeychainItem {
        keychain: Holder {
            held: RefCell::new(None),
        },
        service: Name::new(SERVICE).expect("a service"),
        account: Name::new(ACCOUNT).expect("an account"),
    };
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(CANARIES[3]);
    let secret = Secret::new(bytes).expect("a secret");
    assert_eq!(store.write(&secret), Write::Stored);
    drop(secret);
    match store.read() {
        Read::Found(found) => assert!(found.expose() == CANARIES[3]),
        other => panic!("read answered {other:?}"),
    }
    assert_eq!(store.delete(), Delete::Deleted);
    drop(store);
    assert_eq!(
        seen(3),
        0,
        "a keychain call freed a copy of the secret as it was"
    );
}
