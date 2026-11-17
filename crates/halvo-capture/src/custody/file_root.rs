use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use super::dev_fallback::{aead_open, aead_seal, fill_random, store_key_from};
use super::{associated_data, CustodyError, FileIdentity, KeyLabel, RootId, StoreKey, WrappedKey};
use crate::store::durable::{self, Dir, EntryKind};
use crate::store::RootReading;

/// The root that a root step names: its id and, when the store read it, the
/// identity of its root file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NamedRoot {
    pub(crate) id: RootId,
    pub(crate) file: Option<FileIdentity>,
}

impl NamedRoot {
    pub(crate) fn by_id(id: RootId) -> Self {
        Self { id, file: None }
    }
}

/// Opens the roots directory at `roots` as a handle by the components of the
/// path, following no link at its last part in any spelling (`roots/`,
/// `roots/.`; ruling ST-47 T3).
pub(crate) fn hold(roots: &Path) -> Result<Dir, CustodyError> {
    Dir::open(roots).map_err(|e| match e.kind() {
        ErrorKind::NotFound => CustodyError::RootMissing,
        _ if durable::refusal(&e).is_some() => CustodyError::RootCorrupt,
        _ => root_io(e),
    })
}

const ROOT_EXT: &str = "root";
const PARTIAL_EXT: &str = "root.tmp";

/// A place where a test acts between the root step's listing and its first
/// look at a name (ruling ST-46 K1).
#[cfg(test)]
pub(crate) const SEAM_AFTER_THE_LISTING: &str = "the root step, after its listing";

/// The length of a root, from which the code writes a root file.
const ROOT_LEN: usize = 32;

/// The length of a root file where the build's root file has a fixed
/// length: off Windows the root file holds the root as it is, and on
/// Windows its protected form has no fixed length (ruling ST-45 H4).
#[cfg(not(windows))]
const ROOT_FILE_LEN: Option<u64> = Some(ROOT_LEN as u64);
#[cfg(windows)]
const ROOT_FILE_LEN: Option<u64> = None;

/// Whether an entry at a root's own name can be that root's file: a regular
/// file that is not empty and, where the root file has a fixed length, of
/// that length. An all-zero file of that length counts, so that a delete
/// stopped after its zeros resumes (ruling ST-45 H4).
fn can_be_the_root_file(entry: &durable::Entry) -> bool {
    entry.kind == EntryKind::File && can_be_the_root_len(entry.len)
}

/// Whether a regular file of `len` bytes can be a root's file.
fn can_be_the_root_len(len: u64) -> bool {
    len > 0 && ROOT_FILE_LEN.is_none_or(|fixed| len == fixed)
}

/// A root as read from its file, with the identity of that file.
type ReadRoot = (Zeroizing<[u8; 32]>, FileIdentity);

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
    #[cfg(test)]
    let tag = partial_tag_seam::forced().unwrap_or(tag);
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
    let len = file.metadata()?.len();
    write_zeros(&mut file, len)?;
    durable::full_flush(&file)
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

/// What a root step left in the roots directory that it went through: the
/// ids of the root files and partial root files that stay and their names as
/// they stand (a root name or a partial name, which holds a root id alone: a
/// root-file path under C0.3), the entries that stay at any other name,
/// counted by kind and never named (ruling ST-48 U3), and whether the
/// directory could be listed. `listed` is also false when the flush of the
/// directory after the step's removals failed, so that no removal that the
/// listing shows is known to be on the drive (ruling PR-30 PR9 (3)).
/// `at_its_path` is false when the roots directory
/// that the step went through no longer stood at the path that it was opened
/// by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootsLeft {
    pub left: Vec<RootId>,
    pub names: Vec<String>,
    pub others_left: OthersLeft,
    pub listed: bool,
    pub at_its_path: bool,
}

/// The entries that stay in the roots directory at a name that is no root
/// name and no partial name, counted by kind from the last listing; no field
/// holds a name (ruling ST-48 U3). `root_shaped` counts the regular files that
/// can be a root file, of a root file's length off Windows and of any length
/// above 0 on Windows: such a file can still hold a root, also the store's
/// own root moved to that name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OthersLeft {
    pub root_shaped: u64,
    pub directories: u64,
    pub other: u64,
}

impl OthersLeft {
    /// No entry stays at a name that is no root name and no partial name.
    pub fn none(&self) -> bool {
        self.root_shaped == 0 && self.directories == 0 && self.other == 0
    }
}

impl RootsLeft {
    /// The directory was listed and no entry stays in it: no root file, no
    /// partial root file and nothing at any other name.
    pub fn none_left(&self) -> bool {
        self.listed && self.left.is_empty() && self.others_left.none()
    }

    /// Nothing counted, listed or not, with no directory that left its path.
    pub(crate) fn none_counted(listed: bool) -> Self {
        Self {
            left: Vec::new(),
            names: Vec::new(),
            others_left: OthersLeft::default(),
            listed,
            at_its_path: true,
        }
    }
}

