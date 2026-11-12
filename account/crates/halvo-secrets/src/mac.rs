use crate::answer::{Delete, Fault, Locked, Read, Write};
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
    fn remove_if_holds(&self, bytes: &[u8]) -> Claim;
}

impl<F: Fs, C: Codec> FileSide for Item<F, C> {
    fn remove_if_holds(&self, bytes: &[u8]) -> Claim {
        Item::remove_if_holds(self, bytes)
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
        match self.keychain.read() {
            Read::Found(secret) => match self.file.read() {
                Read::Found(other) if other.expose() == secret.expose() => {
                    self.remove_the_file(secret)
                }
                Read::Found(_) => Read::Locked(Locked::OtherStore),
                _ => Read::Found(secret),
            },
            Read::NotStored => match self.file.read() {
                Read::Found(secret) => self.move_in(secret),
                Read::NotStored => self.keychain.read(),
                answer => answer,
            },
            answer => answer,
        }
    }

    fn move_in(&self, secret: Secret) -> Read {
        match self.keychain.write(&secret) {
            Write::Stored => {}
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
        self.remove_the_file(secret)
    }

    fn remove_the_file(&self, secret: Secret) -> Read {
        match self.file.remove_if_holds(secret.expose()) {
            Claim::Removed => Read::Found(secret),
            Claim::Absent | Claim::Other => not_moved(None),
            Claim::Stranded => Read::Fault(Fault { code: -1 }),
            Claim::Locked(locked) => not_moved(Some(Err(locked))),
            Claim::Fault(fault) => Read::Fault(fault),
        }
    }

    fn read_beside_the_keychain(&self) -> Read {
        if matches!(self.keychain_beside(), Read::Locked(Locked::OtherStore)) {
            return Read::Locked(Locked::OtherStore);
        }
        match self.file.read() {
            Read::NotStored => self.keychain_beside(),
            answer => answer,
        }
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
        match self.keeps {
            Keeps::File => self.file.write(secret),
            Keeps::Keychain => self.keychain.write(secret),
        }
    }

    pub(crate) fn delete(&self) -> Delete {
        match self.read() {
            Read::Found(_) => match self.keeps {
                Keeps::File => self.file.delete(),
                Keeps::Keychain => self.keychain.delete(),
            },
            Read::NotStored => Delete::NotStored,
            Read::Locked(locked) => Delete::Locked(locked),
            Read::Fault(fault) => Delete::Fault(fault),
        }
    }
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
