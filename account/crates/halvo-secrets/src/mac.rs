use crate::answer::{Delete, DeleteIfHolds, Fault, Locked, Read, StoreKind, Write};
use crate::keychain::{Keychain, KeychainItem};
use crate::place::{Claim, Codec, Fs, Item};
use crate::Secret;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Keeps {
    File,
    Keychain,
}

pub(crate) trait FileSide {
    fn read(&self) -> Read;
    fn write(&self, secret: &Secret) -> Write;
    fn delete(&self) -> Delete;
    fn remove_at_word(&self, keychain: Option<&[u8]>) -> Delete;
    fn delete_if_holds(&self, held: &[u8]) -> DeleteIfHolds;
    fn remove_if_holds(&self, bytes: &[u8]) -> Claim;
    fn settle(&self, keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>>;
    fn claimed_or_named(&self) -> Result<bool, Result<Locked, Fault>>;
}

impl<F: Fs, C: Codec> FileSide for Item<F, C> {
    fn remove_if_holds(&self, bytes: &[u8]) -> Claim {
        Item::remove_if_holds(self, bytes)
    }

    fn settle(&self, keychain: Option<&[u8]>) -> Result<(), Result<Locked, Fault>> {
        Item::settle_claimed(self, keychain)
    }

    fn claimed_or_named(&self) -> Result<bool, Result<Locked, Fault>> {
        Item::claimed_or_named(self)
    }

    fn read(&self) -> Read {
        Item::read(self)
    }

    fn write(&self, secret: &Secret) -> Write {
        Item::write(self, secret)
    }

    fn delete(&self) -> Delete {
        Item::delete(self)
    }

    fn remove_at_word(&self, keychain: Option<&[u8]>) -> Delete {
        Item::remove_at_word(self, keychain)
    }

    fn delete_if_holds(&self, held: &[u8]) -> DeleteIfHolds {
        Item::delete_if_holds(self, held)
    }
}

pub(crate) struct Mac<S, K> {
    pub(crate) keeps: Keeps,
    pub(crate) file: S,
    pub(crate) keychain: KeychainItem<K>,
}

impl<S: FileSide, K: Keychain> Mac<S, K> {
    pub(crate) fn read(&self) -> Read {
        match self.keeps {
            Keeps::File => self.read_beside_the_keychain(),
            Keeps::Keychain => self.read_and_move(),
        }
    }

    fn read_and_move(&self) -> Read {
        self.look_and_move(Again::BOTH)
    }

    fn look_and_move(&self, again: Again) -> Read {
        match self.keychain.read() {
            Read::Found(secret) => match self.settled_file(Some(secret.expose())) {
                Read::Found(other) if other.expose() == secret.expose() => {
                    self.remove_the_file(secret, again)
                }
                Read::Found(_) => Read::Locked(Locked::OtherStore),
                _ => Read::Found(secret),
            },
            Read::NotStored => match self.settled_file(None) {
                Read::Found(secret) => self.move_in(secret, again),
                Read::NotStored => self.keychain.read(),
                answer => answer,
            },
            answer => answer,
        }
    }

    fn move_in(&self, secret: Secret, again: Again) -> Read {
        match self.keychain.write(&secret) {
            Write::Stored => {}
            Write::Occupied if again.occupied => {
                return self.look_and_move(Again {
                    occupied: false,
                    ..again
                })
            }
            Write::Occupied => return not_moved(None),
            Write::Locked(locked) => return not_moved(Some(Err(locked))),
            Write::Fault(fault) => return not_moved(Some(Ok(fault))),
        }
        match self.keychain.read() {
            Read::Found(back) if back.expose() == secret.expose() => {}
            Read::Found(_) | Read::NotStored => return not_moved(None),
            Read::Locked(locked) => return not_moved(Some(Err(locked))),
            Read::Fault(fault) => return not_moved(Some(Ok(fault))),
        }
        self.remove_the_file(secret, again)
    }

    fn remove_the_file(&self, secret: Secret, again: Again) -> Read {
        match self.file.remove_if_holds(secret.expose()) {
            Claim::Removed => Read::Found(secret),
            Claim::Absent => self.keychain.read(),
            Claim::Busy if again.busy => self.look_and_move(Again {
                busy: false,
                ..again
            }),
            Claim::Busy => Read::Fault(Fault { code: -1 }),
            Claim::Other => not_moved(None),
            Claim::Stranded => Read::Fault(Fault { code: -1 }),
            Claim::Locked(locked) => not_moved(Some(Err(locked))),
            Claim::Fault(fault) => Read::Fault(fault),
        }
    }

    fn read_beside_the_keychain(&self) -> Read {
        if matches!(self.keychain_beside(), Read::Locked(Locked::OtherStore)) {
            return Read::Locked(Locked::OtherStore);
        }
        match self.settled_file(None) {
            Read::NotStored => self.keychain_beside(),
            answer => answer,
        }
    }