/// How a root step treats the roots directory that it opens (ruling ST-45
/// H1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootStepForm {
    /// The handle's delete-all: with the witness, every root file of the
    /// directory, the named one first.
    Witness,
    /// A tail that holds no lock (its lock failed for a reason other than a
    /// holder, or a part of the store path is one that Win32 trims): with the
    /// witness, the entries of the named root alone.
    NamedAlone,
    /// Every other tail: every entry whose name is a root name or a partial
    /// name, named or not.
    Sweep,
}

/// The name of the roots directory in the directory that holds it: a data
/// directory, or a scratch run's store directory (C3.5, C4 item 9).
pub(crate) const ROOTS_DIR_NAME: &str = "roots";

/// The root step of a tail on the roots directory of `parent`, opened from
/// that handle by its fixed name and never followed: the step of
/// [`destroy_roots_through`] on that directory. A link or an entry of another
/// kind at the name, or a directory that cannot be opened or listed, reads
/// not listed; in the sweep nothing at the name reads listed and empty.
pub(crate) fn destroy_roots_below(
    parent: &Dir,
    named: Option<NamedRoot>,
    form: RootStepForm,
) -> (RootReading, RootsLeft) {
    let unread_reading = named.map_or(RootReading::Unnamed, |n| RootReading::Left(Some(n.id)));
    if named.is_none() && form != RootStepForm::Sweep {
        return (RootReading::Unnamed, RootsLeft::none_counted(false));
    }
    match parent.open_dir(OsStr::new(ROOTS_DIR_NAME)) {
        Ok(dir) => destroy_roots_through(&dir, named, form),
        Err(e) if e.kind() == ErrorKind::NotFound && form == RootStepForm::Sweep => {
            (unread_reading, RootsLeft::none_counted(named.is_none()))
        }
        Err(_) => (unread_reading, RootsLeft::none_counted(false)),
    }
}

