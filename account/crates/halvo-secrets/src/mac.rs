use crate::answer::{Delete, Fault, Locked, Read, Write};
use crate::keychain::{Keychain, KeychainItem};
use crate::place::{Codec, Fs, Item};
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
}

impl<F: Fs, C: Codec> FileSide for Item<F, C> {
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
                Read::Found(_) => Read::Locked(Locked::OtherStore),
                _ => Read::Found(secret),
            },
            Read::NotStored => match self.file.read() {
                Read::Found(secret) => self.move_in(secret),
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
        match self.file.delete() {
            Delete::Deleted => Read::Found(secret),
            Delete::NotStored => not_moved(None),
            Delete::Locked(locked) => not_moved(Some(Err(locked))),
            Delete::Fault(fault) => not_moved(Some(Ok(fault))),
        }
    }

    fn read_beside_the_keychain(&self) -> Read {
        match self.keychain.count() {
            Ok(0) => self.file.read(),
            Ok(_) => Read::Locked(Locked::OtherStore),
            Err(failed) => match self.file.read() {
                Read::NotStored => match failed {
                    Ok(locked) => Read::Locked(locked),
                    Err(fault) => Read::Fault(fault),
                },
                answer => answer,
            },
        }
    }

    pub(crate) fn write(&self, secret: &Secret) -> Write {
        match self.keeps {
            Keeps::File => self.file.write(secret),
            Keeps::Keychain => self.keychain.write(secret),
        }
    }

    pub(crate) fn delete(&self) -> Delete {
        match self.keeps {
            Keeps::File => self.file.delete(),
            Keeps::Keychain => self.keychain.delete(),
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
