use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use super::dev_fallback::{aead_open, aead_seal, fill_random, store_key_from};
use super::{associated_data, CustodyError, KeyLabel, RootId, StoreKey, WrappedKey};
use crate::store::durable::{self, Dir, EntryKind};
use crate::store::RootReading;

const ROOT_EXT: &str = "root";
const PARTIAL_EXT: &str = "root.tmp";

pub(crate) trait Protector {
    fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError>;
    fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError>;
}

pub(crate) struct FileRoots<P> {
    dir: PathBuf,
    protector: P,
}

fn read_error(e: std::io::Error) -> CustodyError {
    match e.kind() {
        ErrorKind::NotFound => CustodyError::RootMissing,
        _ if durable::refusal(&e).is_some() => CustodyError::RootCorrupt,
        _ => root_io(e),
    }
}

fn root_io(e: std::io::Error) -> CustodyError {
    match e.kind() {
        ErrorKind::PermissionDenied => CustodyError::RootDenied,
        _ => CustodyError::Os(e.raw_os_error()),
    }
}

fn root_name(root_id: RootId) -> OsString {
    OsString::from(format!("{root_id}.{ROOT_EXT}"))
}

fn partial_name(root_id: RootId) -> Result<OsString, CustodyError> {
    let mut tag = [0u8; 8];
    fill_random(&mut tag)?;
    let tag: String = tag.iter().map(|b| format!("{b:02x}")).collect();
    Ok(OsString::from(format!("{root_id}.{tag}.{PARTIAL_EXT}")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootFile {
    Root(RootId),
    Partial(RootId),
}

impl RootFile {
    pub(crate) fn named(name: &OsStr) -> Option<Self> {
        let name = name.to_str()?;
        if let Some(stem) = name.strip_suffix(&format!(".{PARTIAL_EXT}")) {
            let (id, tag) = stem.split_once('.')?;
            let tagged = tag.len() == 16 && tag.bytes().all(|b| b.is_ascii_hexdigit());
            return match RootId::parse(id) {
                Some(root_id) if tagged && id.len() == 32 => Some(RootFile::Partial(root_id)),
                _ => None,
            };
        }
        let id = name.strip_suffix(&format!(".{ROOT_EXT}"))?;
        match RootId::parse(id) {
            Some(root_id) if id.len() == 32 => Some(RootFile::Root(root_id)),
            _ => None,
        }
    }

    /// The root file that a name of any case names: the name with its ASCII
    /// letters lowered, read as [`RootFile::named`] reads it.
    pub(crate) fn named_in_any_case(name: &OsStr) -> Option<Self> {
        let lowered = name.to_str()?.to_ascii_lowercase();
        Self::named(OsStr::new(&lowered))
    }

    fn id(self) -> RootId {
        match self {
            RootFile::Root(id) | RootFile::Partial(id) => id,
        }
    }
}

/// Zeroes the entry `name` of `dir` when it is a regular file and removes it,
/// a link or an entry of another kind as the entry that it is, never opened:
/// answers the removal's result.
pub(crate) fn scrub_and_remove(dir: &Dir, name: &OsStr) -> std::io::Result<()> {
    match dir.entry(name) {
        Ok(entry) if entry.kind == EntryKind::File => {
            let _ = zero_fill(dir, name);
        }
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    }
    match dir.remove_entry(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn zero_fill(dir: &Dir, name: &OsStr) -> std::io::Result<()> {
    let mut file = dir.open_file_for_write(name)?;
    let len = file.metadata()?.len() as usize;
    file.write_all(&vec![0u8; len])?;
    file.sync_all()
}

fn seal(
    root_id: RootId,
    label: KeyLabel,
    root: Zeroizing<[u8; 32]>,
    key: &StoreKey,
) -> Result<WrappedKey, CustodyError> {
    let sealed = aead_seal(&root, key.expose(), &associated_data(root_id, label))
        .map_err(|_| CustodyError::Crypto("sealing the store key failed".to_string()))?;
    Ok(WrappedKey(sealed))
}

/// What the destruction of every root file under a roots directory left: the
/// ids of the root files and partial root files that stay, their names as
/// they stand, and whether the directory could be listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootsLeft {
    pub left: Vec<RootId>,
    pub names: Vec<String>,
    pub listed: bool,
}

impl RootsLeft {
    pub fn none_left(&self) -> bool {
        self.listed && self.left.is_empty()
    }
}

/// The root step through one opened handle of the roots location. `named`
/// is the root that the handle, the key file or a mark that reads made names.
/// The step lists, destroys and reads again through that one handle, and it
/// destroys there only when the directory holds an entry of the named root
/// (the witness): a directory put at the old name is not touched, and a
/// roots directory that left its name is not read as destroyed. With the
/// witness every root file of the directory is destroyed, the named one
/// first. With nothing named it destroys nothing. A later call on a handle
/// whose root read destroyed does not come here (ruling ST-45 H3).
pub(crate) fn destroy_roots_in(roots: &Path, named: Option<RootId>) -> (RootReading, RootsLeft) {
    let none_counted = |listed: bool| RootsLeft {
        left: Vec::new(),
        names: Vec::new(),
        listed,
    };
    let Some(named) = named else {
        return (RootReading::Unnamed, none_counted(true));
    };
    let left_unread = (RootReading::Left(Some(named)), none_counted(false));
    #[cfg(windows)]
    if durable::has_a_part_that_windows_trims(roots) {
        return left_unread;
    }
    let Ok(dir) = Dir::open(roots) else {
        return left_unread;
    };
    let Ok(entries) = dir.entries() else {
        return left_unread;
    };
    let mut found: Vec<(RootFile, OsString)> = entries
        .into_iter()
        .filter_map(|entry| RootFile::named_in_any_case(&entry.name).map(|file| (file, entry.name)))
        .collect();
    let holds_named = found.iter().any(|(file, _)| file.id() == named);
    if !holds_named {
        return (RootReading::Left(Some(named)), none_counted(true));
    }
    found.sort_by_key(|(file, _)| file.id() != named);
    let mut every_named_entry_destroyed = true;
    for (file, name) in &found {
        let fate = destroy_root_entry(&dir, name);
        if file.id() == named && fate != EntryFate::Gone {
            every_named_entry_destroyed = false;
        }
    }
    let _ = dir.sync();
    match dir.entries() {
        Ok(entries) => {
            let mut stays: Vec<(RootId, String)> = entries
                .iter()
                .filter_map(|entry| {
                    RootFile::named_in_any_case(&entry.name)
                        .map(|file| (file.id(), entry.name.to_string_lossy().into_owned()))
                })
                .collect();
            stays.sort_by(|a, b| a.1.cmp(&b.1));
            let reading =
                if every_named_entry_destroyed && !stays.iter().any(|(id, _)| *id == named) {
                    RootReading::Destroyed
                } else {
                    RootReading::Left(Some(named))
                };
            let mut left: Vec<RootId> = stays.iter().map(|(id, _)| *id).collect();
            left.dedup();
            let names = stays.into_iter().map(|(_, name)| name).collect();
            (
                reading,
                RootsLeft {
                    left,
                    names,
                    listed: true,
                },
            )
        }
        Err(_) => (RootReading::Left(Some(named)), none_counted(false)),
    }
}

/// What the destruction of one entry of a roots directory left.
#[derive(Debug)]
enum EntryFate {
    /// The name is gone, and no name of the file keeps the key.
    Gone,
    /// The zero-fill reached every name of the file, and the name stays.
    ZeroedAndNamed,
    /// A key may stay behind the name: the removal's error when there is one.
    KeyMayStay(Option<std::io::Error>),
}

impl PartialEq for EntryFate {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// Destroys one entry of a roots directory. An entry that is no regular file
/// is removed as the entry that it is. For a regular file the store opens the
/// file, keeps that handle and makes the file writable, then zero-fills it
/// through its name, which reaches every name of the file, and removes the
/// name. If the zero-fill still fails: with one name (the count read through
/// the kept handle) the name is removed, and the entry is gone only when the
/// count read through that handle after the removal is 0; with more than one
/// name, or a count that cannot be read, the name is kept and a key may stay.
fn destroy_root_entry(dir: &Dir, name: &OsStr) -> EntryFate {
    let remove = |dir: &Dir| match dir.remove_entry(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    };
    match dir.entry(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => return EntryFate::Gone,
        Err(e) => return EntryFate::KeyMayStay(Some(e)),
        Ok(entry) if entry.kind != EntryKind::File => {
            return match remove(dir) {
                Ok(()) => EntryFate::Gone,
                Err(e) => EntryFate::KeyMayStay(Some(e)),
            }
        }
        Ok(_) => {}
    }
    let kept = match dir.open_file(name) {
        Ok(kept) => kept,
        Err(e) => return EntryFate::KeyMayStay(Some(e)),
    };
    make_writable(dir, name, &kept);
    let zeroed = matches!(dir.names_file(name, &kept), Ok(true)) && zero_fill(dir, name).is_ok();
    if zeroed {
        return match remove(dir) {
            Ok(()) => EntryFate::Gone,
            Err(_) => EntryFate::ZeroedAndNamed,
        };
    }
    match durable::file_link_count(&kept) {
        Ok(1) => match remove(dir) {
            Ok(()) if matches!(durable::file_link_count(&kept), Ok(0)) => EntryFate::Gone,
            Ok(()) => EntryFate::KeyMayStay(None),
            Err(e) => EntryFate::KeyMayStay(Some(e)),
        },
        _ => EntryFate::KeyMayStay(None),
    }
}

#[cfg(unix)]
fn make_writable(_dir: &Dir, _name: &OsStr, kept: &std::fs::File) {
    use std::os::unix::fs::PermissionsExt;
    let _ = kept.set_permissions(std::fs::Permissions::from_mode(0o600));
}

#[cfg(windows)]
fn make_writable(dir: &Dir, name: &OsStr, kept: &std::fs::File) {
    if let Ok(metadata) = kept.metadata() {
        let mut permissions = metadata.permissions();
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "a root file that the store destroys has to be writable for its zero-fill"
        )]
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(dir.path().join(name), permissions);
    }
}

pub(crate) fn root_file_may_be_present(roots: &Path, root_id: RootId) -> bool {
    match Dir::open(roots) {
        Ok(dir) => {
            !matches!(dir.entry(&root_name(root_id)), Err(e) if e.kind() == ErrorKind::NotFound)
        }
        Err(e) => e.kind() != ErrorKind::NotFound,
    }
}

pub(crate) fn destroy_root_at(roots: &Path, root_id: RootId) -> Result<(), CustodyError> {
    let dir = match Dir::open(roots) {
        Ok(dir) => dir,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) if durable::refusal(&e).is_some() => return Err(CustodyError::RootCorrupt),
        Err(e) => return Err(root_io(e)),
    };
    let mut fates = Vec::new();
    if let Ok(entries) = dir.entries() {
        for entry in entries {
            match RootFile::named_in_any_case(&entry.name) {
                Some(RootFile::Partial(id)) if id == root_id => {
                    let _ = destroy_root_entry(&dir, &entry.name);
                }
                Some(RootFile::Root(id)) if id == root_id && entry.name != root_name(root_id) => {
                    fates.push(destroy_root_entry(&dir, &entry.name));
                }
                _ => {}
            }
        }
    }
    fates.push(destroy_root_entry(&dir, &root_name(root_id)));
    let _ = dir.sync();
    for fate in fates {
        match fate {
            EntryFate::Gone | EntryFate::ZeroedAndNamed => {}
            EntryFate::KeyMayStay(Some(e)) => return Err(root_io(e)),
            EntryFate::KeyMayStay(None) => return Err(CustodyError::RootDenied),
        }
    }
    Ok(())
}

impl<P: Protector> FileRoots<P> {
    pub(crate) fn new(dir: PathBuf, protector: P) -> Self {
        Self { dir, protector }
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    #[cfg(test)]
    fn path(&self, root_id: RootId) -> PathBuf {
        self.dir.join(root_name(root_id))
    }

    fn open_private_roots_dir(&self) -> Result<Dir, CustodyError> {
        durable::create_private_dir_all(&self.dir).map_err(root_io)?;
        let dir = Dir::open(&self.dir).map_err(root_io)?;
        dir.make_private().map_err(root_io)?;
        Ok(dir)
    }

    fn existing_root(&self, root_id: RootId) -> Result<Option<Zeroizing<[u8; 32]>>, CustodyError> {
        let dir = match Dir::open(&self.dir) {
            Ok(dir) => dir,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) if durable::refusal(&e).is_some() => return Err(CustodyError::RootCorrupt),
            Err(e) => return Err(root_io(e)),
        };
        let mut blob = Zeroizing::new(Vec::new());
        let read = dir
            .open_file(&root_name(root_id))
            .map_err(read_error)
            .and_then(|mut file| file.read_to_end(&mut blob).map_err(read_error));
        match read {
            Ok(_) => {}
            Err(CustodyError::RootMissing) => return Ok(None),
            Err(e) => return Err(e),
        }
        if blob.iter().all(|&b| b == 0) {
            return Ok(None);
        }
        let plain = self.protector.unprotect(&blob, root_id.as_bytes())?;
        if plain.len() != 32 {
            return Err(CustodyError::RootCorrupt);
        }
        let mut root = Zeroizing::new([0u8; 32]);
        root.copy_from_slice(&plain);
        Ok(Some(root))
    }

    pub(crate) fn create_root(&self, root_id: RootId) -> Result<(), CustodyError> {
        match self.existing_root(root_id)? {
            Some(_) => Err(CustodyError::RootExists),
            None => self.publish_new_root_by_link(root_id).map(drop),
        }
    }

    fn publish_new_root_by_link(
        &self,
        root_id: RootId,
    ) -> Result<Zeroizing<[u8; 32]>, CustodyError> {
        let mut root = Zeroizing::new([0u8; 32]);
        fill_random(root.as_mut_slice())?;
        let blob = self
            .protector
            .protect(root.as_slice(), root_id.as_bytes())?;
        let dir = self.open_private_roots_dir()?;
        let partial = partial_name(root_id)?;
        let written = write_partial(&dir, &partial, &blob);
        let published = written.and_then(|()| {
            dir.link(&partial, &root_name(root_id))
                .map_err(|e| match e.kind() {
                    ErrorKind::AlreadyExists => CustodyError::RootExists,
                    _ => root_io(e),
                })
        });
        let discarded = match published {
            Ok(()) => dir.remove_file(&partial),
            Err(_) => scrub_and_remove(&dir, &partial),
        };
        published?;
        #[cfg(test)]
        let discarded = make_root_seams::or_failed(make_root_seams::REMOVE_THE_PARTIAL, discarded);
        let finished = discarded.and_then(|()| {
            let synced = dir.sync();
            #[cfg(test)]
            let synced = make_root_seams::or_failed(make_root_seams::SYNC_THE_DIRECTORY, synced);
            synced
        });
        if let Err(e) = finished {
            let published_root = scrub_and_remove(&dir, &root_name(root_id));
            let _ = scrub_and_remove(&dir, &partial);
            let _ = dir.sync();
            return Err(root_io(published_root.err().unwrap_or(e)));
        }
        Ok(root)
    }

    pub(crate) fn wrap(
        &self,
        root_id: RootId,
        label: KeyLabel,
        key: &StoreKey,
    ) -> Result<WrappedKey, CustodyError> {
        let root = match self.existing_root(root_id)? {
            Some(root) => root,
            None => match self.publish_new_root_by_link(root_id) {
                Err(CustodyError::RootExists) => self
                    .existing_root(root_id)?
                    .ok_or(CustodyError::RootMissing)?,
                made => made?,
            },
        };
        seal(root_id, label, root, key)
    }

    pub(crate) fn unwrap(
        &self,
        root_id: RootId,
        label: KeyLabel,
        wrapped: &WrappedKey,
    ) -> Result<StoreKey, CustodyError> {
        let root = self
            .existing_root(root_id)?
            .ok_or(CustodyError::RootMissing)?;
        let plain = aead_open(&root, &wrapped.0, &associated_data(root_id, label))
            .map_err(|_| CustodyError::WrapAuthFailed)?;
        store_key_from(plain)
    }

    pub(crate) fn destroy_root(&self, root_id: RootId) -> Result<(), CustodyError> {
        destroy_root_at(&self.dir, root_id)
    }

    #[cfg(test)]
    pub(crate) fn create_root_without_a_look(&self, root_id: RootId) -> Result<(), CustodyError> {
        self.publish_new_root_by_link(root_id).map(drop)
    }

    #[cfg(test)]
    pub(crate) fn root_exists(&self, root_id: RootId) -> Result<bool, CustodyError> {
        self.existing_root(root_id).map(|root| root.is_some())
    }

    #[cfg(test)]
    pub(crate) fn root_path(&self, root_id: RootId) -> PathBuf {
        self.path(root_id)
    }

    #[cfg(test)]
    pub(crate) fn root_bytes(
        &self,
        root_id: RootId,
    ) -> Result<Option<Zeroizing<[u8; 32]>>, CustodyError> {
        self.existing_root(root_id)
    }

    #[cfg(test)]
    pub(crate) fn replace_root(
        &self,
        root_id: RootId,
        root: &[u8; 32],
    ) -> Result<(), CustodyError> {
        let blob = self.protector.protect(root, root_id.as_bytes())?;
        let dir = self.open_private_roots_dir()?;
        let partial = partial_name(root_id)?;
        write_partial(&dir, &partial, &blob)?;
        dir.rename(&partial, &root_name(root_id)).map_err(root_io)
    }
}

fn write_partial(dir: &Dir, partial: &OsStr, blob: &[u8]) -> Result<(), CustodyError> {
    let mut file = dir.create_new(partial).map_err(root_io)?;
    file.write_all(blob).map_err(root_io)?;
    file.sync_all().map_err(root_io)
}

#[cfg(test)]
pub(crate) mod make_root_seams {
    use std::cell::Cell;

    pub(crate) const REMOVE_THE_PARTIAL: &str = "the removal of the partial name after the link";
    pub(crate) const SYNC_THE_DIRECTORY: &str = "the sync of the roots directory after the link";

    thread_local! {
        static FAIL: Cell<Option<&'static str>> = const { Cell::new(None) };
    }

    pub(crate) fn fail(step: Option<&'static str>) {
        FAIL.with(|fail| fail.set(step));
    }

    pub(crate) fn or_failed(
        step: &'static str,
        result: std::io::Result<()>,
    ) -> std::io::Result<()> {
        FAIL.with(|fail| match fail.get() {
            Some(at) if at == step => {
                fail.set(None);
                Err(std::io::Error::other(at))
            }
            _ => result,
        })
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use zeroize::Zeroizing;

    use super::{FileRoots, Protector};
    use crate::custody::{CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};

    pub(crate) struct TestProtector {
        mask: u8,
        unprotects: Arc<AtomicUsize>,
        fail_next_unprotect: Mutex<Option<CustodyError>>,
    }

    impl TestProtector {
        pub(crate) fn new(mask: u8) -> (Self, Arc<AtomicUsize>) {
            let unprotects = Arc::new(AtomicUsize::new(0));
            let protector = Self {
                mask,
                unprotects: Arc::clone(&unprotects),
                fail_next_unprotect: Mutex::new(None),
            };
            (protector, unprotects)
        }

        pub(crate) fn failing_once(mask: u8, error: CustodyError) -> (Self, Arc<AtomicUsize>) {
            let (protector, unprotects) = Self::new(mask);
            *protector.fail_next_unprotect.lock().unwrap() = Some(error);
            (protector, unprotects)
        }

        fn transform(&self, bytes: &[u8], label: &[u8]) -> Vec<u8> {
            bytes
                .iter()
                .zip(label.iter().cycle())
                .map(|(b, l)| b ^ l ^ self.mask)
                .collect()
        }
    }

    impl Protector for TestProtector {
        fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError> {
            Ok(self.transform(plain, label))
        }

        fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError> {
            self.unprotects.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.fail_next_unprotect.lock().unwrap().take() {
                return Err(error);
            }
            Ok(Zeroizing::new(self.transform(blob, label)))
        }
    }

    impl<P: Protector> Protector for Arc<P> {
        fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError> {
            P::protect(self, plain, label)
        }

        fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError> {
            P::unprotect(self, blob, label)
        }
    }

    pub(crate) struct FileRootCustody<P> {
        pub(crate) roots: FileRoots<P>,
        pub(crate) root_id: RootId,
    }

    impl<P: Protector> KeyCustody for FileRootCustody<P> {
        fn kind(&self) -> CustodyKind {
            CustodyKind::Dpapi
        }

        fn root_id(&self) -> RootId {
            self.root_id
        }

        fn roots(&self) -> Option<&std::path::Path> {
            Some(self.roots.dir())
        }

        fn wrap_store_key(
            &self,
            key: &StoreKey,
            _label: crate::custody::KeyLabel,
        ) -> Result<WrappedKey, CustodyError> {
            self.roots.wrap(self.root_id, _label, key)
        }

        fn create_root(&self) -> Result<(), CustodyError> {
            self.roots.create_root(self.root_id)
        }

        fn unwrap_store_key(
            &self,
            wrapped: &WrappedKey,
            _label: crate::custody::KeyLabel,
        ) -> Result<StoreKey, CustodyError> {
            self.roots.unwrap(self.root_id, _label, wrapped)
        }

        fn destroy_root(&self) -> Result<(), CustodyError> {
            self.roots.destroy_root(self.root_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct XorProtector(u8);

    impl Protector for XorProtector {
        fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError> {
            Ok(plain
                .iter()
                .zip(label.iter().cycle())
                .map(|(b, l)| b ^ l ^ self.0)
                .collect())
        }
        fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError> {
            self.protect(blob, label).map(Zeroizing::new)
        }
    }

    fn roots(dir: &Path) -> FileRoots<XorProtector> {
        FileRoots::new(dir.join("roots"), XorProtector(0x5a))
    }

    #[test]
    fn a_create_root_whose_step_after_the_link_fails_leaves_no_root_file_of_its_id() {
        use super::make_root_seams;
        for step in [
            make_root_seams::REMOVE_THE_PARTIAL,
            make_root_seams::SYNC_THE_DIRECTORY,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let roots = roots(dir.path());
            let root_id = RootId::fresh().unwrap();
            make_root_seams::fail(Some(step));
            let made = roots.create_root(root_id);
            make_root_seams::fail(None);
            let left: Vec<String> = std::fs::read_dir(dir.path().join("roots"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with(&root_id.to_string()))
                .collect();
            println!("MAKE-ROOT step={step} answer={made:?} left={left:?}");
            assert!(made.is_err(), "{step}: create_root answered Ok");
            assert!(left.is_empty(), "{step}: {left:?} stands");
        }
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        assert!(roots.create_root(root_id).is_ok());
        assert!(roots.root_exists(root_id).unwrap());
    }

    #[test]
    fn wrap_unwrap_roundtrips_and_the_root_file_holds_no_raw_root() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        let key = StoreKey::from_bytes([42u8; 32]);
        let wrapped = roots
            .wrap(root_id, crate::custody::KeyLabel::STORE, &key)
            .unwrap();
        assert_eq!(
            roots
                .unwrap(root_id, crate::custody::KeyLabel::STORE, &wrapped)
                .unwrap()
                .expose(),
            &[42u8; 32]
        );
        assert!(roots.root_exists(root_id).unwrap());
        let file = std::fs::read(roots.path(root_id)).unwrap();
        let raw = roots.existing_root(root_id).unwrap().unwrap();
        assert_ne!(file.as_slice(), raw.as_slice());
        assert!(partials(dir.path()).is_empty());
    }

    #[test]
    fn destroying_the_root_makes_the_wrapped_key_unusable_and_is_safe_twice() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        let wrapped = roots
            .wrap(
                root_id,
                crate::custody::KeyLabel::STORE,
                &StoreKey::from_bytes([7u8; 32]),
            )
            .unwrap();
        roots.destroy_root(root_id).unwrap();
        assert!(!roots.root_exists(root_id).unwrap());
        assert!(matches!(
            roots.unwrap(root_id, crate::custody::KeyLabel::STORE, &wrapped),
            Err(CustodyError::RootMissing)
        ));
        roots.destroy_root(root_id).unwrap();
        assert!(!roots.root_exists(RootId::fresh().unwrap()).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_in_place_of_the_root_file_is_an_os_error_for_unwrap_and_destroy() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        let wrapped = roots
            .wrap(
                root_id,
                crate::custody::KeyLabel::STORE,
                &StoreKey::from_bytes([5u8; 32]),
            )
            .unwrap();
        let path = roots.path(root_id);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let unwrapped = roots.unwrap(root_id, crate::custody::KeyLabel::STORE, &wrapped);
        assert!(
            matches!(unwrapped, Err(CustodyError::Os(Some(_)))),
            "unwrap answered {:?}",
            unwrapped.err()
        );
        let destroyed = roots.destroy_root(root_id);
        assert!(
            destroyed.is_ok() && !path.exists(),
            "destroy of an empty directory at the root's name answered {destroyed:?} and left \
             it: {}",
            path.exists()
        );
        std::fs::create_dir_all(path.join("occupied")).unwrap();
        let destroyed = roots.destroy_root(root_id);
        assert!(
            matches!(destroyed, Err(CustodyError::Os(Some(_)))),
            "destroy of a directory that holds an entry answered {destroyed:?}"
        );
        assert!(path.join("occupied").exists());
    }

    #[test]
    fn destroying_a_root_also_removes_a_partial_root_file_that_a_failed_write_left() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        roots
            .wrap(
                root_id,
                crate::custody::KeyLabel::STORE,
                &StoreKey::from_bytes([3u8; 32]),
            )
            .unwrap();
        let partial = dir
            .path()
            .join("roots")
            .join(format!("{root_id}.0123456789abcdef.{PARTIAL_EXT}"));
        std::fs::copy(roots.path(root_id), &partial).unwrap();
        roots.destroy_root(root_id).unwrap();
        assert!(
            !partial.exists(),
            "a partial root file stayed after its root was destroyed"
        );
    }

    #[test]
    fn a_root_write_that_fails_leaves_no_partial_root_file() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        std::fs::create_dir_all(roots.path(root_id).join("occupied")).unwrap();
        let made = roots.publish_new_root_by_link(root_id);
        assert!(
            matches!(made, Err(CustodyError::RootExists)),
            "a root write over a directory answered {:?}",
            made.as_ref().map(drop)
        );
        assert!(
            partials(dir.path()).is_empty(),
            "a failed root write left its partial file"
        );
    }

    fn partials(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir.join("roots"))
            .map(|entries| {
                entries
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .filter(|name| name.ends_with(PARTIAL_EXT))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_root_write_leaves_alone_a_partial_root_file_that_another_writer_holds() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        let theirs = dir
            .path()
            .join("roots")
            .join(format!("{root_id}.fedcba9876543210.{PARTIAL_EXT}"));
        std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
        let in_progress = b"the blob that another create is writing";
        std::fs::write(&theirs, in_progress).unwrap();
        roots.create_root(root_id).unwrap();
        let wrapped = roots
            .wrap(root_id, KeyLabel::STORE, &StoreKey::from_bytes([0x61; 32]))
            .unwrap();
        let left = std::fs::read(&theirs).ok();
        assert!(
            left.as_deref() == Some(in_progress.as_slice()),
            "a root write changed or removed the partial file of another writer: {:?}",
            left.map(|bytes| bytes.len())
        );
        assert_eq!(
            roots
                .unwrap(root_id, KeyLabel::STORE, &wrapped)
                .unwrap()
                .expose(),
            &[0x61; 32]
        );
    }

    #[test]
    fn a_key_wrapped_under_one_root_does_not_unwrap_under_another() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let (a, b) = (RootId::fresh().unwrap(), RootId::fresh().unwrap());
        let wrapped = roots
            .wrap(
                a,
                crate::custody::KeyLabel::STORE,
                &StoreKey::from_bytes([1u8; 32]),
            )
            .unwrap();
        roots
            .wrap(
                b,
                crate::custody::KeyLabel::STORE,
                &StoreKey::from_bytes([2u8; 32]),
            )
            .unwrap();
        assert!(matches!(
            roots.unwrap(b, crate::custody::KeyLabel::STORE, &wrapped),
            Err(CustodyError::WrapAuthFailed)
        ));
        let other = FileRoots::new(dir.path().join("roots"), XorProtector(0x00));
        assert!(other
            .unwrap(a, crate::custody::KeyLabel::STORE, &wrapped)
            .is_err());
    }

    #[cfg(unix)]
    fn within<T: Send + 'static>(
        bound: std::time::Duration,
        call: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let (sender, answers) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(call());
        });
        answers.recv_timeout(bound).ok()
    }

    #[cfg(unix)]
    #[test]
    fn a_root_made_under_a_roots_directory_that_is_a_link_narrows_nothing_outside() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("a-directory-at-0755");
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("roots")).unwrap();

        let made = roots(dir.path()).create_root(RootId::fresh().unwrap());

        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert!(
            made.is_err(),
            "a root was made through a link at the roots directory"
        );
        assert_eq!(
            mode, 0o755,
            "the target of the link was narrowed ({made:?})"
        );
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_root_file_that_is_a_link_is_refused_and_its_target_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        roots.create_root(root_id).unwrap();
        let wrapped = roots
            .wrap(
                root_id,
                crate::custody::KeyLabel::STORE,
                &StoreKey::from_bytes([0x42; 32]),
            )
            .unwrap();
        let path = roots.root_path(root_id);
        let target = outside.path().join("a-copy-of-the-root");
        std::fs::rename(&path, &target).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();

        let unwrapped = roots
            .unwrap(root_id, crate::custody::KeyLabel::STORE, &wrapped)
            .map(drop);

        assert!(
            matches!(unwrapped, Err(CustodyError::RootCorrupt)),
            "an unwrap through a root file that is a link answered {unwrapped:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_root_file_that_is_a_fifo_answers_within_a_bound() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        roots.create_root(root_id).unwrap();
        let wrapped = roots
            .wrap(
                root_id,
                crate::custody::KeyLabel::STORE,
                &StoreKey::from_bytes([0x42; 32]),
            )
            .unwrap();
        let path = roots.root_path(root_id);
        std::fs::remove_file(&path).unwrap();
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: the name is a NUL-terminated copy that lives across the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);

        let unwrapped = within(std::time::Duration::from_secs(5), move || {
            roots
                .unwrap(root_id, crate::custody::KeyLabel::STORE, &wrapped)
                .map(drop)
        });

        assert!(
            matches!(unwrapped, Some(Err(CustodyError::RootCorrupt))),
            "an unwrap with a FIFO at the root file answered {unwrapped:?} within 5 s"
        );
    }
}