/// The root step through one opened handle of the roots location. `named`
/// is the root that the handle, the key file or a mark that reads made names,
/// with the identity of its root file when the store read it. The step lists,
/// destroys and reads again through that one handle. In the forms with the
/// witness it destroys there only when the directory holds the named root's
/// file: with the identity known, a regular file of that identity at any
/// name of the directory, so that a root moved inside the directory is
/// destroyed where it lies; without it, at the named root's own name in any
/// letter case, a file that can be the root's file. A partial name, a link,
/// an empty file or an entry of another kind is no witness, and without the
/// witness the step destroys nothing in that directory, also not the entry
/// that it met, and the root reads left (ruling ST-45 Addition 2 (a)). With
/// the identity known, the form for the named root alone leaves an entry of
/// another identity at the root's own name, which then stays and is named.
/// The sweep destroys every root file and partial root file, named or not,
/// and at any other name removes each regular file, writing through it only
/// as [`written_through`] lets it (ruling ST-47 T1, ST-48 U1); a link or an
/// entry of another kind is removed as the entry that it is, and a directory
/// is not entered. On the path by identity the name at which the file stands
/// decides whether it is written through, and a name removed with no write
/// reads the root left (ruling ST-48 U2). What stays at a name that is no root
/// name and no partial name is counted by kind and not named (ruling ST-48
/// U3).
/// The root reads destroyed only when one is named and this call destroyed
/// its file: with the identity known, a file of that identity, met through a
/// kept handle of that identity at its own look, wherever it lies in the
/// directory, found again by one more listing when the name that the first
/// listing gave was absent at that look; without it, a file at the root's own
/// name in any letter case that can be the root's file, as
/// `destroy_root_entry` met it at its own look and never as the listing saw
/// it; and no entry of the root stays; a name of the root that is absent at
/// that look reads the root left (ruling ST-46 K1); left when one is named
/// and not destroyed; unnamed when nothing names one. An entry at the root's
/// own name that cannot be its file (a link, an entry of another kind, an
/// empty file, or off Windows a file of another length) is removed as the
/// entry that it is, with no zero-fill and no write through it (ruling ST-45
/// H4); so is, in every form, a regular file at another root's name that
/// cannot be a root file and has more than one name, or a count that cannot
/// be read (ruling ST-45 Addition 2 (b)); a partial name is zero-filled. A
/// directory that cannot be listed reads not listed. The result says whether
/// the directory still stood at the path that it was opened by after the
/// step. Each zero-fill ends with the full-flush call on its file, and after
/// the removals the directory is flushed with the full-flush call; when that
/// flush fails, the root reads left and the result reads not listed, so the
/// delete answers that it is not finished (ruling PR-30 PR9 (3)). A later
/// call on a handle whose root read destroyed does not come here (ruling
/// ST-45 H3).
pub(crate) fn destroy_roots_through(
    dir: &Dir,
    named: Option<NamedRoot>,
    form: RootStepForm,
) -> (RootReading, RootsLeft) {
    let unread_reading = named.map_or(RootReading::Unnamed, |n| RootReading::Left(Some(n.id)));
    let at_its_path = || matches!(dir.still_named(), Ok(true));
    let unread = || {
        (
            unread_reading,
            RootsLeft {
                at_its_path: at_its_path(),
                ..RootsLeft::none_counted(false)
            },
        )
    };
    if named.is_none() && form != RootStepForm::Sweep {
        return (RootReading::Unnamed, RootsLeft::none_counted(false));
    }
    let Ok(entries) = dir.entries() else {
        return unread();
    };
    let named_id = named.map(|n| n.id);
    let own_file = named.and_then(|n| n.file);
    let of_named = |file: &RootFile| Some(file.id()) == named_id;
    let mut found: Vec<(RootFile, durable::Entry)> = entries
        .iter()
        .filter_map(|entry| {
            RootFile::named_in_any_case(&entry.name).map(|file| (file, entry.clone()))
        })
        .collect();
    let mut identified = own_file.map_or_else(Vec::new, |own| files_of(dir, &entries, own));
    if form != RootStepForm::Sweep {
        let witness = match own_file {
            Some(own) => {
                if identified.is_empty() {
                    identified = dir
                        .entries()
                        .map(|again| files_of(dir, &again, own))
                        .unwrap_or_default();
                }
                !identified.is_empty()
            }
            None => found.iter().any(|(file, entry)| {
                matches!(file, RootFile::Root(_)) && of_named(file) && can_be_the_root_file(entry)
            }),
        };
        if !witness {
            return (unread_reading, roots_that_stand(&entries, at_its_path()));
        }
        if form == RootStepForm::NamedAlone {
            found.retain(|(file, _)| {
                of_named(file) && (own_file.is_none() || matches!(file, RootFile::Partial(_)))
            });
        }
    }
    found.sort_by_key(|(file, _)| !of_named(file));
    #[cfg(test)]
    crate::store::failpoint::seam(SEAM_AFTER_THE_LISTING);
    let mut own_root_file_removed = false;
    let mut every_named_entry_destroyed = true;
    let mut handled: Vec<OsString> = Vec::new();
    if let (Some(own), Some(id)) = (own_file, named_id) {
        let mut destroy_at = |name: &OsStr| match destroy_identified(dir, name, own, id) {
            EntryFate::Absent => false,
            fate => {
                handled.push(name.to_os_string());
                fate == EntryFate::Gone
            }
        };
        for name in &identified {
            own_root_file_removed |= destroy_at(name);
        }
        if !own_root_file_removed {
            let again = dir
                .entries()
                .map(|again| files_of(dir, &again, own))
                .unwrap_or_default();
            for name in again {
                own_root_file_removed |= destroy_at(&name);
            }
        }
    }
    for (file, entry) in &found {
        if handled.contains(&entry.name) {
            continue;
        }
        handled.push(entry.name.clone());
        let at = match file {
            RootFile::Root(_) if of_named(file) => EntryName::OwnRoot,
            RootFile::Root(_) => EntryName::OtherRoot,
            RootFile::Partial(_) => EntryName::Partial,
        };
        let fate = destroy_root_entry(dir, &entry.name, at);
        let decided_by_identity = own_file.is_some() && at == EntryName::OwnRoot;
        if of_named(file) && !decided_by_identity && fate != EntryFate::Gone {
            every_named_entry_destroyed = false;
        }
        if at == EntryName::OwnRoot && !decided_by_identity && fate == EntryFate::Gone {
            own_root_file_removed = true;
        }
    }
    if form != RootStepForm::NamedAlone {
        for entry in entries
            .iter()
            .filter(|entry| entry.kind != EntryKind::Dir && !handled.contains(&entry.name))
        {
            let _ = destroy_root_entry(dir, &entry.name, EntryName::OtherName);
        }
    }
    let flushed = dir.sync().is_ok();
    match dir.entries() {
        Ok(entries) => {
            let stands = RootsLeft {
                listed: flushed,
                ..roots_that_stand(&entries, at_its_path())
            };
            let reading = match named_id {
                None => RootReading::Unnamed,
                Some(named)
                    if own_root_file_removed
                        && every_named_entry_destroyed
                        && flushed
                        && !stands.left.contains(&named) =>
                {
                    RootReading::Destroyed
                }
                Some(named) => RootReading::Left(Some(named)),
            };
            (reading, stands)
        }
        Err(_) => unread(),
    }
}

