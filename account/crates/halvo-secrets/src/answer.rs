use crate::Secret;
use std::fmt;

/// The kind of store that a `Store` reads and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreKind {
    /// A generic password item of the macOS Keychain.
    Keychain,
    /// A file of mode 0600 in a directory of mode 0700, on macOS.
    File,
    /// A file that holds the secret sealed by DPAPI in the scope of the user, on Windows.
    Dpapi,
}

impl StoreKind {
    /// `keychain`, `file` or `dpapi`.
    pub fn as_str(self) -> &'static str {
        match self {
            StoreKind::Keychain => "keychain",
            StoreKind::File => "file",
            StoreKind::Dpapi => "dpapi",
        }
    }
}

/// What a read answers: one of these four and nothing else. A read that fails is never
/// `NotStored`, so a caller that makes a secret only on `NotStored` never makes a second one.
#[must_use]
pub enum Read {
    /// No item stands under the name.
    NotStored,
    /// The item, read whole.
    Found(Secret),
    /// An item, or something in its place, stands under the name, and the store does not
    /// give it.
    Locked(Locked),
    /// The system failed with its code; whether an item is stored is not known.
    Fault(Fault),
}

impl fmt::Debug for Read {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Read::NotStored => f.write_str("NotStored"),
            Read::Found(_) => f.write_str("Found(..)"),
            Read::Locked(locked) => f.debug_tuple("Locked").field(locked).finish(),
            Read::Fault(fault) => f.debug_tuple("Fault").field(fault).finish(),
        }
    }
}

/// What a write answers: one of these four and nothing else. A write only creates: it
/// never replaces or changes what stands under the name.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Write {
    /// The secret is stored under the name, which held nothing before.
    Stored,
    /// Something already stands under the name; nothing was changed.
    Occupied,
    /// The store refused the write; nothing was stored.
    Locked(Locked),
    /// The system failed with its code; whether the secret is stored is not known.
    Fault(Fault),
}

/// What a delete answers: one of these four and nothing else. A delete removes the one
/// item that a read of the same name would give, and nothing else.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delete {
    /// The item stood under the name and is removed.
    Deleted,
    /// No item stood under the name.
    NotStored,
    /// Something stands under the name that this store does not remove.
    Locked(Locked),
    /// The system failed with its code; whether the item is removed is not known.
    Fault(Fault),
}

/// Why a store does not give, take or remove what stands under a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locked {
    /// The store refused the call with its code: an OSStatus of the Keychain, an errno of
    /// macOS, or an error code of Windows.
    Refused {
        /// The code of the refusal.
        code: i64,
    },
    /// The file has a permission bit outside 0600, or a setuid, setgid or sticky bit, or
    /// its directory has a permission bit for the group or for others.
    Mode,
    /// The file or its directory belongs to another user.
    Owner,
    /// The file or its directory carries an access control list, which can open it to
    /// others whatever its mode says.
    Acl,
    /// The file has a name other than this one too: its link count is not 1.
    HardLink,
    /// Something other than a regular file, such as a link or a directory, stands under the
    /// name.
    NotAFile,
    /// Something other than a directory, such as a link or a file, stands in the place of
    /// the directory.
    NotADirectory,
    /// A directory on the way to the store's directory can be changed by someone other
    /// than the person and root: it belongs to another user, it lets the group or others
    /// write without the sticky bit, or an allow entry of its access control list lets
    /// another user or a group add, remove or rename its entries; or a link on the way
    /// belongs to another user; or a step on the way is no directory and no link.
    Ancestor,
    /// The item holds no byte or more than the store keeps.
    Size,
    /// More than one Keychain item answers to the name.
    Duplicate,
    /// On macOS, the store that this build does not keep its secret in holds an item under
    /// the name too: a build without the feature `keychain` found a Keychain item, or a
    /// build with it found the file of the file store beside its own Keychain item, with
    /// other bytes. The store picks neither, and no operation of this crate leaves the
    /// state in which both stores hold other bytes.
    OtherStore,
    /// On macOS, a build with the feature `keychain` found the file of the file store, and
    /// the move of that file into the Keychain did not finish because a step of it failed:
    /// the add or the read back failed, the removal of the file was refused or found another
    /// file under the name, or a second add found an item that another process had added.
    /// The file is kept. This is the one member after which the Keychain may hold the item
    /// that the add made; a later read that finds equal bytes in both stores finishes the
    /// move.
    Move {
        /// The OSStatus or errno with which that step was refused or failed, or -1 when the
        /// step answered without one: a second add found an item, the read back found none
        /// or other bytes, the removal found another file, or the step was refused with
        /// another member.
        code: i64,
    },
}

/// A failure of the system with its code: an OSStatus of the Keychain, an errno of macOS,
/// an error code of Windows, or -1 for a failure that carries no code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    /// The code of the failure.
    pub code: i64,
}
