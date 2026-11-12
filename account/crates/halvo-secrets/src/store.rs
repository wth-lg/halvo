use crate::answer::{Delete, Read, StoreKind, Write};
#[cfg(feature = "keychain")]
use crate::keychain::{system::SystemKeychain, KeychainItem};
use crate::place::{Item, Plain, Rules};
use crate::unix::UnixFs;
use crate::{Directory, Name, Secret};

/// The one store of a secret under one name, picked when the crate is built. On macOS it is
/// the file `<dir>/<name>` of mode 0600 in a directory of mode 0700, or, when the crate is
/// built with the feature `keychain` that a Developer ID release sets, the Keychain item of
/// `service` and `name`. A read that fails answers `Locked` or `Fault` and never
/// `NotStored`; a write only creates; a delete removes the one item that a read gives.
pub struct Store {
    place: Place,
}

enum Place {
    File(Item<UnixFs, Plain>),
    #[cfg(feature = "keychain")]
    Keychain(KeychainItem<SystemKeychain>),
}

impl Store {
    /// The store that this build keeps the secret `name` in: the file `name` in `dir`, or
    /// with the feature `keychain` the Keychain item of `service` and `name`.
    pub fn new(dir: Directory, service: Name, name: Name) -> Store {
        #[cfg(feature = "keychain")]
        {
            let _ = dir;
            Store {
                place: Place::Keychain(KeychainItem {
                    keychain: SystemKeychain,
                    service,
                    account: name,
                }),
            }
        }
        #[cfg(not(feature = "keychain"))]
        {
            let _ = service;
            Store::file(dir, name)
        }
    }

    /// Debug builds only: the file `name` in `dir`, whatever the features, so that a gate
    /// keeps its secret in a temporary directory and never in the Keychain.
    #[cfg(debug_assertions)]
    pub fn in_directory(dir: Directory, name: Name) -> Store {
        Store::file(dir, name)
    }

    #[cfg_attr(feature = "keychain", allow(dead_code))]
    fn file(dir: Directory, name: Name) -> Store {
        Store {
            place: Place::File(Item {
                fs: UnixFs,
                codec: Plain,
                rules: Rules::Private,
                dir: dir.as_path().to_path_buf(),
                name,
            }),
        }
    }

    /// The kind of store that this one reads and writes.
    pub fn kind(&self) -> StoreKind {
        match &self.place {
            Place::File(_) => StoreKind::File,
            #[cfg(feature = "keychain")]
            Place::Keychain(_) => StoreKind::Keychain,
        }
    }

    /// Reads the secret: `NotStored`, `Found`, `Locked` or `Fault`.
    pub fn read(&self) -> Read {
        match &self.place {
            Place::File(item) => item.read(),
            #[cfg(feature = "keychain")]
            Place::Keychain(item) => item.read(),
        }
    }

    /// Stores `secret` when nothing stands under the name, and changes nothing otherwise.
    pub fn write(&self, secret: &Secret) -> Write {
        match &self.place {
            Place::File(item) => item.write(secret),
            #[cfg(feature = "keychain")]
            Place::Keychain(item) => item.write(secret),
        }
    }

    /// Removes the item that a read would give, and nothing else.
    pub fn delete(&self) -> Delete {
        match &self.place {
            Place::File(item) => item.delete(),
            #[cfg(feature = "keychain")]
            Place::Keychain(item) => item.delete(),
        }
    }
}