/// The root files and partial root files of a listing, by their names as
/// they stand, sorted by name, and the entries at any other name counted by
/// kind and not named (ruling ST-48 U3).
fn roots_that_stand(entries: &[durable::Entry], at_its_path: bool) -> RootsLeft {
    let mut stays: Vec<(RootId, String)> = entries
        .iter()
        .filter_map(|entry| {
            RootFile::named_in_any_case(&entry.name)
                .map(|file| (file.id(), entry.name.to_string_lossy().into_owned()))
        })
        .collect();
    stays.sort_by(|a, b| a.1.cmp(&b.1));
    let mut left: Vec<RootId> = stays.iter().map(|(id, _)| *id).collect();
    left.dedup();
    let mut others_left = OthersLeft::default();
    for entry in entries
        .iter()
        .filter(|entry| RootFile::named_in_any_case(&entry.name).is_none())
    {
        match entry.kind {
            EntryKind::File if can_be_the_root_file(entry) => others_left.root_shaped += 1,
            EntryKind::Dir => others_left.directories += 1,
            _ => others_left.other += 1,
        }
    }
    RootsLeft {
        left,
        names: stays.into_iter().map(|(_, name)| name).collect(),
        others_left,
        listed: true,
        at_its_path,
    }
}

/// The names in `entries` of regular files whose identity, read through
/// `dir` without following a link, is `own`.
fn files_of(dir: &Dir, entries: &[durable::Entry], own: FileIdentity) -> Vec<OsString> {
    entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .filter(
            |entry| matches!(dir.file_identity_at(&entry.name), Ok(identity) if identity == own),
        )
        .map(|entry| entry.name.clone())
        .collect()
}

/// The kind of the name `name` for the root `id`, as the destroy step reads
/// it.
fn entry_name_for(name: &OsStr, id: RootId) -> EntryName {
    match RootFile::named_in_any_case(name) {
        Some(RootFile::Root(of)) if of == id => EntryName::OwnRoot,
        Some(RootFile::Root(_)) => EntryName::OtherRoot,
        Some(RootFile::Partial(_)) => EntryName::Partial,
        None => EntryName::OtherName,
    }
}

/// Destroys the root file of identity `own` at `name`, wherever in the
/// directory it lies; `id` is the root that the identity is of. The identity
/// decides only which file is looked at. Whether it is written through is
/// decided by the name at which it stands, as `destroy_root_entry` decides it
/// for that name ([`written_through`], ruling ST-48 U2): the count of names
/// and the length are first read by the name, with no open and no mode
/// change, and a file that may not be written there has that name removed
/// with no write through it while the name still leads to that file, which
/// is no destruction of the root (`NotTheRootFile`). Nothing is written
/// unless the handle kept at this call's own look holds a file of that
/// identity and shows again what the read by name showed; then the file is
/// zero-filled through that handle, which reaches every name of it, and the
/// name is removed while it still leads to that file. `Gone` when the
/// zero-fill went through, or when the count read through the kept handle
/// after the removal of its one name is 0; `Absent` when no file of that
/// identity stood at the name at the look.
fn destroy_identified(dir: &Dir, name: &OsStr, own: FileIdentity, id: RootId) -> EntryFate {
    match dir.entry(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => return EntryFate::Absent,
        Err(e) => return EntryFate::KeyMayStay(Some(e)),
        Ok(entry) if entry.kind != EntryKind::File => return EntryFate::Absent,
        Ok(_) => {}
    }
    let shape = match dir.file_shape_at(name) {
        Ok(shape) if shape.identity == own => shape,
        Ok(_) => return EntryFate::Absent,
        Err(e) if e.kind() == ErrorKind::NotFound || durable::refusal(&e).is_some() => {
            return EntryFate::Absent
        }
        Err(e) => return EntryFate::KeyMayStay(Some(e)),
    };
    let at = entry_name_for(name, id);
    let removed_unwritten = || match dir.file_identity_at(name) {
        Ok(identity) if identity == own => match dir.remove_entry(name) {
            Err(e) if e.kind() != ErrorKind::NotFound => EntryFate::KeyMayStay(Some(e)),
            _ => EntryFate::NotTheRootFile,
        },
        _ => EntryFate::Absent,
    };
    if !written_through(at, shape.names == 1, can_be_the_root_len(shape.len)) {
        return removed_unwritten();
    }
    #[cfg(unix)]
    let _ = dir.set_file_mode(name, 0o600);
    let kept = match dir
        .open_file_for_update(name)
        .or_else(|_| dir.open_file(name))
        .or_else(|_| dir.open_file_for_write(name))
    {
        Ok(kept) => kept,
        Err(e) if e.kind() == ErrorKind::NotFound || durable::refusal(&e).is_some() => {
            return EntryFate::Absent
        }
        Err(e) => return EntryFate::KeyMayStay(Some(e)),
    };
    if !matches!(durable::identity_of(&kept), Ok(identity) if identity == own) {
        return EntryFate::Absent;
    }
    let root_len = kept
        .metadata()
        .is_ok_and(|meta| meta.is_file() && can_be_the_root_len(meta.len()));
    let one_name = matches!(durable::file_link_count(&kept), Ok(1));
    if !written_through(at, one_name, root_len) {
        return removed_unwritten();
    }
    #[cfg(windows)]
    make_writable(dir, name, &kept);
    let remove = || match dir.remove_entry(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    };
    if zero_fill_through(dir, name, &kept).is_ok() {
        if !matches!(dir.names_file(name, &kept), Ok(true)) {
            return EntryFate::Gone;
        }
        return match remove() {
            Ok(()) => EntryFate::Gone,
            Err(_) => EntryFate::ZeroedAndNamed,
        };
    }
    match durable::file_link_count(&kept) {
        Ok(1) if matches!(dir.names_file(name, &kept), Ok(true)) => match remove() {
            Ok(()) if matches!(durable::file_link_count(&kept), Ok(0)) => EntryFate::Gone,
            Ok(()) => EntryFate::KeyMayStay(None),
            Err(e) => EntryFate::KeyMayStay(Some(e)),
        },
        _ => EntryFate::KeyMayStay(None),
    }
}

