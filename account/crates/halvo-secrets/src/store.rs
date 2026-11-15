use crate::answer::{Delete, DeleteIfHolds, Read, Removal, StoreKind, Write};
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
/// gives; `delete_if_holds` removes it only while it holds the bytes that its caller read;
/// and `remove`, at the person's word, removes the item of one named store.
///
/// The removal by `delete_if_holds` from a file, on both systems: it claims the file by one
/// rename, which never replaces a name, to the claim's private name in the store
/// directory, reads what it claimed, and removes it only when it holds the caller's bytes,
/// byte for byte; another file, or one that cannot be read or removed, is named back. So a
/// file that another process puts under the name after the claim stays. When the claimed
/// file is gone before it is read, because another operation put it back, the removal
/// starts again once and then answers `Fault`. A file that cannot be named back because
/// the name was taken again stays under its private name and the answer is `Fault`. From
/// the Keychain it finds the item with its bytes and a reference to it, compares the
/// bytes, removes that item by its reference and no other, and then looks again: no item
/// answers `Deleted`, an item with other bytes answers `Other`, and the same bytes, or a
/// look that fails, answer `Fault`, because the system gives no code for a removal by
/// reference.
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
/// makes every later call relative to that handle. A move into the Keychain removes the
/// file only after it claims it by one rename, which never replaces a name, to a private
/// name `.<name>.<pid>.<n>.claim` in the store directory, a form of the claim's own that no
/// temporary file of a write has, and finds there the bytes that it moved; another file is
/// named back. A move that is killed after its claim, or that could not name a file back
/// because the name was taken again, leaves the file under that private name, where no
/// read gives it. The next operation of `Store::new` ends it: before it reads the file, it
/// lists the store directory, by the path that the handle names, for names of the claim's
/// form, and handles each such file, relative to the handle, one by one: one that holds the
/// bytes of the Keychain item is removed, which ends the move; one beside no item or other
/// bytes is put back under the store's name; one that cannot be read, removed or put back
/// makes the operation answer as for a file place that cannot be looked at. No other name
/// in the directory is touched. A store that keeps its file alone, the one of
/// `Store::in_directory` and the DPAPI store of Windows, does the same before every
/// operation and puts each such file back under the store's name, so that a
/// `delete_if_holds` that was stopped after its claim never hides a credential. A claim that
/// cannot be named back because the name is taken makes every operation answer `Fault`
/// with -1 until a removal at the person's word, which removes it.
///
/// A claim of another process that is alive while an operation runs. When an operation of
/// either store, of either system, finds no file under the store's name, as its read, as
/// its delete or its removal at the person's word (at the open or at the removal of the
/// name), as the open or the claim of `delete_if_holds`, or as the claim of a move or its
/// read of what it claimed, it lists the store directory once more before it answers
/// `NotStored`. When the listing holds the store's name or a name of the claim's form,
/// another operation was at work: the operation ends each claim as above and runs its look
/// once more (a move runs its whole look again from its read of the Keychain, so that it
/// removes the file only after a fresh read of the Keychain gives the file's bytes), and a
/// second such finding answers `Fault` with -1. A listing that shows neither the name nor a
/// claim lets the answer stand: `NotStored`, and for a move what the Keychain holds. So no
/// operation answers `NotStored` while the credential lies in another process's claim, and
/// no write stores a second credential beside one that a claim held. A file whose link
/// count reads 0 right after its open was removed while open: it is no second name, and it
/// counts as a file that is not there. While each claim is ended, a claim that is gone at
/// its open, whose link count reads 0, or that the removal or the rename back finds absent
/// by the system's own absent code was its owner's at work: it is passed over, and the
/// operation's own listing answers for what stands. A live claim that stands is put back
/// under the store's name, and its owner then answers `Fault` with -1.
///
/// The answers `Fault` with -1 that contention gives, each after the finding named: an
/// operation of either store after its second listing that holds the name or a claim; the
/// claim of `delete_if_holds` whose claimed file is gone twice; a move whose second look
/// finds the name or a claim again; `delete_if_holds` whose claim was put back by another
/// operation and that cannot name it back; and a settle that finds a claim beside a taken
/// name, after one finding (a removal at the person's word ends such a claim instead).
///
/// Absent and missing, on both systems: a read answers `NotStored` only when the store
/// directory or the item is absent under a parent that exists. A step above the store
/// directory that is missing answers `Fault`, and a write makes the store directory alone,
/// never a step above it.
///
/// Both stores of a Mac, for `Store::new`. Every macOS build searches the login keychain: a
/// build without the feature `keychain` looks for the Keychain item of `service` and `name`
/// by its attributes alone at every read, write and delete, and a build with it keeps its
/// secret there and also reads the file `<dir>/<name>`. In both builds a read that finds no
/// Keychain item and no file looks at the Keychain once more before it answers `NotStored`,
/// so a move that another process finishes between its two looks is seen.
///
/// A read of a build without the feature answers `Locked::OtherStore` when a Keychain item
/// stands, whatever the file holds; when the Keychain cannot be searched and no file stands,
/// it answers the Keychain's `Locked` or `Fault`; otherwise it reads the file.
///
/// A read of a build with the feature can move. When the file stands and the Keychain holds
/// no item, it adds the file's bytes to the Keychain (only-create), reads the item back, and
/// only when the bytes are equal removes the file, as the place above says, and answers
/// `Found`. When the add or the read back fails, or the removal is refused or finds another
/// file, it answers `Locked::Move` with that step's code and keeps the file; a removal that
/// faults, or a file that it could not name back, answers `Fault`. An add that finds an item
/// that another process added first makes the read start again once and answer what it
/// then finds; a second one answers `Locked::Move`. A removal that finds the file gone
/// answers what the Keychain holds. When the Keychain item and the file stand with equal
/// bytes, a read removes the file and answers `Found`: it finishes a move that stopped
/// there. When they stand with other bytes, every read, write and delete of both builds
/// answers `Locked::OtherStore` and changes nothing; `remove` at the person's word is the
/// one operation that leaves that state. When the Keychain item stands and the file cannot
/// be read, a read gives the item.
///
/// A write of either build first reads as above, the move included. On `NotStored` it makes
/// the only-create write in the store that the build keeps; on `Found` it answers
/// `Occupied`; on `Locked` or `Fault` it answers that and stores nothing. What this does not
/// close: two first sign-ins on the two builds at one moment can still store one each.
/// The tests that call `Store::new` on a Mac reach its login keychain; tests use
/// `Store::in_directory`.
///
/// A delete of either build first reads as above, the move included, and answers the
/// read's `Locked` or `Fault`. When the read gives the item, the delete removes it from the
/// store that the build keeps and passes on a `Locked` or a `Fault` of that removal. After
/// a removal, or when the read answered `NotStored`, it looks at the other store once more:
/// - removed, and the other store is empty: `Deleted`;
/// - removed, and the other store holds an item: `Fault` with the code -1, because the
///   removal took effect and a credential still stands;
/// - removed, and the other store cannot be looked at: `Fault` with its code, or -1 where
///   its answer carries none;
/// - nothing removed, and the other store is empty: `NotStored`;
/// - nothing removed, and the other store holds an item: `Locked::OtherStore`;
/// - nothing removed, and the other store cannot be looked at: its `Locked` or `Fault`.
///
/// `delete_if_holds` of either build reads in the same way and answers the read's `Locked`
/// or `Fault`. When the read gives other bytes than the caller's, it answers `Other` with
/// them and removes nothing. When it gives the caller's bytes, it removes them from the
/// store that the build keeps only while that store still holds them, as above, and
/// passes on its `Other`, `Locked` or `Fault`. After a removal, or when the read answered
/// `NotStored`, it answers by the table above.
///
/// `remove` at the person's word, after the app has said what it costs: `from` names the
/// file of the file store or the Keychain item. It removes whatever item that store holds,
/// with no compare, and the other store's item stays; a removal of either store's item
/// first ends each claim as the place above says, with the bytes that the Keychain item
/// holds, so a claim of a killed move of the Keychain item's bytes goes with that item.
/// A removal of the file, of the file store of `Store::new`, of `Store::in_directory` or of
/// the DPAPI store, also removes each claim of the store's form that cannot be named back
/// because the name is taken, and each temporary file of a write of the store's form
/// (`.<name>.<pid>.<n>.tmp`) in the store directory: a write that was killed leaves its
/// temporary file, which holds a whole credential that no read gives, until such a removal.
/// No other operation removes a temporary file, because a live write of another process may
/// own it; such a write beside a removal at the person's word answers `Fault` with the
/// system's absent code (2) at the rename of its temporary file and stores nothing.
/// Then it reads, the move included, and gives that
/// read beside the removal's answer, so the caller learns what then stands: in the state
/// with other bytes in both stores, a build with the feature then reads the item that
/// stays, moving the file into the Keychain where the Keychain's item was removed, and a
/// build without it reads the file where the Keychain's item was removed and answers
/// `Locked::OtherStore` while the Keychain item stays. A kind that the store never looks
/// at, `Dpapi` on macOS, answers `None`.
///
/// Two limits stay. A delete or a `delete_if_holds` of a build without the feature, or a
/// `remove` of the file by either build, whose steps all fall between a move's read of the
/// file and its add answers `Deleted` (a `remove` with its read then answering what stands
/// at that moment, possibly `NotStored`), and the move then adds the credential that it
/// read, which the next read gives. Two first sign-ins on the two
/// builds at one moment can store one each, which leaves both stores with other bytes
/// until `remove`. No order of steps closes either without one lock over the operations
/// of both builds.
///
/// `Store::in_directory` reads and writes the file alone and never searches the Keychain.
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
    /// Windows the DPAPI file `name` in `dir`. On macOS both builds also look in the other
    /// store, so every read, write and delete searches the login keychain; see the type.
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

    /// Builds with debug assertions and the features `hosted-store-tests` and `keychain`, on
    /// macOS only: the store of `Store::new`, whose Keychain fails the step `fault` of the
    /// move. A build without debug assertions refuses the feature `hosted-store-tests`.
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

    /// The kind of store that this one keeps its secret in. On macOS a store of the kind
    /// `File` that `Store::new` made still searches the Keychain at every call.
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

    /// Reads the secret: `NotStored`, `Found`, `Locked` or `Fault`. On macOS a read of the
    /// store that `Store::new` made with the feature `keychain` can move the file into the
    /// Keychain; see the type.
    pub fn read(&self) -> Read {
        match &self.place {
            #[cfg(target_os = "macos")]
            Place::File(item) => item.after_claims(Item::read),
            #[cfg(target_os = "macos")]
            Place::Mac(mac) => mac.read(),
            #[cfg(windows)]
            Place::Dpapi(item) => item.after_claims(Item::read),
        }
    }

    /// Stores `secret` when nothing stands under the name, and changes nothing otherwise.
    /// On macOS a write of the store that `Store::new` made first reads, the move included,
    /// and stores only where that read answers `NotStored`; see the type.
    pub fn write(&self, secret: &Secret) -> Write {
        match &self.place {
            #[cfg(target_os = "macos")]
            Place::File(item) => item.after_claims(|item| item.write(secret)),
            #[cfg(target_os = "macos")]
            Place::Mac(mac) => mac.write(secret),
            #[cfg(windows)]
            Place::Dpapi(item) => item.after_claims(|item| item.write(secret)),
        }
    }

    /// Removes the item that a read would give, and nothing else. On macOS a delete of the
    /// store that `Store::new` made first reads, the move included, and answers from both
    /// stores; see the type.
    pub fn delete(&self) -> Delete {
        match &self.place {
            #[cfg(target_os = "macos")]
            Place::File(item) => item.after_claims(Item::delete),
            #[cfg(target_os = "macos")]
            Place::Mac(mac) => mac.delete(),
            #[cfg(windows)]
            Place::Dpapi(item) => item.after_claims(Item::delete),
        }
    }

    /// Removes the item only while it holds `held`, the bytes that the caller read, byte
    /// for byte, and keeps any other item, whose bytes `Other` gives. On macOS a delete of
    /// the store that `Store::new` made first reads, the move included, and answers from
    /// both stores as `delete` does; see the type.
    pub fn delete_if_holds(&self, held: &Secret) -> DeleteIfHolds {
        let held = held.expose();
        match &self.place {
            #[cfg(target_os = "macos")]
            Place::File(item) => item.after_claims(|item| item.delete_if_holds(held)),
            #[cfg(target_os = "macos")]
            Place::Mac(mac) => mac.delete_if_holds(held),
            #[cfg(windows)]
            Place::Dpapi(item) => item.after_claims(|item| item.delete_if_holds(held)),
        }
    }

    /// At the person's word, after the app has said what it costs: removes whatever item
    /// the store `from` holds under the name, and then reads. `None` when this store never
    /// looks at a store of that kind, and then nothing is touched. See the type.
    pub fn remove(&self, from: StoreKind) -> Option<Removal> {
        let removed = match &self.place {
            #[cfg(target_os = "macos")]
            Place::File(item) => (from == StoreKind::File).then(|| item.remove_at_word(None)),
            #[cfg(target_os = "macos")]
            Place::Mac(mac) => mac.remove(from),
            #[cfg(windows)]
            Place::Dpapi(item) => (from == StoreKind::Dpapi).then(|| item.remove_at_word(None)),
        }?;
        Some(Removal {
            removed,
            then: self.read(),
        })
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::keychain::{ItemRef, DUPLICATE_ITEM, ITEM_NOT_FOUND};
    use std::sync::Mutex;

    /// A Keychain of one item at most, which a `Store` can hold.
    struct Held(Mutex<Vec<Vec<u8>>>);

    impl Keychain for Held {
        fn count(&self, _service: &str, _account: &str) -> Result<usize, i32> {
            Ok(self.0.lock().expect("the items").len())
        }

        fn data(&self, _service: &str, _account: &str) -> Result<Vec<u8>, i32> {
            self.0
                .lock()
                .expect("the items")
                .first()
                .cloned()
                .ok_or(ITEM_NOT_FOUND)
        }

        fn add(&self, _service: &str, _account: &str, bytes: &[u8]) -> Result<(), i32> {
            let mut items = self.0.lock().expect("the items");
            if !items.is_empty() {
                return Err(DUPLICATE_ITEM);
            }
            items.push(bytes.to_vec());
            Ok(())
        }

        fn delete(&self, _service: &str, _account: &str) -> Result<(), i32> {
            let mut items = self.0.lock().expect("the items");
            if items.is_empty() {
                return Err(ITEM_NOT_FOUND);
            }
            items.clear();
            Ok(())
        }

        fn find(&self, service: &str, account: &str) -> Result<(Vec<u8>, ItemRef), i32> {
            Ok((self.data(service, account)?, ItemRef::Fake(0)))
        }

        fn delete_item(&self, _item: ItemRef) {
            self.0.lock().expect("the items").clear();
        }
    }

    /// G4 (e): `remove` gives what then stands, here the other store's item.
    #[test]
    fn a_removal_gives_what_then_stands_beside_the_item_of_the_other_store() {
        let parent =
            std::env::temp_dir().join(format!("halvo-secrets-store-{}-then", std::process::id()));
        std::fs::create_dir_all(&parent).expect("the parent of the store directory");
        let dir = Directory::new(parent.join("store")).expect("an absolute directory");
        let name = Name::new("account").expect("a name");
        let kept: &[u8] = b"the Keychain item's credential..";
        let store = Store {
            place: mac_place(
                Keeps::Keychain,
                &dir,
                Name::new("dev.halvo.test").expect("a service"),
                name.clone(),
                Box::new(Held(Mutex::new(vec![kept.to_vec()]))),
            ),
        };
        let file = Secret::new(b"the file's other credential.....".to_vec()).expect("a secret");
        assert_eq!(file_item(&dir, name).write(&file), Write::Stored);
        let removal = store
            .remove(StoreKind::File)
            .expect("a store of this place");
        assert_eq!(removal.removed, Delete::Deleted);
        assert!(
            matches!(&removal.then, Read::Found(held) if held.expose() == kept),
            "{:?}",
            removal.then
        );
    }
}