    fn settled_file(&self, keychain: Option<&[u8]>) -> Read {
        for last in [false, true] {
            let read = match self.file.settle(keychain) {
                Ok(()) => self.file.read(),
                Err(Ok(locked)) => return Read::Locked(locked),
                Err(Err(fault)) => return Read::Fault(fault),
            };
            if !matches!(read, Read::NotStored) {
                return read;
            }
            match self.file.claimed_or_named() {
                Ok(false) => return read,
                Ok(true) if last => return Read::Fault(Fault { code: -1 }),
                Ok(true) => {}
                Err(Ok(locked)) => return Read::Locked(locked),
                Err(Err(fault)) => return Read::Fault(fault),
            }
        }
        Read::Fault(Fault { code: -1 })
    }

    fn keychain_beside(&self) -> Read {
        match self.keychain.count() {
            Ok(0) => Read::NotStored,
            Ok(_) => Read::Locked(Locked::OtherStore),
            Err(Ok(locked)) => Read::Locked(locked),
            Err(Err(fault)) => Read::Fault(fault),
        }
    }

    pub(crate) fn write(&self, secret: &Secret) -> Write {
        match self.read() {
            Read::NotStored => match self.keeps {
                Keeps::File => self.file.write(secret),
                Keeps::Keychain => self.keychain.write(secret),
            },
            Read::Found(_) => Write::Occupied,
            Read::Locked(locked) => Write::Locked(locked),
            Read::Fault(fault) => Write::Fault(fault),
        }
    }

    pub(crate) fn delete(&self) -> Delete {
        let own = match self.read() {
            Read::Found(_) => match self.keeps {
                Keeps::File => self.file.delete(),
                Keeps::Keychain => self.keychain.delete(),
            },
            Read::NotStored => Delete::NotStored,
            Read::Locked(locked) => return Delete::Locked(locked),
            Read::Fault(fault) => return Delete::Fault(fault),
        };
        self.answer_from_both(own)
    }

    pub(crate) fn delete_if_holds(&self, held: &[u8]) -> DeleteIfHolds {
        let step = match self.read() {
            Read::Found(stands) if stands.expose() == held => match self.keeps {
                Keeps::File => self.file.delete_if_holds(held),
                Keeps::Keychain => self.keychain.delete_if_holds(held),
            },
            Read::Found(stands) => return DeleteIfHolds::Other(stands),
            Read::NotStored => DeleteIfHolds::NotStored,
            Read::Locked(locked) => return DeleteIfHolds::Locked(locked),
            Read::Fault(fault) => return DeleteIfHolds::Fault(fault),
        };
        match step {
            DeleteIfHolds::Deleted => self.answer_from_both(Delete::Deleted).into(),
            DeleteIfHolds::NotStored => self.answer_from_both(Delete::NotStored).into(),
            answer => answer,
        }
    }

    pub(crate) fn remove(&self, from: StoreKind) -> Option<Delete> {
        if from == StoreKind::Dpapi {
            return None;
        }
        let keychain = self.keychain.read();
        let moved = match &keychain {
            Read::Found(secret) => Some(secret.expose()),
            _ => None,
        };
        if from == StoreKind::File {
            return Some(self.file.remove_at_word(moved));
        }
        Some(match self.file.settle(moved) {
            Ok(()) => self.keychain.delete(),
            Err(Ok(locked)) => Delete::Locked(locked),
            Err(Err(fault)) => Delete::Fault(fault),
        })
    }

    fn answer_from_both(&self, own: Delete) -> Delete {
        match own {
            Delete::Deleted => match self.other_place() {
                Other::Empty => Delete::Deleted,
                Other::Holds => Delete::Fault(Fault { code: -1 }),
                Other::Locked(Locked::Refused { code }) => Delete::Fault(Fault { code }),
                Other::Locked(_) => Delete::Fault(Fault { code: -1 }),
                Other::Fault(fault) => Delete::Fault(fault),
            },
            Delete::NotStored => match self.other_place() {
                Other::Empty => Delete::NotStored,
                Other::Holds => Delete::Locked(Locked::OtherStore),
                Other::Locked(locked) => Delete::Locked(locked),
                Other::Fault(fault) => Delete::Fault(fault),
            },
            answer => answer,
        }
    }

    fn other_place(&self) -> Other {
        match self.keeps {
            Keeps::File => match self.keychain.count() {
                Ok(0) => Other::Empty,
                Ok(_) => Other::Holds,
                Err(Ok(locked)) => Other::Locked(locked),
                Err(Err(fault)) => Other::Fault(fault),
            },
            Keeps::Keychain => match self.settled_file(None) {
                Read::NotStored => Other::Empty,
                Read::Found(_) => Other::Holds,
                Read::Locked(locked) => Other::Locked(locked),
                Read::Fault(fault) => Other::Fault(fault),
            },
        }
    }
}

#[derive(Clone, Copy)]
struct Again {
    occupied: bool,
    busy: bool,
}

impl Again {
    const BOTH: Again = Again {
        occupied: true,
        busy: true,
    };
}

enum Other {
    Empty,
    Holds,
    Locked(Locked),
    Fault(Fault),
}

fn not_moved(failed: Option<Result<Fault, Locked>>) -> Read {
    let code = match failed {
        Some(Ok(fault)) => fault.code,
        Some(Err(Locked::Refused { code })) => code,
        Some(Err(_)) | None => -1,
    };
    Read::Locked(Locked::Move { code })
}

#[cfg(test)]
mod tests;
