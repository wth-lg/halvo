use crate::answer::{Delete, Locked, Read, Write};
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
            Keeps::Keychain => self.keychain.read(),
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

#[cfg(test)]
mod tests;
