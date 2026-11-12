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
    pub(crate) acl: bool,
    pub(crate) acl_write: bool,
    pub(crate) links: u64,
}

impl Meta {
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn of_kind(kind: EntryKind) -> Meta {
        Meta {
            kind,
            owner: 0,
            mode: 0,
            len: 0,
            acl: false,
            acl_write: false,
            links: 1,
        }
    }
}

pub(crate) enum Last<D> {
    Absent,
    Found(Meta, Option<D>),
}

pub(crate) struct Walk<D> {
    pub(crate) ancestors: Vec<Meta>,
    pub(crate) last: Last<D>,
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
    type Dir;
    type Item;
    type Temp;
    fn user(&self) -> u32;
    fn walk(&self, path: &Path) -> io::Result<Walk<Self::Dir>>;
    fn make_dir(&self, path: &Path) -> io::Result<()>;
    fn exists(&self, dir: &Self::Dir, name: &str) -> io::Result<bool>;
    fn open_item(&self, dir: &Self::Dir, name: &str) -> io::Result<Opened<Self::Item>>;
    fn item_meta(&self, item: &Self::Item) -> io::Result<Meta>;
    fn read_item(&self, item: &mut Self::Item, buf: &mut [u8]) -> io::Result<usize>;
    fn create_temp(&self, dir: &Self::Dir, name: &str) -> io::Result<Option<Self::Temp>>;
    fn write_temp(&self, temp: &mut Self::Temp, bytes: &[u8]) -> io::Result<()>;
    fn sync_temp(&self, temp: &Self::Temp) -> io::Result<()>;
    fn publish(&self, dir: &Self::Dir, from: &str, to: &str) -> io::Result<Published>;
    fn remove(&self, dir: &Self::Dir, name: &str) -> io::Result<()>;
    fn sync_dir(&self, dir: &Self::Dir) -> io::Result<()>;
}

pub(crate) enum Sealed<'a> {
    #[cfg_attr(windows, allow(dead_code))]
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

#[cfg_attr(windows, allow(dead_code))]
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
    #[cfg_attr(windows, allow(dead_code))]
    Private,
    #[cfg_attr(not(windows), allow(dead_code))]
    KindOnly,
}

impl Rules {
    pub(crate) fn ancestor(self, meta: &Meta, user: u32) -> Option<Locked> {
        if meta.kind != EntryKind::Directory {
            return Some(Locked::Ancestor);
        }
        if self == Rules::Private {
            if meta.owner != 0 && meta.owner != user {
                return Some(Locked::Ancestor);
            }
            if meta.mode & 0o022 != 0 && meta.mode & 0o1000 == 0 {
                return Some(Locked::Ancestor);
            }
        }
        if meta.acl_write {
            return Some(Locked::Ancestor);
        }
        None
    }

