use crate::answer::{Delete, Fault, Locked, Read, Write};
use crate::secret::Buffer;
use crate::{Name, Secret, MAX_SECRET_LEN};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Directory,
    Link,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Meta {
    pub(crate) kind: EntryKind,
    pub(crate) owner: u32,
    pub(crate) mode: u32,
    pub(crate) len: u64,
}

pub(crate) enum Opened<H> {
    Item(H),
    Absent,
    NotAFile,
}

pub(crate) enum Published {
    Done,
    Exists,
}

pub(crate) trait Fs {
    type Item;
    type Temp;
    fn user(&self) -> u32;
    fn lstat(&self, path: &Path) -> io::Result<Option<Meta>>;
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    fn open_item(&self, path: &Path) -> io::Result<Opened<Self::Item>>;
    fn item_meta(&self, item: &Self::Item) -> io::Result<Meta>;
    fn read_item(&self, item: &mut Self::Item, buf: &mut [u8]) -> io::Result<usize>;
    fn create_temp(&self, path: &Path) -> io::Result<Option<Self::Temp>>;
    fn write_temp(&self, temp: &mut Self::Temp, bytes: &[u8]) -> io::Result<()>;
    fn sync_temp(&self, temp: &Self::Temp) -> io::Result<()>;
    fn publish(&self, from: &Path, to: &Path) -> io::Result<Published>;
    fn remove(&self, path: &Path) -> io::Result<()>;
    fn sync_dir(&self, path: &Path) -> io::Result<()>;
}

pub(crate) enum Sealed<'a> {
    Plain(&'a [u8]),
    #[cfg_attr(not(windows), allow(dead_code))]
    Blob(Vec<u8>),
}

impl Sealed<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Sealed::Plain(bytes) => bytes,
            Sealed::Blob(bytes) => bytes,
        }
    }
}

pub(crate) trait Codec {
    fn seal<'a>(&self, secret: &'a Secret) -> Result<Sealed<'a>, Fault>;
    fn open(&self, bytes: Vec<u8>) -> Result<Secret, Locked>;
    fn limit(&self) -> usize;
}

pub(crate) struct Plain;