/// Which name of a roots directory an entry stands at, for its destruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryName {
    /// The named root's own name, in any letter case.
    OwnRoot,
    /// The root name of another id.
    OtherRoot,
    /// A partial name.
    Partial,
    /// A name that is no root name and no partial name (ruling ST-47 T1).
    OtherName,
}

/// What the destruction of one entry of a roots directory met and left.
#[derive(Debug)]
enum EntryFate {
    /// This call met a file that can be the root's, zero-filled it through
    /// its kept handle or read its count 0 after its own removal: the name is
    /// gone, and no name of the file keeps the key.
    Gone,
    /// Nothing stood at the name at this call's own look (ruling ST-46 K1).
    Absent,
    /// What stood at the name was removed as the entry that it is, with no
    /// write through it, and counts as no removal of the root: at the root's
    /// own name it could not be the root's file (ruling ST-45 H4, ST-46 K1);
    /// on the path by identity its name let no write through it (ruling
    /// ST-48 U2).
    NotTheRootFile,
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

/// Whether the destroy step may write through a regular file that stands at a
/// name of kind `at`, by its count of names and its length: at a partial name
/// always; at the named root's own name only with a root file's length
/// (ruling ST-45 H4); at another id's root name with a root file's length or
/// with one name (ruling ST-45 Addition 2 (b)); at any other name only with
/// one name and a root file's length (ruling ST-48 U1). Any other file has its
/// name removed with no write through it.
fn written_through(at: EntryName, one_name: bool, root_len: bool) -> bool {
    match at {
        EntryName::Partial => true,
        EntryName::OwnRoot => root_len,
        EntryName::OtherRoot => root_len || one_name,
        EntryName::OtherName => one_name && root_len,
    }
}

/// Destroys one entry of a roots directory. An entry that is no regular file
/// is removed as the entry that it is. At a name that is no root name and no
/// partial name the step first reads the file's count of names and its length
/// by its name through the directory's handle, following no link and opening
/// nothing; a file whose read shows more than one name, a count that cannot be
/// read or no root file's length has its name removed with no open, no mode
/// change and no write (ruling ST-48 U1). For a regular file that may be
/// written the step then sets its mode to 0600 by its name through the
/// directory's handle, following no link (on Windows it clears the read-only
/// attribute after the open and the test), then keeps one handle: the open for update, or the read open, or the write open
/// of the same name, none following a link. The count, the test that the name
/// still leads to that file and the zero-fill go through the kept handle; a
/// kept handle that cannot write zero-fills through a write open of the name
/// that still leads to the same file. The zero-fill reaches every name of the
/// file, and the name is removed. If the zero-fill still fails: with one name
/// (the count read through the kept handle) the name is removed, and the
/// entry is gone only when the count read through that handle after the
/// removal is 0; with more than one name, or a count that cannot be read, the
/// name is kept and a key may stay. When no open succeeds the name is kept
/// and a key may stay, and every later tail tries it and names it again
/// (ruling ST-45 H2). Everything rests on what this call met at its own look,
/// never on a listing (ruling ST-46 K1): a name that is absent at the look is
/// `Absent`; at the root's own name, an entry of another kind, or a file
/// whose length read through the kept handle cannot be a root file's, is
/// removed as the entry that it is with no write through it and is
/// `NotTheRootFile`; at another root's name such a file with more than one
/// name, or a count that cannot be read, is removed with no write through it
/// (ruling ST-45 Addition 2 (b)); at any other name the kept handle shows again
/// the identity, the one name and the root file's length that the read by
/// name showed before any write, or the name is removed with no write through
/// it ([`written_through`]).
fn destroy_root_entry(dir: &Dir, name: &OsStr, at: EntryName) -> EntryFate {
    let remove = |dir: &Dir| match dir.remove_entry(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    };
    let removed_unwritten = |dir: &Dir| match remove(dir) {
        Ok(()) if at == EntryName::OwnRoot => EntryFate::NotTheRootFile,
        Ok(()) => EntryFate::Gone,
        Err(e) => EntryFate::KeyMayStay(Some(e)),
    };
    match dir.entry(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => return EntryFate::Absent,
        Err(e) => return EntryFate::KeyMayStay(Some(e)),
        Ok(entry) if entry.kind != EntryKind::File => return removed_unwritten(dir),
        Ok(_) => {}
    }
    let by_name = if at == EntryName::OtherName {
        match dir.file_shape_at(name) {
            Ok(shape) if written_through(at, shape.names == 1, can_be_the_root_len(shape.len)) => {
                Some(shape.identity)
            }
            Err(e) if e.kind() == ErrorKind::NotFound => return EntryFate::Absent,
            _ => return removed_unwritten(dir),
        }
    } else {
        None
    };
    #[cfg(unix)]
    let _ = dir.set_file_mode(name, 0o600);
    let kept = match dir
        .open_file_for_update(name)
        .or_else(|_| dir.open_file(name))
        .or_else(|_| dir.open_file_for_write(name))
    {
        Ok(kept) => kept,
        Err(e) if e.kind() == ErrorKind::NotFound => return EntryFate::Absent,
        Err(e) if durable::refusal(&e).is_some() => return removed_unwritten(dir),
        Err(e) => return EntryFate::KeyMayStay(Some(e)),
    };
    let root_len = kept
        .metadata()
        .is_ok_and(|meta| meta.is_file() && can_be_the_root_len(meta.len()));
    let one_name = matches!(durable::file_link_count(&kept), Ok(1));
    let same_file = by_name
        .is_none_or(|identity| matches!(durable::identity_of(&kept), Ok(held) if held == identity));
    if !(same_file && written_through(at, one_name, root_len)) {
        return removed_unwritten(dir);
    }
    #[cfg(windows)]
    make_writable(dir, name, &kept);
    let zeroed = matches!(dir.names_file(name, &kept), Ok(true))
        && zero_fill_through(dir, name, &kept).is_ok();
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

/// Zero-fills the file of the kept handle: through that handle, or, when it
/// cannot write, through a write open of `name` that still leads to the same
/// file.
fn zero_fill_through(dir: &Dir, name: &OsStr, kept: &std::fs::File) -> std::io::Result<()> {
    if zero_through(kept).is_ok() {
        return Ok(());
    }
    let writer = dir.open_file_for_write(name)?;
    if !(dir.names_file(name, kept)? && dir.names_file(name, &writer)?) {
        return Err(std::io::Error::from(ErrorKind::NotFound));
    }
    zero_through(&writer)
}

fn zero_through(file: &std::fs::File) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom};
    let len = file.metadata()?.len();
    let mut writer = file;
    writer.seek(SeekFrom::Start(0))?;
    write_zeros(&mut writer, len)?;
    durable::full_flush(file)
}