    pub(crate) fn dir(self, meta: &Meta, user: u32) -> Option<Locked> {
        if meta.kind != EntryKind::Directory {
            return Some(Locked::NotADirectory);
        }
        if meta.owner != user {
            return Some(Locked::Owner);
        }
        if self == Rules::Private {
            if meta.mode & 0o077 != 0 {
                return Some(Locked::Mode);
            }
            if meta.acl {
                return Some(Locked::Acl);
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
            if meta.acl {
                return Some(Locked::Acl);
            }
            if meta.links != 1 {
                return Some(Locked::HardLink);
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

#[derive(Debug)]
pub(crate) struct Code(pub(crate) i32);

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a failure with the code {}", self.0)
    }
}

impl std::error::Error for Code {}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn fault_with(code: i32) -> io::Error {
    io::Error::other(Code(code))
}

enum Stop {
    Locked(Locked),
    Fault(Fault),
}

impl Stop {
    fn of(error: &io::Error) -> Stop {
        let fault = Stop::fault(error);
        if error.kind() == io::ErrorKind::PermissionDenied {
            Stop::Locked(Locked::Refused { code: fault.code })
        } else {
            Stop::Fault(fault)
        }
    }

    fn fault(error: &io::Error) -> Fault {
        let carried = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<Code>())
            .map(|code| code.0);
        Fault {
            code: error.raw_os_error().or(carried).map_or(-1, i64::from),
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
    fn place(&self) -> Result<Option<F::Dir>, Stop> {
        let walk = self.fs.walk(&self.dir).map_err(|error| Stop::of(&error))?;
        let user = self.fs.user();
        for meta in &walk.ancestors {
            if let Some(locked) = self.rules.ancestor(meta, user) {
                return Err(Stop::Locked(locked));
            }
        }
        match walk.last {
            Last::Absent => Ok(None),
            Last::Found(meta, dir) => {
                if let Some(locked) = self.rules.dir(&meta, user) {
                    return Err(Stop::Locked(locked));
                }
                dir.map(Some).ok_or(Stop::Locked(Locked::NotADirectory))
            }
        }
    }

    fn open(&self, dir: &F::Dir) -> Result<Option<(F::Item, Meta)>, Stop> {
        let item = match self.fs.open_item(dir, self.name.as_str()) {
            Ok(Opened::Item(item)) => item,
            Ok(Opened::Absent) => return Ok(None),
            Ok(Opened::NotAFile) => return Err(Stop::Locked(Locked::NotAFile)),
            Err(error) => return Err(Stop::of(&error)),
        };
        let meta = self.fs.item_meta(&item).map_err(|error| Stop::of(&error))?;
        match self.rules.item(&meta, self.fs.user(), self.codec.limit()) {
            Some(locked) => Err(Stop::Locked(locked)),
            None => Ok(Some((item, meta))),
        }
    }

    fn temp(&self, dir: &F::Dir) -> io::Result<Option<(String, F::Temp)>> {
        let pid = std::process::id();
        for n in 0..TEMP_TRIES {
            let name = format!(".{}.{pid}.{n}.tmp", self.name.as_str());
            if let Some(temp) = self.fs.create_temp(dir, &name)? {
                return Ok(Some((name, temp)));
            }
        }
        Ok(None)
    }

    pub(crate) fn read(&self) -> Read {
        let dir = match self.place() {
            Ok(Some(dir)) => dir,
            Ok(None) => return Read::NotStored,
            Err(stop) => return stop.into(),
        };
        let (mut item, meta) = match self.open(&dir) {
            Ok(Some(opened)) => opened,
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
        if filled as u64 != meta.len {
            return Read::Locked(Locked::Size);
        }
        match self.codec.open(buffer.into_vec(filled)) {
            Ok(secret) => Read::Found(secret),
            Err(locked) => Read::Locked(locked),
        }
    }

    pub(crate) fn write(&self, secret: &Secret) -> Write {
        let dir = match self.place() {
            Ok(Some(dir)) => dir,
            Ok(None) => {
                if let Err(error) = self.fs.make_dir(&self.dir) {
                    return Stop::of(&error).into();
                }
                match self.place() {
                    Ok(Some(dir)) => dir,
                    Ok(None) => return Write::Fault(Fault { code: -1 }),
                    Err(stop) => return stop.into(),
                }
            }
            Err(stop) => return stop.into(),
        };
        let name = self.name.as_str();
        match self.fs.exists(&dir, name) {
            Ok(false) => {}
            Ok(true) => return Write::Occupied,
            Err(error) => return Stop::of(&error).into(),
        }
        let sealed = match self.codec.seal(secret) {
            Ok(sealed) => sealed,
            Err(fault) => return Write::Fault(fault),
        };
        let (temp_name, mut temp) = match self.temp(&dir) {
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
        let published = written.and_then(|()| self.fs.publish(&dir, &temp_name, name));
        match published {
            Ok(Published::Done) => {}
            Ok(Published::Exists) => {
                let _ = self.fs.remove(&dir, &temp_name);
                return Write::Occupied;
            }
            Err(error) => {
                let _ = self.fs.remove(&dir, &temp_name);
                return Stop::of(&error).into();
            }
        }
        match self.fs.sync_dir(&dir) {
            Ok(()) => Write::Stored,
            Err(error) => Write::Fault(Stop::fault(&error)),
        }
    }

    pub(crate) fn delete(&self) -> Delete {
        let dir = match self.place() {
            Ok(Some(dir)) => dir,
            Ok(None) => return Delete::NotStored,
            Err(stop) => return stop.into(),
        };
        match self.open(&dir) {
            Ok(Some(_)) => {}
            Ok(None) => return Delete::NotStored,
            Err(stop) => return stop.into(),
        }
        match self.fs.remove(&dir, self.name.as_str()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Delete::NotStored,
            Err(error) => return Stop::of(&error).into(),
        }
        match self.fs.sync_dir(&dir) {
            Ok(()) => Delete::Deleted,
            Err(error) => Delete::Fault(Stop::fault(&error)),
        }
    }
}

#[cfg(test)]
mod tests;