impl Codec for Plain {
    fn seal<'a>(&self, secret: &'a Secret) -> Result<Sealed<'a>, Fault> {
        Ok(Sealed::Plain(secret.expose()))
    }

    fn open(&self, bytes: Vec<u8>) -> Result<Secret, Locked> {
        Secret::new(bytes).ok_or(Locked::Size)
    }

    fn limit(&self) -> usize {
        MAX_SECRET_LEN
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rules {
    Private,
    #[cfg_attr(not(windows), allow(dead_code))]
    KindOnly,
}

impl Rules {
    pub(crate) fn dir(self, meta: &Meta, user: u32) -> Option<Locked> {
        if meta.kind != EntryKind::Directory {
            return Some(Locked::NotADirectory);
        }
        if self == Rules::Private {
            if meta.owner != user {
                return Some(Locked::Owner);
            }
            if meta.mode & 0o077 != 0 {
                return Some(Locked::Mode);
            }
        }
        None
    }

    pub(crate) fn item(self, meta: &Meta, user: u32, limit: usize) -> Option<Locked> {
        if meta.kind != EntryKind::File {
            return Some(Locked::NotAFile);
        }
        if self == Rules::Private {
            if meta.owner != user {
                return Some(Locked::Owner);
            }
            if meta.mode & 0o7177 != 0 {
                return Some(Locked::Mode);
            }
        }
        if meta.len == 0 || meta.len > limit as u64 {
            return Some(Locked::Size);
        }
        None
    }
}

pub(crate) struct Item<F, C> {
    pub(crate) fs: F,
    pub(crate) codec: C,
    pub(crate) rules: Rules,
    pub(crate) dir: PathBuf,
    pub(crate) name: Name,
}

const TEMP_TRIES: u32 = 100;

enum Stop {
    Locked(Locked),
    Fault(Fault),
}

impl Stop {
    fn of(error: &io::Error) -> Stop {
        let code = error.raw_os_error().map_or(-1, i64::from);
        if error.kind() == io::ErrorKind::PermissionDenied {
            Stop::Locked(Locked::Refused { code })
        } else {
            Stop::Fault(Fault { code })
        }
    }
}

impl From<Stop> for Read {
    fn from(stop: Stop) -> Read {
        match stop {
            Stop::Locked(locked) => Read::Locked(locked),
            Stop::Fault(fault) => Read::Fault(fault),
        }
    }
}

impl From<Stop> for Write {
    fn from(stop: Stop) -> Write {
        match stop {
            Stop::Locked(locked) => Write::Locked(locked),
            Stop::Fault(fault) => Write::Fault(fault),
        }
    }
}

impl From<Stop> for Delete {
    fn from(stop: Stop) -> Delete {
        match stop {
            Stop::Locked(locked) => Delete::Locked(locked),
            Stop::Fault(fault) => Delete::Fault(fault),
        }
    }
}

impl<F: Fs, C: Codec> Item<F, C> {
    fn path(&self) -> PathBuf {
        self.dir.join(self.name.as_str())
    }

    fn dir_ready(&self) -> Result<bool, Stop> {
        match self.fs.lstat(&self.dir) {
            Ok(None) => Ok(false),
            Ok(Some(meta)) => match self.rules.dir(&meta, self.fs.user()) {
                Some(locked) => Err(Stop::Locked(locked)),
                None => Ok(true),
            },
            Err(error) => Err(Stop::of(&error)),
        }
    }

    fn open(&self) -> Result<Option<F::Item>, Stop> {
        let item = match self.fs.open_item(&self.path()) {
            Ok(Opened::Item(item)) => item,
            Ok(Opened::Absent) => return Ok(None),
            Ok(Opened::NotAFile) => return Err(Stop::Locked(Locked::NotAFile)),
            Err(error) => return Err(Stop::of(&error)),
        };
        let meta = self.fs.item_meta(&item).map_err(|error| Stop::of(&error))?;
        match self.rules.item(&meta, self.fs.user(), self.codec.limit()) {
            Some(locked) => Err(Stop::Locked(locked)),
            None => Ok(Some(item)),
        }
    }

    fn temp(&self) -> io::Result<Option<(PathBuf, F::Temp)>> {
        let pid = std::process::id();
        for n in 0..TEMP_TRIES {
            let path = self
                .dir
                .join(format!(".{}.{pid}.{n}.tmp", self.name.as_str()));
            if let Some(temp) = self.fs.create_temp(&path)? {
                return Ok(Some((path, temp)));
            }
        }
        Ok(None)
    }

    pub(crate) fn read(&self) -> Read {
        match self.dir_ready() {
            Ok(true) => {}
            Ok(false) => return Read::NotStored,
            Err(stop) => return stop.into(),
        }
        let mut item = match self.open() {
            Ok(Some(item)) => item,
            Ok(None) => return Read::NotStored,
            Err(stop) => return stop.into(),
        };
        let mut buffer = Buffer::zeroed(self.codec.limit() + 1);
        let mut filled = 0;
        while filled < buffer.capacity() {
            match self.fs.read_item(&mut item, &mut buffer.space()[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(error) => return Stop::of(&error).into(),
            }
        }
        if filled == 0 || filled > self.codec.limit() {
            return Read::Locked(Locked::Size);
        }
        match self.codec.open(buffer.into_vec(filled)) {
            Ok(secret) => Read::Found(secret),
            Err(locked) => Read::Locked(locked),
        }
    }

    pub(crate) fn write(&self, secret: &Secret) -> Write {
        match self.dir_ready() {
            Ok(true) => {}
            Ok(false) => {
                if let Err(error) = self.fs.create_dir(&self.dir) {
                    return Stop::of(&error).into();
                }
                match self.dir_ready() {
                    Ok(true) => {}
                    Ok(false) => return Write::Fault(Fault { code: -1 }),
                    Err(stop) => return stop.into(),
                }
            }
            Err(stop) => return stop.into(),
        }
        let path = self.path();
        match self.fs.lstat(&path) {
            Ok(None) => {}
            Ok(Some(_)) => return Write::Occupied,
            Err(error) => return Stop::of(&error).into(),
        }
        let sealed = match self.codec.seal(secret) {
            Ok(sealed) => sealed,
            Err(fault) => return Write::Fault(fault),
        };
        let (temp_path, mut temp) = match self.temp() {
            Ok(Some(made)) => made,
            Ok(None) => return Write::Fault(Fault { code: -1 }),
            Err(error) => return Stop::of(&error).into(),
        };
        let written = self
            .fs
            .write_temp(&mut temp, sealed.bytes())
            .and_then(|()| self.fs.sync_temp(&temp));
        drop(temp);
        drop(sealed);
        let published = written.and_then(|()| self.fs.publish(&temp_path, &path));
        match published {
            Ok(Published::Done) => {}
            Ok(Published::Exists) => {
                let _ = self.fs.remove(&temp_path);
                return Write::Occupied;
            }
            Err(error) => {
                let _ = self.fs.remove(&temp_path);
                return Stop::of(&error).into();
            }
        }
        match self.fs.sync_dir(&self.dir) {
            Ok(()) => Write::Stored,
            Err(error) => Stop::of(&error).into(),
        }
    }

    pub(crate) fn delete(&self) -> Delete {
        match self.dir_ready() {
            Ok(true) => {}
            Ok(false) => return Delete::NotStored,
            Err(stop) => return stop.into(),
        }
        match self.open() {
            Ok(Some(_)) => {}
            Ok(None) => return Delete::NotStored,
            Err(stop) => return stop.into(),
        }
        match self.fs.remove(&self.path()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Delete::NotStored,
            Err(error) => return Stop::of(&error).into(),
        }
        match self.fs.sync_dir(&self.dir) {
            Ok(()) => Delete::Deleted,
            Err(error) => Stop::of(&error).into(),
        }
    }
}

#[cfg(test)]
mod tests;
