use crate::answer::{Delete, Read, StoreKind, Write};
#[cfg(windows)]
use crate::dpapi::{Dpapi, WindowsFs};
#[cfg(target_os = "macos")]
use crate::keychain::{system::SystemKeychain, Keychain, KeychainItem};
#[cfg(target_os = "macos")]
use crate::mac::{Keeps, Mac};
use crate::place::{Item, Rules};
#[cfg(target_os = "macos")]
use crate::{place::Plain, unix::UnixFs};
use crate::{Directory, Name, Secret};

/// The one store of a secret under one name, picked when the crate is built. On macOS it is
/// the file `<dir>/<name>` of mode 0600 in a directory of mode 0700, or, when the crate is
/// built with the feature `keychain` that a Developer ID release sets, the Keychain item of
/// `service` and `name`. On Windows it is the file `<dir>/<name>` that holds the secret
/// sealed by DPAPI in the scope of the user. A read that fails answers `Locked` or `Fault`
/// and never `NotStored`; a write only creates; a delete removes the one item that a read
/// gives.
///
/// The place on macOS: every call walks the caller's path as given, from `/`, each step
/// opened for search alone, relative to the step above and never through a link. A step
/// that is a link is read relative to the directory that holds it and then followed, from
/// `/` or from that directory; one walk follows at most 32 links, and the next answers
/// `Fault` with ELOOP. The call refuses the place as `Locked::Ancestor` when a directory
/// that the walk passed can be changed by someone other than the person and root (it
/// belongs to another user, lets the group or others write without the sticky bit, or has
/// an allow entry that is not inherit-only and names another user or a group), or when a
/// link that it followed belongs to another user. An access list that cannot be read
/// answers `Fault`. The call opens the store directory once without following a link and
/// makes every later call relative to that handle.
///
/// Absent and missing, on both systems: a read answers `NotStored` only when the store
/// directory or the item is absent under a parent that exists. A step above the store
/// directory that is missing answers `Fault`, and a write makes the store directory alone,
/// never a step above it.
///
/// Both stores of a Mac, for `Store::new`: in both builds a read that finds no Keychain
/// item and no file looks at the Keychain once more before it answers `NotStored`, so a
/// move that another process finishes between its two looks is seen. A build without the
/// feature `keychain` also
/// looks for the Keychain item of `service` and `name`, by its attributes alone. When one
/// stands there, a read answers `Locked::OtherStore` whatever the file holds. When the
/// Keychain cannot be searched, a read that finds no file answers the Keychain's `Locked`
/// or `Fault`, never `NotStored`. Its write and its delete never touch the Keychain.
/// A build with the feature also reads the file `<dir>/<name>`. When the Keychain item and
/// the file both stand with other bytes, a read answers `Locked::OtherStore` and changes
/// nothing. When the file stands and the Keychain holds no item, the read moves the file
/// once: it adds its bytes to the Keychain (only-create), reads the item back, and only
/// when the bytes are equal removes the file and answers `Found`. When the add or the read
/// back fails or the removal is refused, it answers `Locked::Move` with that step's code
/// and keeps the file; a removal that faults answers that `Fault`. An item that the add
/// made stays, so when the item and the file stand with equal bytes, a read removes the
/// file and answers `Found`: it finishes a move that stopped there. When the Keychain item
/// stands and the file cannot be read, a read gives the item. Its write and its delete
/// touch the Keychain alone.
/// `Store::in_directory` reads and writes the file alone.
///
/// The place on Windows: the crate makes the directory with a protected access list that
/// gives the person, SYSTEM and Administrators full control, and refuses a directory whose
/// owner is anyone else as `Locked::Owner`; it reads no entry of that list and no step
/// above the directory, because the blob is sealed for the person, so another directory
/// put in its place can neither give the person another credential nor read theirs. Only
/// "file not found" for the store directory or the item, under a parent that is a
/// directory, reads as no item; "path not found", a link at the store directory whose
/// target is gone, and a drive or a share that cannot be reached answer `Fault`, also for
/// a delete. What this does not hold: whoever can rename or remove the
/// directory or a step above it can make the credential read as `NotStored`. By default
/// those are SYSTEM, the administrators and the person, so the caller passes a directory
/// under `%LOCALAPPDATA%`, not under `%APPDATA%` or Documents, which a redirection or a
/// sync service can move.
pub struct Store {
    place: Place,
}

#[cfg(target_os = "macos")]
type SharedKeychain = Box<dyn Keychain + Send + Sync>;

enum Place {
    #[cfg(target_os = "macos")]
    File(Item<UnixFs, Plain>),
    #[cfg(target_os = "macos")]
    Mac(Mac<Item<UnixFs, Plain>, SharedKeychain>),
    #[cfg(windows)]
    Dpapi(Item<WindowsFs, Dpapi>),
}