/// The size of one write of zeros of the root step: its zero-fill writes a
/// file's length in blocks of this size and never allocates that length
/// (ruling ST-47 T1). The key file's zero-fill is not the root step's and is
/// not held to it.
const ZERO_BLOCK: usize = 64 * 1024;

/// Writes `len` zero bytes to `out` in blocks of at most [`ZERO_BLOCK`].
fn write_zeros(out: &mut impl Write, len: u64) -> std::io::Result<()> {
    let block = [0u8; ZERO_BLOCK];
    let mut left = len;
    while left > 0 {
        let now = left.min(ZERO_BLOCK as u64) as usize;
        out.write_all(&block[..now])?;
        left -= now as u64;
    }
    Ok(())
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
        let _ = std::fs::set_permissions(dir.entry_path(name), permissions);
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
    destroy_named_root_at(roots, NamedRoot::by_id(root_id))
}

/// [`destroy_root_through`] on the roots directory at `roots`, opened here;
/// Ok when nothing stands there.
pub(crate) fn destroy_named_root_at(roots: &Path, root: NamedRoot) -> Result<(), CustodyError> {
    let dir = match Dir::open(roots) {
        Ok(dir) => dir,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) if durable::refusal(&e).is_some() => return Err(CustodyError::RootCorrupt),
        Err(e) => return Err(root_io(e)),
    };
    destroy_root_through(&dir, root)
}

