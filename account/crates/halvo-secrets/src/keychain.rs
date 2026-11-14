use crate::answer::{Delete, Fault, Locked, Read, Write};
use crate::{Name, Secret};

#[cfg(all(
    target_os = "macos",
    feature = "hosted-store-tests",
    feature = "keychain"
))]
pub(crate) mod failing;
#[cfg(target_os = "macos")]
pub(crate) mod system;

pub(crate) const ITEM_NOT_FOUND: i32 = -25300;
pub(crate) const DUPLICATE_ITEM: i32 = -25299;
pub(crate) const AUTH_FAILED: i32 = -25293;
pub(crate) const INTERACTION_NOT_ALLOWED: i32 = -25308;
pub(crate) const USER_CANCELED: i32 = -128;

pub(crate) trait Keychain {
    fn count(&self, service: &str, account: &str) -> Result<usize, i32>;
    fn data(&self, service: &str, account: &str) -> Result<Vec<u8>, i32>;
    fn add(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), i32>;
    fn delete(&self, service: &str, account: &str) -> Result<(), i32>;
}

impl<K: Keychain + ?Sized> Keychain for Box<K> {
    fn count(&self, service: &str, account: &str) -> Result<usize, i32> {
        (**self).count(service, account)
    }

    fn data(&self, service: &str, account: &str) -> Result<Vec<u8>, i32> {
        (**self).data(service, account)
    }

    fn add(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), i32> {
        (**self).add(service, account, bytes)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), i32> {
        (**self).delete(service, account)
    }
}

pub(crate) struct KeychainItem<K> {
    pub(crate) keychain: K,
    pub(crate) service: Name,
    pub(crate) account: Name,
}

enum Class {
    Absent,
    Duplicate,
    Refused(Locked),
    Fault,
}

fn class(code: i32) -> Class {
    match code {
        ITEM_NOT_FOUND => Class::Absent,
        DUPLICATE_ITEM => Class::Duplicate,
        AUTH_FAILED | INTERACTION_NOT_ALLOWED | USER_CANCELED => Class::Refused(Locked::Refused {
            code: i64::from(code),
        }),
        _ => Class::Fault,
    }
}

fn fault(code: i32) -> Fault {
    Fault {
        code: i64::from(code),
    }
}

impl<K: Keychain> KeychainItem<K> {
    fn names(&self) -> (&str, &str) {
        (self.service.as_str(), self.account.as_str())
    }

    pub(crate) fn count(&self) -> Result<usize, Result<Locked, Fault>> {
        let (service, account) = self.names();
        match self.keychain.count(service, account) {
            Ok(n) => Ok(n),
            Err(code) => match class(code) {
                Class::Absent => Ok(0),
                Class::Refused(locked) => Err(Ok(locked)),
                Class::Duplicate | Class::Fault => Err(Err(fault(code))),
            },
        }
    }

    pub(crate) fn read(&self) -> Read {
        match self.count() {
            Ok(0) => return Read::NotStored,
            Ok(1) => {}
            Ok(_) => return Read::Locked(Locked::Duplicate),
            Err(Ok(locked)) => return Read::Locked(locked),
            Err(Err(fault)) => return Read::Fault(fault),
        }
        let (service, account) = self.names();
        match self.keychain.data(service, account) {
            Ok(bytes) => match Secret::new(bytes) {
                Some(secret) => Read::Found(secret),
                None => Read::Locked(Locked::Size),
            },
            Err(code) => match class(code) {
                Class::Absent => Read::NotStored,
                Class::Refused(locked) => Read::Locked(locked),
                Class::Duplicate | Class::Fault => Read::Fault(fault(code)),
            },
        }
    }

    pub(crate) fn write(&self, secret: &Secret) -> Write {
        match self.count() {
            Ok(0) => {}
            Ok(_) => return Write::Occupied,
            Err(Ok(locked)) => return Write::Locked(locked),
            Err(Err(fault)) => return Write::Fault(fault),
        }
        let (service, account) = self.names();
        match self.keychain.add(service, account, secret.expose()) {
            Ok(()) => Write::Stored,
            Err(code) => match class(code) {
                Class::Duplicate => Write::Occupied,
                Class::Refused(locked) => Write::Locked(locked),
                Class::Absent | Class::Fault => Write::Fault(fault(code)),
            },
        }
    }

    pub(crate) fn delete(&self) -> Delete {
        match self.count() {
            Ok(0) => return Delete::NotStored,
            Ok(1) => {}
            Ok(_) => return Delete::Locked(Locked::Duplicate),
            Err(Ok(locked)) => return Delete::Locked(locked),
            Err(Err(fault)) => return Delete::Fault(fault),
        }
        let (service, account) = self.names();
        match self.keychain.delete(service, account) {
            Ok(()) => Delete::Deleted,
            Err(code) => match class(code) {
                Class::Absent => Delete::NotStored,
                Class::Refused(locked) => Delete::Locked(locked),
                Class::Duplicate | Class::Fault => Delete::Fault(fault(code)),
            },
        }
    }
}

#[cfg(test)]
mod tests;