impl Store {
    /// The store that this build keeps the secret `name` in: on macOS the file `name` in
    /// `dir`, or with the feature `keychain` the Keychain item of `service` and `name`; on
    /// Windows the DPAPI file `name` in `dir`.
    pub fn new(dir: Directory, service: Name, name: Name) -> Store {
        #[cfg(target_os = "macos")]
        let place = {
            let keeps = if cfg!(feature = "keychain") {
                Keeps::Keychain
            } else {
                Keeps::File
            };
            mac_place(keeps, &dir, service, name, Box::new(SystemKeychain))
        };
        #[cfg(windows)]
        let place = {
            let _ = service;
            dpapi_place(&dir, name)
        };
        Store { place }
    }

    /// Test builds with the features `hosted-store-tests` and `keychain` on macOS only: the
    /// store of `Store::new`, whose Keychain fails the step `fault` of the move.
    #[cfg(all(
        target_os = "macos",
        feature = "hosted-store-tests",
        feature = "keychain"
    ))]
    #[doc(hidden)]
    pub fn with_move_fault(
        dir: Directory,
        service: Name,
        name: Name,
        fault: crate::MoveFault,
    ) -> Store {
        let keychain = crate::keychain::failing::Failing {
            inner: SystemKeychain,
            fault,
        };
        Store {
            place: mac_place(Keeps::Keychain, &dir, service, name, Box::new(keychain)),
        }
    }

    /// Debug builds only: the file `name` in `dir` (sealed by DPAPI on Windows), whatever
    /// the features, so that a gate keeps its secret in a temporary directory and never
    /// reads or writes the Keychain.
    #[cfg(debug_assertions)]
    pub fn in_directory(dir: Directory, name: Name) -> Store {
        #[cfg(target_os = "macos")]
        let place = Place::File(file_item(&dir, name));
        #[cfg(windows)]
        let place = dpapi_place(&dir, name);
        Store { place }
    }

    /// The kind of store that this one reads and writes.
    pub fn kind(&self) -> StoreKind {
        match &self.place {
            #[cfg(target_os = "macos")]
            Place::File(_) => StoreKind::File,
            #[cfg(target_os = "macos")]
            Place::Mac(mac) => match mac.keeps {
                Keeps::File => StoreKind::File,
                Keeps::Keychain => StoreKind::Keychain,
            },
            #[cfg(windows)]
            Place::Dpapi(_) => StoreKind::Dpapi,
        }
    }

    /// Reads the secret: `NotStored`, `Found`, `Locked` or `Fault`.
    pub fn read(&self) -> Read {
        match &self.place {
            #[cfg(target_os = "macos")]
            Place::File(item) => item.read(),
            #[cfg(target_os = "macos")]
            Place::Mac(mac) => mac.read(),
            #[cfg(windows)]
            Place::Dpapi(item) => item.read(),
        }
    }

    /// Stores `secret` when nothing stands under the name, and changes nothing otherwise.
    pub fn write(&self, secret: &Secret) -> Write {
        match &self.place {
            #[cfg(target_os = "macos")]
            Place::File(item) => item.write(secret),
            #[cfg(target_os = "macos")]
            Place::Mac(mac) => mac.write(secret),
            #[cfg(windows)]
            Place::Dpapi(item) => item.write(secret),
        }
    }

    /// Removes the item that a read would give, and nothing else.
    pub fn delete(&self) -> Delete {
        match &self.place {
            #[cfg(target_os = "macos")]
            Place::File(item) => item.delete(),
            #[cfg(target_os = "macos")]
            Place::Mac(mac) => mac.delete(),
            #[cfg(windows)]
            Place::Dpapi(item) => item.delete(),
        }
    }
}

#[cfg(target_os = "macos")]
fn mac_place(
    keeps: Keeps,
    dir: &Directory,
    service: Name,
    name: Name,
    keychain: SharedKeychain,
) -> Place {
    Place::Mac(Mac {
        keeps,
        file: file_item(dir, name.clone()),
        keychain: KeychainItem {
            keychain,
            service,
            account: name,
        },
    })
}

#[cfg(target_os = "macos")]
fn file_item(dir: &Directory, name: Name) -> Item<UnixFs, Plain> {
    Item {
        fs: UnixFs,
        codec: Plain,
        rules: Rules::Private,
        dir: dir.as_path().to_path_buf(),
        name,
    }
}

#[cfg(windows)]
fn dpapi_place(dir: &Directory, name: Name) -> Place {
    Place::Dpapi(Item {
        fs: WindowsFs,
        codec: Dpapi,
        rules: Rules::KindOnly,
        dir: dir.as_path().to_path_buf(),
        name,
    })
}