/// The destruction of one root through `dir`, for a cleanup and for the
/// custody's destroy. With the identity of its root file known it acts on the
/// file of that identity alone, wherever it lies in the directory, and on
/// nothing else, and writes through it only where the name at which it
/// stands lets the step write; a name removed there with no write through it
/// is taken as done (ruling ST-48 U2); without it, every entry of the id at its root name in any
/// letter case and at its partial names, each partial counted as the root
/// file is (the review's N7). Answers Ok when no entry of the root stands at
/// its look, so that a cleanup that runs again ends, and the error of the
/// flush of the directory after the removals when that flush fails (ruling
/// PR-30 PR9 (3)).
pub(crate) fn destroy_root_through(dir: &Dir, root: NamedRoot) -> Result<(), CustodyError> {
    let mut fates = Vec::new();
    match root.file {
        Some(own) => {
            let entries = dir.entries().map_err(root_io)?;
            for name in files_of(dir, &entries, own) {
                fates.push(destroy_identified(dir, &name, own, root.id));
            }
        }
        None => {
            if let Ok(entries) = dir.entries() {
                for entry in entries {
                    match RootFile::named_in_any_case(&entry.name) {
                        Some(RootFile::Partial(id)) if id == root.id => {
                            fates.push(destroy_root_entry(dir, &entry.name, EntryName::Partial));
                        }
                        Some(RootFile::Root(id))
                            if id == root.id && entry.name != root_name(root.id) =>
                        {
                            fates.push(destroy_root_entry(dir, &entry.name, EntryName::OwnRoot));
                        }
                        _ => {}
                    }
                }
            }
            fates.push(destroy_root_entry(
                dir,
                &root_name(root.id),
                EntryName::OwnRoot,
            ));
        }
    }
    let flushed = dir.sync();
    for fate in fates {
        match fate {
            EntryFate::Gone
            | EntryFate::Absent
            | EntryFate::NotTheRootFile
            | EntryFate::ZeroedAndNamed => {}
            EntryFate::KeyMayStay(Some(e)) => return Err(root_io(e)),
            EntryFate::KeyMayStay(None) => return Err(CustodyError::RootDenied),
        }
    }
    flushed.map_err(root_io)
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
        Ok(self.existing_root_in(&dir, root_id)?.map(|(root, _)| root))
    }

    /// The root of `root_id` read through `dir`, with the identity of the
    /// file that was read; `None` for no file, an empty one or an all-zero
    /// one.
    fn existing_root_in(
        &self,
        dir: &Dir,
        root_id: RootId,
    ) -> Result<Option<ReadRoot>, CustodyError> {
        let mut blob = Zeroizing::new(Vec::new());
        let read = dir
            .open_file(&root_name(root_id))
            .map_err(read_error)
            .and_then(|mut file| {
                let identity = durable::identity_of(&file).map_err(root_io)?;
                file.read_to_end(&mut blob).map_err(read_error)?;
                Ok(identity)
            });
        let identity = match read {
            Ok(identity) => identity,
            Err(CustodyError::RootMissing) => return Ok(None),
            Err(e) => return Err(e),
        };
        if blob.iter().all(|&b| b == 0) {
            return Ok(None);
        }
        let plain = self.protector.unprotect(&blob, root_id.as_bytes())?;
        if plain.len() != ROOT_LEN {
            return Err(CustodyError::RootCorrupt);
        }
        let mut root = Zeroizing::new([0u8; 32]);
        root.copy_from_slice(&plain);
        Ok(Some((root, identity)))
    }

    /// Wraps `key` under the root of `root_id` read through `dir`: the wrap
    /// and the identity of the root file that it read.
    pub(crate) fn wrap_in(
        &self,
        dir: &Dir,
        root_id: RootId,
        label: KeyLabel,
        key: &StoreKey,
    ) -> Result<(WrappedKey, FileIdentity), CustodyError> {
        let (root, identity) = self
            .existing_root_in(dir, root_id)?
            .ok_or(CustodyError::RootMissing)?;
        Ok((seal(root_id, label, root, key)?, identity))
    }

    /// Unwraps `wrapped` under the root of `root_id` read through `dir`: the
    /// key and the identity of the root file that it read.
    pub(crate) fn unwrap_in(
        &self,
        dir: &Dir,
        root_id: RootId,
        label: KeyLabel,
        wrapped: &WrappedKey,
    ) -> Result<(StoreKey, FileIdentity), CustodyError> {
        let (root, identity) = self
            .existing_root_in(dir, root_id)?
            .ok_or(CustodyError::RootMissing)?;
        let plain = aead_open(&root, &wrapped.0, &associated_data(root_id, label))
            .map_err(|_| CustodyError::WrapAuthFailed)?;
        Ok((store_key_from(plain)?, identity))
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
        let mut root = Zeroizing::new([0u8; ROOT_LEN]);
        fill_random(root.as_mut_slice())?;
        let blob = self
            .protector
            .protect(root.as_slice(), root_id.as_bytes())?;
        let dir = self.open_private_roots_dir()?;
        let partial = partial_name(root_id)?;
        let file = dir.create_new(&partial).map_err(root_io)?;
        let written = fill_partial(file, &blob);
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

#[cfg(test)]
fn write_partial(dir: &Dir, partial: &OsStr, blob: &[u8]) -> Result<(), CustodyError> {
    fill_partial(dir.create_new(partial).map_err(root_io)?, blob)
}

fn fill_partial(mut file: std::fs::File, blob: &[u8]) -> Result<(), CustodyError> {
    file.write_all(blob).map_err(root_io)?;
    file.sync_all().map_err(root_io)
}

/// A test's tag for the partial names that this thread writes next, in
/// place of the random one (ruling S-95 (f)).
#[cfg(test)]
pub(crate) mod partial_tag_seam {
    use std::cell::Cell;

    thread_local! {
        static FORCED: Cell<Option<[u8; 8]>> = const { Cell::new(None) };
    }

    pub(crate) fn force(tag: Option<[u8; 8]>) {
        FORCED.with(|forced| forced.set(tag));
    }

    pub(crate) fn forced() -> Option<[u8; 8]> {
        FORCED.with(|forced| forced.get())
    }
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

        fn wrap_store_key_in(
            &self,
            roots: &crate::custody::RootsHandle,
            key: &StoreKey,
            label: crate::custody::KeyLabel,
        ) -> Result<(WrappedKey, crate::custody::FileIdentity), CustodyError> {
            self.roots.wrap_in(&roots.dir, self.root_id, label, key)
        }

        fn unwrap_store_key_in(
            &self,
            roots: &crate::custody::RootsHandle,
            wrapped: &WrappedKey,
            label: crate::custody::KeyLabel,
        ) -> Result<(StoreKey, crate::custody::FileIdentity), CustodyError> {
            self.roots
                .unwrap_in(&roots.dir, self.root_id, label, wrapped)
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
    fn a_zero_fill_writes_in_blocks_of_a_fixed_size_and_never_the_whole_length_at_once() {
        struct Recorder {
            writes: Vec<usize>,
            zeros: bool,
        }
        impl Write for Recorder {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.writes.push(bytes.len());
                self.zeros &= bytes.iter().all(|&b| b == 0);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let len = 3 * ZERO_BLOCK as u64 + 5;
        let mut out = Recorder {
            writes: Vec::new(),
            zeros: true,
        };
        write_zeros(&mut out, len).unwrap();
        let largest = out.writes.iter().copied().max().unwrap_or(0);
        println!(
            "ZERO-BLOCKS writes={} largest={largest} total={}",
            out.writes.len(),
            out.writes.iter().sum::<usize>()
        );
        assert!(out.zeros);
        assert!(largest <= ZERO_BLOCK, "a write of {largest} bytes");
        assert_eq!(out.writes.iter().sum::<usize>() as u64, len);
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

    #[cfg(target_os = "macos")]
    #[test]
    fn a_cleanup_whose_partial_root_file_cannot_be_destroyed_answers_an_error() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        roots.create_root(root_id).unwrap();
        let partial = dir
            .path()
            .join("roots")
            .join(format!("{root_id}.0123456789abcdef.{PARTIAL_EXT}"));
        std::fs::write(&partial, [0x5au8; 32]).unwrap();
        let immutable = |on: bool| {
            let c = std::ffi::CString::new(partial.as_os_str().as_bytes()).unwrap();
            let flags = if on { libc::UF_IMMUTABLE } else { 0 };
            assert_eq!(unsafe { libc::chflags(c.as_ptr(), flags) }, 0);
        };
        immutable(true);
        let destroyed = roots.destroy_root(root_id);
        immutable(false);
        let bytes = std::fs::read(&partial).ok();
        println!(
            "N7 destroy={destroyed:?} partial_keeps_its_bytes={}",
            bytes.as_deref() == Some(&[0x5au8; 32][..])
        );
        assert!(
            destroyed.is_err(),
            "the cleanup answered Ok while a partial root file of its id holds the root's bytes"
        );
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
    fn a_root_write_whose_partial_name_another_writer_holds_leaves_that_file_and_makes_no_root() {
        use super::partial_tag_seam;
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh().unwrap();
        std::fs::create_dir_all(dir.path().join("roots")).unwrap();
        let theirs = dir
            .path()
            .join("roots")
            .join(format!("{root_id}.0f0f0f0f0f0f0f0f.{PARTIAL_EXT}"));
        let in_progress = b"the blob that another create is writing";
        std::fs::write(&theirs, in_progress).unwrap();
        partial_tag_seam::force(Some([0x0f; 8]));
        let made = roots.create_root(root_id);
        partial_tag_seam::force(None);
        let left = std::fs::read(&theirs).ok();
        println!(
            "S95F made={made:?} theirs_kept={} root_made={}",
            left.as_deref() == Some(in_progress.as_slice()),
            roots.path(root_id).exists()
        );
        assert!(
            made.is_err(),
            "the root write shared another writer's partial name"
        );
        assert_eq!(
            left.as_deref(),
            Some(in_progress.as_slice()),
            "the root write changed or removed the partial file of another writer"
        );
        assert!(!roots.path(root_id).exists());
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
