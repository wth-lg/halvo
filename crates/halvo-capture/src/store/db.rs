//! SQLCipher metadata store and the [`EncryptedStore`].

use std::ffi::OsStr;
use std::fs::{File, TryLockError};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use halvo_sqlcipher::rusqlite::config::DbConfig;
use halvo_sqlcipher::rusqlite::{self, Connection, ErrorCode, OpenFlags};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::custody::{
    custody_for, CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey,
};
use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

use super::durable::{self, Dir, DurableFs, EntryKind, OsFs, Refused};
use super::segment::{self, SegmentHeader, SegmentKey};
use super::{SegmentStore, StoreError, StoreResult};

const KEY_FILE_VERSION: u32 = 1;
const DATABASE_VERSION: i64 = 1;
const KEY_FILE: &str = "store.key";
const KEY_FILE_TEMP: &str = "store.key.tmp";
const DATABASE_FILE: &str = "store.db";
const DATABASE_LOG: &str = "store.db-wal";
const LINKED_DATABASE: &str = "the database file of the store is a link";
const STORE_DIR_MOVED: &str = "the store directory no longer stands at its name";
const LINKED_SIDE_FILE: &str = "a file beside the database of the store is a link";
const DATABASE_SIDE_FILES: [&str; 3] = [DATABASE_LOG, "store.db-shm", "store.db-journal"];

#[cfg(test)]
pub(crate) const SEAM_BEFORE_THE_LOOK: &str = "a database open, before its look";
#[cfg(test)]
pub(crate) const SEAM_AFTER_THE_LAST_LOOK: &str = "a database open, after its last look";
#[cfg(test)]
pub(crate) const SEAM_BEFORE_THE_MODE_STEP: &str = "the mode step of store.db";
#[cfg(test)]
pub(crate) const SEAM_SIZE_AFTER_THE_LISTING: &str = "the size, after its first listing";

#[derive(Deserialize)]
struct KeyFileVersion {
    version: u32,
}

fn known_version(found: i64, supported: i64) -> StoreResult<()> {
    match found.cmp(&supported) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Greater => Err(StoreError::NewerStore { found, supported }),
        std::cmp::Ordering::Less => Err(StoreError::OlderStore { found, supported }),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct KeyFile {
    pub version: u32,
    pub custody_kind: CustodyKind,
    pub root_id: RootId,
    pub wrapped_store_key_b64: String,
}

impl KeyFile {
    pub(crate) fn path(store_dir: &Path) -> PathBuf {
        store_dir.join("store.key")
    }

    fn is_in(store_dir: &Dir) -> StoreResult<bool> {
        match store_dir.entry(OsStr::new(KEY_FILE)) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_error(e)),
        }
    }

    fn write(&self, store_dir: &Dir) -> StoreResult<()> {
        let json =
            serde_json::to_vec_pretty(self).map_err(|_| StoreError::Io(ErrorKind::InvalidData))?;
        durable::write_durably(
            &OsFs,
            store_dir,
            OsStr::new(KEY_FILE_TEMP),
            OsStr::new(KEY_FILE),
            io_error,
            |_| Ok(()),
            |dir, temp| match dir.remove_file(temp) {
                Err(e) if e.kind() != ErrorKind::NotFound => Err(io_error(e)),
                _ => Ok(()),
            },
            |out| out.write_all(&json).map_err(io_error),
        )
    }

    pub(crate) fn read(store_dir: &Path) -> StoreResult<Self> {
        Self::read_in(&Dir::open(store_dir).map_err(store_dir_error)?)
    }

    fn read_in(store_dir: &Dir) -> StoreResult<Self> {
        let json = match read_whole(store_dir, KEY_FILE) {
            Ok(json) => json,
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(StoreError::Sealed),
            Err(e) => return Err(key_file_error(e)),
        };
        serde_json::from_slice::<KeyFileVersion>(&json)
            .map_err(|_| StoreError::KeyFileUnparseable)
            .and_then(|head| known_version(i64::from(head.version), i64::from(KEY_FILE_VERSION)))?;
        serde_json::from_slice(&json).map_err(|_| StoreError::KeyFileUnparseable)
    }

    pub(crate) fn wrapped_store_key(&self) -> StoreResult<WrappedKey> {
        let wrapped = BASE64
            .decode(self.wrapped_store_key_b64.as_bytes())
            .map_err(|_| StoreError::KeyFileUnparseable)?;
        match self.custody_kind.wrapped_store_key_len() {
            Some(len) if wrapped.len() != len => Err(StoreError::KeyFileUnparseable),
            _ => Ok(WrappedKey(wrapped)),
        }
    }

    pub(crate) fn destroy(store_dir: &Path) -> StoreResult<()> {
        failpoint::hit("key_file_destroy")?;
        let path = Self::path(store_dir);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return remove_if_present(&path),
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io_error(e)),
        }
        match durable::no_follow(File::options().write(true)).open(&path) {
            Ok(mut file) => {
                let len = file.metadata().map_err(io_error)?.len() as usize;
                file.write_all(&vec![0u8; len]).map_err(io_error)?;
                file.sync_all().map_err(io_error)?;
            }
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io_error(e)),
        }
        remove_if_present(&path)
    }
}

fn io_error(e: std::io::Error) -> StoreError {
    StoreError::Io(e.kind())
}

fn read_whole(store_dir: &Dir, name: &str) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    store_dir
        .open_file(OsStr::new(name))?
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn key_file_error(e: std::io::Error) -> StoreError {
    match durable::refusal(&e) {
        Some(Refused::Link) => {
            StoreError::Corrupt("the key file of the store is a link".to_string())
        }
        Some(_) => StoreError::KeyFileUnparseable,
        None => io_error(e),
    }
}

fn store_dir_error(e: std::io::Error) -> StoreError {
    match durable::refusal(&e) {
        Some(Refused::Link) => StoreError::Corrupt("the store directory is a link".to_string()),
        #[cfg(windows)]
        Some(Refused::Moved) => StoreError::Corrupt(STORE_DIR_MOVED.to_string()),
        Some(_) => StoreError::Io(ErrorKind::NotADirectory),
        None => io_error(e),
    }
}

fn database_file_error(e: std::io::Error) -> StoreError {
    match durable::refusal(&e) {
        Some(Refused::Link) => StoreError::Corrupt(LINKED_DATABASE.to_string()),
        Some(_) => StoreError::DatabaseUnreadable,
        None if e.kind() == ErrorKind::NotFound => StoreError::DatabaseMissing,
        None => io_error(e),
    }
}

fn database_open_error(e: rusqlite::Error) -> StoreError {
    match &e {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.extended_code == rusqlite::ffi::SQLITE_CANTOPEN_SYMLINK =>
        {
            StoreError::Corrupt(LINKED_DATABASE.to_string())
        }
        _ => db_error(e),
    }
}

const DATABASE_OPEN: OpenFlags = OpenFlags::SQLITE_OPEN_READ_WRITE
    .union(OpenFlags::SQLITE_OPEN_NO_MUTEX)
    .union(OpenFlags::SQLITE_OPEN_NOFOLLOW);

const PROBE_OPEN: OpenFlags = OpenFlags::SQLITE_OPEN_READ_ONLY
    .union(OpenFlags::SQLITE_OPEN_NO_MUTEX)
    .union(OpenFlags::SQLITE_OPEN_NOFOLLOW);

/// Opens the connection on the database file of `store_dir`. On macOS the
/// store first sets the mode of the file to 0600 on its own descriptor and
/// closes that descriptor before SQLite opens the name, because closing a
/// descriptor of the file after SQLite took its locks would drop them.
#[cfg(unix)]
fn open_database(store_dir: &Dir, flags: OpenFlags) -> StoreResult<Connection> {
    use std::os::unix::fs::PermissionsExt;
    #[cfg(test)]
    failpoint::seam(SEAM_BEFORE_THE_MODE_STEP);
    store_dir
        .open_file(OsStr::new(DATABASE_FILE))
        .map_err(database_file_error)?
        .set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(io_error)?;
    open_connection(store_dir, flags)
}

/// SQLite opens the database by its name under the handle's name, so the
/// store goes on only while that name still stands for the handle's own
/// directory.
fn refuse_a_moved_store_dir(store_dir: &Dir) -> StoreResult<()> {
    match store_dir.still_named() {
        Ok(true) => Ok(()),
        Ok(false) => Err(StoreError::Corrupt(STORE_DIR_MOVED.to_string())),
        Err(e) => Err(io_error(e)),
    }
}

/// Opens the connection on the database file of `store_dir`. On Windows the
/// store holds the file from its check until the connection is open, with a
/// handle that shares no delete, so no other file can take the name.
#[cfg(windows)]
fn open_database(store_dir: &Dir, flags: OpenFlags) -> StoreResult<Connection> {
    #[cfg(test)]
    failpoint::seam(SEAM_BEFORE_THE_MODE_STEP);
    open_connection(store_dir, flags)
}

/// The one open of SQLite in the store, for the probe and for both
/// connections: a look at the four names of the database through the
/// handle, the last look at the handle's name, then SQLite's open of the
/// database by that name with the flags that the caller passes.
fn open_connection(store_dir: &Dir, flags: OpenFlags) -> StoreResult<Connection> {
    #[cfg(test)]
    failpoint::seam(SEAM_BEFORE_THE_LOOK);
    look_at_the_database_files(store_dir)?;
    #[cfg(windows)]
    let held = hold_the_database_file(store_dir)?;
    refuse_a_moved_store_dir(store_dir)?;
    #[cfg(test)]
    failpoint::seam(SEAM_AFTER_THE_LAST_LOOK);
    let conn =
        Connection::open_with_flags(Db::path(store_dir.path()), flags).map_err(database_open_error);
    #[cfg(windows)]
    drop(held);
    conn
}

/// On Windows the database file is held from the look until the connection
/// is open, with a handle that shares no delete, so no other file can take
/// its name meanwhile.
#[cfg(windows)]
fn hold_the_database_file(store_dir: &Dir) -> StoreResult<Option<File>> {
    match store_dir.hold_file(OsStr::new(DATABASE_FILE)) {
        Ok(held) => Ok(Some(held)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(database_file_error(e)),
    }
}

/// Reads store.db and its three side files through the handle, following no
/// link and without a wait: each is absent or a regular file, a link answers
/// Corrupt, and a name that is no regular file answers DatabaseUnreadable.
fn look_at_the_database_files(store_dir: &Dir) -> StoreResult<()> {
    let names = [(DATABASE_FILE, LINKED_DATABASE)]
        .into_iter()
        .chain(DATABASE_SIDE_FILES.map(|name| (name, LINKED_SIDE_FILE)));
    for (name, linked) in names {
        match store_dir.entry(OsStr::new(name)) {
            Ok(entry) if entry.kind == EntryKind::File => {}
            Ok(entry) if entry.kind == EntryKind::Link => {
                return Err(StoreError::Corrupt(linked.to_string()))
            }
            Ok(_) => return Err(StoreError::DatabaseUnreadable),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(e)),
        }
    }
    Ok(())
}

pub(crate) use super::failpoint;

pub(crate) fn db_error(e: rusqlite::Error) -> StoreError {
    match e {
        rusqlite::Error::SqliteFailure(failure, _) => StoreError::Db(failure.extended_code),
        other => StoreError::Corrupt(format!(
            "the database returned an unexpected value: {other}"
        )),
    }
}

pub(crate) fn unix_secs_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn unix_nanos(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

pub(crate) fn remove_if_present(path: &Path) -> StoreResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}

fn remove_in(store_dir: &Dir, name: &str) -> StoreResult<()> {
    match store_dir.remove_file(OsStr::new(name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}

struct WriterLock {
    file: File,
}

const LOCK_FILE_OPEN_TRIES: u32 = 4;

impl WriterLock {
    fn take(store_dir: &Path) -> StoreResult<Self> {
        Self::take_with(store_dir, &mut Self::open_lock_file)
    }

    fn take_with(
        store_dir: &Path,
        open: &mut dyn FnMut(&Path) -> std::io::Result<File>,
    ) -> StoreResult<Self> {
        let path = store_dir.join("writer.lock");
        let mut tries = 1;
        let file = loop {
            match open(&path) {
                Err(e) if e.kind() == ErrorKind::NotFound && tries < LOCK_FILE_OPEN_TRIES => {
                    tries += 1
                }
                opened => break opened.map_err(io_error)?,
            }
        };
        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(TryLockError::WouldBlock) => Err(StoreError::Locked(store_dir.to_path_buf())),
            Err(TryLockError::Error(e)) => Err(io_error(e)),
        }
    }

    fn open_lock_file(path: &Path) -> std::io::Result<File> {
        let mut options = File::options();
        options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        durable::no_follow(&mut options).open(path)
    }
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    pub busy: i64,
    pub wal_bytes: u64,
}

pub(crate) struct Db {
    conn: Connection,
    store_dir: Dir,
    log_name: std::ffi::OsString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Database {
    New,
    Existing,
}

const SCHEMA_TABLES: [&str; 3] = ["segment_keys", "events", "events_fts"];

impl Db {
    pub(crate) fn path(store_dir: &Path) -> PathBuf {
        store_dir.join("store.db")
    }

    fn side_files(store_dir: &Path) -> [PathBuf; 3] {
        let db = Self::path(store_dir);
        [
            db.clone(),
            db.with_extension("db-wal"),
            db.with_extension("db-shm"),
        ]
    }

    pub(crate) fn destroy(store_dir: &Path) -> StoreResult<()> {
        Self::side_files(store_dir)
            .iter()
            .try_for_each(|p| remove_if_present(p))
    }

    fn destroy_in(store_dir: &Dir) -> StoreResult<()> {
        [DATABASE_FILE, DATABASE_LOG, "store.db-shm"]
            .iter()
            .try_for_each(|name| remove_in(store_dir, name))
    }

    pub(crate) fn open(store_dir: &Dir, key: &[u8; 32]) -> StoreResult<Self> {
        Self::open_file(store_dir, key, Database::New)
    }

    pub(crate) fn open_existing(store_dir: &Dir, key: &[u8; 32]) -> StoreResult<Self> {
        Self::open_file(store_dir, key, Database::Existing)
    }

    fn refuse_a_linked_file(store_dir: &Dir) -> StoreResult<()> {
        match store_dir.entry(OsStr::new(DATABASE_FILE)) {
            Ok(entry) if entry.kind == EntryKind::Link => {
                Err(StoreError::Corrupt(LINKED_DATABASE.to_string()))
            }
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_error(e)),
        }
    }

    fn refuse_a_live_holder(store_dir: &Dir) -> StoreResult<()> {
        halvo_sqlcipher::initialize().map_err(StoreError::Db)?;
        match store_dir.entry(OsStr::new(DATABASE_FILE)) {
            Ok(entry) if entry.kind == EntryKind::File && entry.len > 0 => {}
            _ => return Ok(()),
        }
        let probe = match open_connection(store_dir, PROBE_OPEN) {
            Ok(probe) => probe,
            Err(StoreError::Db(_)) => return Ok(()),
            Err(e) => return Err(e),
        };
        probe.busy_timeout(Duration::ZERO).map_err(db_error)?;
        probe
            .execute_batch("PRAGMA locking_mode = EXCLUSIVE;")
            .map_err(db_error)?;
        let read = probe.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        });
        match read.map_err(|e| e.sqlite_error_code()) {
            Err(Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)) => {
                Err(StoreError::Locked(store_dir.path().to_path_buf()))
            }
            _ => Ok(()),
        }
    }

    fn require_file(store_dir: &Dir) -> StoreResult<()> {
        match store_dir.entry(OsStr::new(DATABASE_FILE)) {
            Ok(entry) if entry.kind == EntryKind::Link => {
                Err(StoreError::Corrupt(LINKED_DATABASE.to_string()))
            }
            Ok(entry) if entry.kind == EntryKind::File && entry.len == 0 => {
                Err(StoreError::DatabaseMissing)
            }
            Ok(entry) if entry.kind == EntryKind::File => Ok(()),
            Ok(_) => Err(StoreError::DatabaseUnreadable),
            Err(e) if e.kind() == ErrorKind::NotFound => Err(StoreError::DatabaseMissing),
            Err(e) => Err(io_error(e)),
        }
    }

    fn open_file(store_dir: &Dir, key: &[u8; 32], database: Database) -> StoreResult<Self> {
        if database == Database::Existing {
            Self::require_file(store_dir)?;
        }
        halvo_sqlcipher::initialize().map_err(StoreError::Db)?;
        let log_was_there = match store_dir.entry(OsStr::new(DATABASE_LOG)) {
            Ok(_) => true,
            Err(e) if e.kind() == ErrorKind::NotFound => false,
            Err(e) => return Err(io_error(e)),
        };
        if database == Database::New {
            store_dir
                .create_new(OsStr::new(DATABASE_FILE))
                .map_err(io_error)?;
        }
        let conn = open_database(store_dir, DATABASE_OPEN)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, log_was_there)
            .map_err(db_error)?;
        conn.busy_timeout(Duration::ZERO).map_err(db_error)?;
        conn.execute_batch(&key_pragma(key)).map_err(db_error)?;
        conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE;")
            .map_err(db_error)?;
        verify_key(&conn, store_dir.path())?;
        if database == Database::Existing {
            known_version(pragma_int(&conn, "user_version")?, DATABASE_VERSION)?;
        }
        conn.execute_batch(
            "PRAGMA secure_delete = ON;\n\
             PRAGMA temp_store = MEMORY;\n\
             PRAGMA foreign_keys = ON;\n\
             PRAGMA checkpoint_fullfsync = ON;\n\
             PRAGMA cipher_log_level = NONE;",
        )
        .map_err(db_error)?;
        require_write_ahead_log(&conn)?;
        require_secure_delete(&conn)?;
        require_exclusive_locking(&conn)?;
        require_checkpoint_fullfsync(&conn)?;
        require_cipher_log_level_none(&conn)?;
        require_pinned_cipher(&conn)?;
        let db = Self {
            conn,
            store_dir: store_dir.try_clone().map_err(io_error)?,
            log_name: DATABASE_LOG.into(),
        };
        match database {
            Database::New => {
                db.init_schema()?;
                db.conn
                    .execute_batch(&format!("PRAGMA user_version = {DATABASE_VERSION};"))
                    .map_err(db_error)?;
            }
            Database::Existing => db.require_schema()?,
        }
        db.enable_fts_secure_delete()?;
        failpoint::hit("db_open")?;
        db.conn
            .set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)
            .map_err(db_error)?;
        Ok(db)
    }

    fn init_schema(&self) -> StoreResult<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS segment_keys (\n\
                   id INTEGER PRIMARY KEY,\n\
                   segment_id TEXT NOT NULL UNIQUE,\n\
                   wrapped_key BLOB,\n\
                   created_at INTEGER NOT NULL,\n\
                   expires_at INTEGER NOT NULL\n\
                 );\n\
                 CREATE TABLE IF NOT EXISTS events (\n\
                   event_id TEXT PRIMARY KEY,\n\
                   wall_clock_nanos INTEGER NOT NULL,\n\
                   sequence INTEGER NOT NULL,\n\
                   modality TEXT NOT NULL,\n\
                   foreground_app TEXT NOT NULL,\n\
                   segment_id TEXT NOT NULL,\n\
                   byte_offset INTEGER NOT NULL,\n\
                   byte_len INTEGER NOT NULL,\n\
                   segment_key_id INTEGER NOT NULL,\n\
                   is_derived INTEGER NOT NULL,\n\
                   retention_class TEXT NOT NULL\n\
                 );\n\
                 CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(foreground_app);",
            )
            .map_err(db_error)
    }

    fn require_schema(&self) -> StoreResult<()> {
        for table in SCHEMA_TABLES {
            let defined: i64 = self
                .conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .map_err(unreadable_or_db_error)?;
            if defined != 1 {
                return Err(StoreError::DatabaseUnreadable);
            }
            self.conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(unreadable_or_db_error)?;
        }
        Ok(())
    }

    fn enable_fts_secure_delete(&self) -> StoreResult<()> {
        self.conn
            .execute_batch("INSERT INTO events_fts(events_fts, rank) VALUES('secure-delete', 1);")
            .map_err(db_error)
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    fn reserve_segment_key(
        &self,
        segment_id: uuid::Uuid,
        created_at: u64,
        expires_at: u64,
    ) -> StoreResult<SegmentKeyId> {
        self.conn
            .execute(
                "INSERT INTO segment_keys (segment_id, wrapped_key, created_at, expires_at) \
                 VALUES (?1, NULL, ?2, ?3)",
                rusqlite::params![
                    segment_id.simple().to_string(),
                    created_at as i64,
                    expires_at as i64
                ],
            )
            .map_err(db_error)?;
        Ok(SegmentKeyId(self.conn.last_insert_rowid()))
    }

    fn commit_segment_key(&self, id: SegmentKeyId, wrapped: &[u8]) -> StoreResult<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE segment_keys SET wrapped_key = ?1 WHERE id = ?2 AND wrapped_key IS NULL",
                rusqlite::params![wrapped, id.0],
            )
            .map_err(db_error)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(StoreError::Corrupt(format!(
                "segment key row {} was not reserved for this write",
                id.0
            )))
        }
    }

    pub(crate) fn insert_event(&self, event: &CaptureStreamEvent) -> StoreResult<()> {
        self.conn
            .execute_batch("SAVEPOINT insert_event;")
            .map_err(db_error)?;
        match self.insert_event_rows(event) {
            Ok(()) => self
                .conn
                .execute_batch("RELEASE insert_event;")
                .map_err(db_error),
            Err(cause) => match self
                .conn
                .execute_batch("ROLLBACK TO insert_event; RELEASE insert_event;")
            {
                Ok(()) => Err(cause),
                Err(e) => Err(db_error(e)),
            },
        }
    }

    fn insert_event_rows(&self, event: &CaptureStreamEvent) -> StoreResult<()> {
        self.conn
            .execute(
                "INSERT INTO events (event_id, wall_clock_nanos, sequence, modality, \
                   foreground_app, segment_id, byte_offset, byte_len, segment_key_id, \
                   is_derived, retention_class) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![
                    event.event_id.simple().to_string(),
                    unix_nanos(event.captured_at.wall_clock),
                    event.captured_at.sequence.get() as i64,
                    format!("{:?}", event.modality),
                    event.foreground_app,
                    event.segment_ref.segment_id.simple().to_string(),
                    event.segment_ref.byte_offset as i64,
                    event.segment_ref.byte_len as i64,
                    event.segment_key_id.0,
                    event.is_derived as i64,
                    format!("{:?}", event.retention_class),
                ],
            )
            .map_err(db_error)?;
        let rowid = self.conn.last_insert_rowid();
        self.conn
            .execute(
                "INSERT INTO events_fts (rowid, foreground_app) VALUES (?1, ?2)",
                rusqlite::params![rowid, event.foreground_app],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn pragma_int(&self, name: &str) -> StoreResult<i64> {
        pragma_int(&self.conn, name)
    }

    pub(crate) fn cipher_provider(&self) -> StoreResult<String> {
        self.conn
            .query_row("PRAGMA cipher_provider", [], |r| r.get::<_, String>(0))
            .map_err(db_error)
    }

    pub(crate) fn fts_secure_delete(&self) -> StoreResult<i64> {
        self.conn
            .query_row(
                "SELECT CAST(v AS INTEGER) FROM events_fts_config WHERE k='secure-delete'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map_err(db_error)
    }

    pub(crate) fn wal_bytes(&self) -> StoreResult<u64> {
        match self.store_dir.entry(&self.log_name) {
            Ok(entry) if entry.kind == EntryKind::File => Ok(entry.len),
            Ok(entry) if entry.kind == EntryKind::Link => {
                Err(StoreError::Corrupt(LINKED_SIDE_FILE.to_string()))
            }
            Ok(_) => Err(StoreError::Io(ErrorKind::InvalidData)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(0),
            Err(e) => Err(io_error(e)),
        }
    }

    pub(crate) fn checkpoint_truncate(&self) -> StoreResult<Checkpoint> {
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .map_err(db_error)?;
        let checkpoint = Checkpoint {
            busy,
            wal_bytes: self.wal_bytes()?,
        };
        if checkpoint.busy != 0 || checkpoint.wal_bytes != 0 {
            return Err(StoreError::CheckpointIncomplete {
                busy: checkpoint.busy,
                wal_bytes: checkpoint.wal_bytes,
            });
        }
        Ok(checkpoint)
    }
}

fn verify_key(conn: &Connection, store_dir: &Path) -> StoreResult<()> {
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|_| ())
    .map_err(|e| match e.sqlite_error_code() {
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => {
            StoreError::DatabaseUnreadable
        }
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
            StoreError::Locked(store_dir.to_path_buf())
        }
        _ => db_error(e),
    })
}

fn unreadable_or_db_error(e: rusqlite::Error) -> StoreError {
    match e.sqlite_error_code() {
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => {
            StoreError::DatabaseUnreadable
        }
        _ => db_error(e),
    }
}

fn require_secure_delete(conn: &Connection) -> StoreResult<()> {
    match pragma_int(conn, "secure_delete")? {
        1 => Ok(()),
        _ => Err(StoreError::SettingNotInEffect("secure_delete")),
    }
}

fn require_write_ahead_log(conn: &Connection) -> StoreResult<()> {
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL;", [], |r| r.get(0))
        .map_err(db_error)?;
    if mode == "wal" {
        Ok(())
    } else {
        Err(StoreError::SettingNotInEffect("journal_mode"))
    }
}

const CIPHER_PROVIDER: &str = "libtomcrypt";
const CIPHER_VERSION: &str = "4.19.0";

fn require_pinned_cipher(conn: &Connection) -> StoreResult<()> {
    let text = |pragma: &str| -> StoreResult<String> {
        conn.query_row(pragma, [], |r| r.get(0)).map_err(db_error)
    };
    cipher_in_effect(
        &text("PRAGMA cipher_provider")?,
        &text("PRAGMA cipher_version")?,
        halvo_sqlcipher::os_entropy_hook_installed(),
    )
}

fn cipher_in_effect(provider: &str, version: &str, os_entropy_hook: bool) -> StoreResult<()> {
    if provider != CIPHER_PROVIDER {
        return Err(StoreError::SettingNotInEffect("cipher_provider"));
    }
    if version.split_whitespace().next() != Some(CIPHER_VERSION) {
        return Err(StoreError::SettingNotInEffect("cipher_version"));
    }
    if !os_entropy_hook {
        return Err(StoreError::SettingNotInEffect("os_entropy_hook"));
    }
    Ok(())
}

fn require_exclusive_locking(conn: &Connection) -> StoreResult<()> {
    let mode: String = conn
        .query_row("PRAGMA main.locking_mode", [], |r| r.get(0))
        .map_err(db_error)?;
    if mode == "exclusive" {
        Ok(())
    } else {
        Err(StoreError::SettingNotInEffect("locking_mode"))
    }
}

fn require_cipher_log_level_none(conn: &Connection) -> StoreResult<()> {
    let level: String = conn
        .query_row("PRAGMA cipher_log_level", [], |r| r.get(0))
        .map_err(db_error)?;
    if level == "NONE" {
        Ok(())
    } else {
        Err(StoreError::SettingNotInEffect("cipher_log_level"))
    }
}

fn require_checkpoint_fullfsync(conn: &Connection) -> StoreResult<()> {
    match pragma_int(conn, "checkpoint_fullfsync")? {
        1 => Ok(()),
        _ => Err(StoreError::SettingNotInEffect("checkpoint_fullfsync")),
    }
}

fn pragma_int(conn: &Connection, name: &str) -> StoreResult<i64> {
    conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))
        .map_err(db_error)
}

const KEY_PRAGMA_HEAD: &str = "PRAGMA key = \"x'";
const KEY_PRAGMA_TAIL: &str = "'\";";

fn key_pragma(key: &[u8; 32]) -> Zeroizing<String> {
    let mut pragma = Zeroizing::new(String::with_capacity(
        KEY_PRAGMA_HEAD.len() + 64 + KEY_PRAGMA_TAIL.len(),
    ));
    pragma.push_str(KEY_PRAGMA_HEAD);
    pragma.push_str(&hex_lower(key));
    pragma.push_str(KEY_PRAGMA_TAIL);
    pragma
}

fn hex_lower(bytes: &[u8; 32]) -> Zeroizing<String> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = Zeroizing::new(String::with_capacity(64));
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

enum Keyed {
    Open { db: Db, store_key: StoreKey },
    Sealed,
}

/// The encrypted store: the SQLCipher database, the per-segment files
/// under `dir`, and the one store key in the clear only in memory.
pub struct EncryptedStore {
    dir: Dir,
    retention: Duration,
    custody: Box<dyn KeyCustody + Send>,
    keyed: Keyed,
    fs: Arc<dyn DurableFs>,
    unwrap_time: Duration,
    _writer_lock: WriterLock,
}

impl EncryptedStore {
    /// Create a fresh store in `dir`: open a new database under a random store
    /// key, wrap that key with `custody` and write the store-key file. A
    /// directory with a store-key file already holds a store; one without is
    /// free, and any keyless leftovers in it are removed. When a step fails,
    /// the call removes the database and key files that it made and destroys
    /// the root that it made, and answers the step's error; when one of those
    /// removals fails, it answers that removal's error in its place, and what
    /// it could not remove is left. The directory and its `writer.lock` stay.
    /// It opens `writer.lock` by name without `O_NONBLOCK`, so a FIFO there
    /// makes it wait for good.
    pub fn create(
        dir: &Path,
        custody: Box<dyn KeyCustody + Send>,
        retention: Duration,
    ) -> StoreResult<Self> {
        Self::create_with(dir, custody, retention, &mut WriterLock::open_lock_file)
    }

    pub(crate) fn create_with(
        dir: &Path,
        custody: Box<dyn KeyCustody + Send>,
        retention: Duration,
        open_lock: &mut dyn FnMut(&Path) -> std::io::Result<File>,
    ) -> StoreResult<Self> {
        durable::create_private_dir_all(dir).map_err(io_error)?;
        let store = Dir::open(dir).map_err(store_dir_error)?;
        store.make_private().map_err(io_error)?;
        let dir = &store;
        let _writer_lock = WriterLock::take_with(dir.path(), open_lock)?;
        if KeyFile::is_in(dir)? {
            return Err(StoreError::AlreadyExists(dir.path().to_path_buf()));
        }
        Db::destroy_in(dir)?;
        segment::remove_segments_tree(dir)?;
        remove_in(dir, KEY_FILE_TEMP)?;
        let store_key = StoreKey::random()?;
        let db = Db::open(dir, store_key.expose()).map_err(|e| Self::without_database(dir, e))?;
        let db = Self::under_a_new_root(dir, custody.as_ref(), db)?;
        match Self::write_key_file(dir, custody.as_ref(), &store_key) {
            Ok(()) => Ok(Self {
                dir: store,
                retention,
                custody,
                keyed: Keyed::Open { db, store_key },
                fs: Arc::new(OsFs),
                unwrap_time: Duration::ZERO,
                _writer_lock,
            }),
            Err(e) => {
                drop(db);
                Err(Self::abandon_create(dir, custody.as_ref(), e))
            }
        }
    }

    fn under_a_new_root(dir: &Dir, custody: &dyn KeyCustody, db: Db) -> StoreResult<Db> {
        match custody.create_root() {
            Ok(()) => Ok(db),
            Err(e) => {
                drop(db);
                Err(Self::without_database(dir, StoreError::Custody(e)))
            }
        }
    }

    fn write_key_file(
        dir: &Dir,
        custody: &dyn KeyCustody,
        store_key: &StoreKey,
    ) -> StoreResult<()> {
        let wrapped = custody.wrap_store_key(store_key)?;
        KeyFile {
            version: KEY_FILE_VERSION,
            custody_kind: custody.kind(),
            root_id: custody.root_id(),
            wrapped_store_key_b64: BASE64.encode(&wrapped.0),
        }
        .write(dir)
    }

    fn without_database(dir: &Dir, cause: StoreError) -> StoreError {
        match Db::destroy_in(dir) {
            Ok(()) => cause,
            Err(left_behind) => left_behind,
        }
    }

    fn abandon_create(dir: &Dir, custody: &dyn KeyCustody, cause: StoreError) -> StoreError {
        let root = custody.destroy_root().map_err(StoreError::Custody);
        let key_file = KeyFile::destroy(dir.path());
        let database = Db::destroy_in(dir);
        match root.and(key_file).and(database) {
            Ok(()) => cause,
            Err(left_behind) => left_behind,
        }
    }

    /// Open an existing store: read the store-key file, **unwrap the store key
    /// through the custody it names** and open the database. `open` never
    /// makes a database. Fails [`StoreError::Locked`] while another writer holds
    /// the directory, [`StoreError::Sealed`] when `store.key` is missing
    /// (delete-all ran, or no store was made here), [`StoreError::DatabaseMissing`]
    /// when `store.db` is missing or empty, and
    /// [`StoreError::DatabaseUnreadable`] when the unwrapped key does not open
    /// the database: a key of another store, a damaged page, a cut file, or a
    /// database without the store's tables. Fails
    /// [`StoreError::KeyFileUnparseable`] for a key file that does not parse or
    /// whose wrapped key has another length than a wrap, and
    /// [`StoreError::Custody`] with the custody's own variant when the custody
    /// kind is not in this build or the unwrap fails:
    /// [`CustodyError::RootMissing`], [`CustodyError::RootCorrupt`] and
    /// [`CustodyError::WrapAuthFailed`] mean that the key cannot come back.
    /// It opens `writer.lock` by name without `O_NONBLOCK`, so a FIFO there
    /// makes it wait for good.
    pub fn open(dir: &Path, retention: Duration) -> StoreResult<Self> {
        Self::open_with(dir, retention, custody_for)
    }

    pub(crate) fn open_with(
        dir: &Path,
        retention: Duration,
        custody_for: impl FnOnce(
            CustodyKind,
            RootId,
        ) -> Result<Box<dyn KeyCustody + Send>, CustodyError>,
    ) -> StoreResult<Self> {
        let store = Dir::open(dir).map_err(store_dir_error)?;
        store.make_private().map_err(io_error)?;
        let _writer_lock = WriterLock::take(store.path())?;
        let key_file = KeyFile::read_in(&store)?;
        let wrapped = key_file.wrapped_store_key()?;
        segment::refuse_a_linked_segments_root(&store)?;
        Db::refuse_a_linked_file(&store)?;
        Db::refuse_a_live_holder(&store)?;
        let custody = custody_for(key_file.custody_kind, key_file.root_id)?;
        let started = Instant::now();
        let store_key = custody.unwrap_store_key(&wrapped)?;
        let unwrap_time = started.elapsed();
        let db = Db::open_existing(&store, store_key.expose())?;
        Ok(Self {
            dir: store,
            retention,
            custody,
            keyed: Keyed::Open { db, store_key },
            fs: Arc::new(OsFs),
            unwrap_time,
            _writer_lock,
        })
    }

    fn keyed(&self) -> StoreResult<(&Db, &StoreKey)> {
        match &self.keyed {
            Keyed::Open { db, store_key } => Ok((db, store_key)),
            Keyed::Sealed => Err(StoreError::Sealed),
        }
    }

    pub(crate) fn db(&self) -> StoreResult<&Db> {
        self.keyed().map(|(db, _)| db)
    }

    pub(crate) fn dir(&self) -> &Path {
        self.dir.path()
    }

    pub(crate) fn handle(&self) -> &Dir {
        &self.dir
    }

    /// How long the custody unwrap of the store key took when `open` made this
    /// handle; zero for a handle from `create`, which unwraps nothing.
    pub fn unwrap_time(&self) -> Duration {
        self.unwrap_time
    }

    pub(crate) fn fs(&self) -> &dyn DurableFs {
        self.fs.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn use_fs(&mut self, fs: Arc<dyn DurableFs>) {
        self.fs = fs;
    }

    pub(crate) fn store_key(&self) -> StoreResult<&StoreKey> {
        self.keyed().map(|(_, key)| key)
    }

    pub(crate) fn seal(&mut self) {
        self.keyed = Keyed::Sealed;
    }

    pub(crate) fn is_sealed(&self) -> bool {
        matches!(self.keyed, Keyed::Sealed)
    }

    /// Read and decrypt a segment's plaintext: unwrap its per-segment key with the
    /// store key, then read the file twice, first to authenticate every chunk and
    /// then to write the plaintext to `out`. Nothing reaches `out` before the first
    /// read has authenticated the whole file; a file that changes between the two
    /// reads ends in an error, after `out` has taken the chunks that still
    /// authenticate. Fails [`StoreError::Sealed`] if the segment has been
    /// crypto-shredded or never finished writing (its key row is NULL or gone),
    /// or if this store never wrote a segment with that id, and
    /// [`StoreError::Corrupt`] if the file does not authenticate.
    pub fn read_segment(&self, segment_id: uuid::Uuid, out: &mut dyn Write) -> StoreResult<u64> {
        let (db, store_key) = self.keyed()?;
        let wrapped: Option<Vec<u8>> = db
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
                [segment_id.simple().to_string()],
                |r| r.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => StoreError::Sealed,
                other => db_error(other),
            })?;
        let wrapped = wrapped.ok_or(StoreError::Sealed)?;
        let seg_key = segment::unwrap_segment_key(store_key.expose(), &wrapped)?;
        segment::read_segment_file(&self.dir, &seg_key, segment_id, out)
    }

    pub(crate) fn destroy_root(&self) -> Result<(), CustodyError> {
        self.custody.destroy_root()
    }

    pub(crate) fn write_segment_with_expiry(
        &mut self,
        bytes: &mut dyn Read,
        created_at: u64,
        expires_at: u64,
    ) -> StoreResult<(SegmentRef, SegmentKeyId)> {
        let (db, store_key) = self.keyed()?;
        let segment_id = uuid::Uuid::now_v7();
        let seg_key = SegmentKey::random()?;
        let wrapped = segment::wrap_segment_key(store_key.expose(), &seg_key)?;
        let segment_key_id = db.reserve_segment_key(segment_id, created_at, expires_at)?;
        let header = SegmentHeader {
            segment_id,
            segment_key_id,
        };
        let byte_len =
            segment::write_segment_file(&self.dir, self.fs.as_ref(), &seg_key, &header, bytes)?;
        if let Err(e) = db.commit_segment_key(segment_key_id, &wrapped) {
            segment::remove_written_segment(&self.dir, segment_id)?;
            return Err(e);
        }
        Ok((
            SegmentRef {
                segment_id,
                byte_offset: 0,
                byte_len,
            },
            segment_key_id,
        ))
    }
}

impl SegmentStore for EncryptedStore {
    fn append_event(&mut self, event: &CaptureStreamEvent) -> StoreResult<()> {
        self.db()?.insert_event(event)
    }

    fn write_segment(&mut self, bytes: &mut dyn Read) -> StoreResult<(SegmentRef, SegmentKeyId)> {
        let created = unix_secs_now();
        let expires = created + self.retention.as_secs();
        self.write_segment_with_expiry(bytes, created, expires)
    }

    fn size_bytes(&self) -> StoreResult<u64> {
        dir_size(&self.dir)
    }
}

fn listed(dir: &Dir) -> StoreResult<Vec<durable::Entry>> {
    match dir.entries() {
        Ok(entries) => Ok(entries),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(io_error(e)),
    }
}

/// The bytes of the regular files below `dir`, each directory opened and
/// listed through the handle of its parent; a link is neither followed nor
/// counted.
pub(crate) fn dir_size(dir: &Dir) -> StoreResult<u64> {
    type Pending = Vec<(Option<Rc<Dir>>, std::ffi::OsString)>;
    fn take(
        parent: Option<&Rc<Dir>>,
        entries: Vec<durable::Entry>,
        total: &mut u64,
        pending: &mut Pending,
    ) {
        for entry in entries {
            match entry.kind {
                EntryKind::File => *total += entry.len,
                EntryKind::Dir => pending.push((parent.cloned(), entry.name)),
                EntryKind::Link | EntryKind::Other => {}
            }
        }
    }
    let mut total = 0u64;
    let mut pending = Pending::new();
    take(None, listed(dir)?, &mut total, &mut pending);
    #[cfg(test)]
    failpoint::seam(SEAM_SIZE_AFTER_THE_LISTING);
    while let Some((parent, name)) = pending.pop() {
        let opened = match &parent {
            Some(parent) => parent.open_dir(&name),
            None => dir.open_dir(&name),
        };
        let below = match opened {
            Ok(below) => Rc::new(below),
            Err(e) if e.kind() == ErrorKind::NotFound || durable::refusal(&e).is_some() => continue,
            Err(e) => return Err(io_error(e)),
        };
        take(Some(&below), listed(&below)?, &mut total, &mut pending);
    }
    Ok(total)
}

#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) use crate::store::pages;

    use std::ops::{Deref, DerefMut};
    use std::path::Path;

    use super::EncryptedStore;
    use crate::custody::dev_fallback::DevFallbackCustody;
    use crate::store::roots::RootGuard;
    use crate::store::DEFAULT_RETENTION;

    pub(crate) struct TestStore {
        store: EncryptedStore,
        _root: RootGuard,
    }

    impl Deref for TestStore {
        type Target = EncryptedStore;
        fn deref(&self) -> &EncryptedStore {
            &self.store
        }
    }

    impl DerefMut for TestStore {
        fn deref_mut(&mut self) -> &mut EncryptedStore {
            &mut self.store
        }
    }

    pub(crate) fn make_store(dir: &Path) -> TestStore {
        let root = RootGuard::fresh();
        let custody = DevFallbackCustody::new(root.id());
        TestStore {
            store: EncryptedStore::create(dir, Box::new(custody), DEFAULT_RETENTION).unwrap(),
            _root: root,
        }
    }

    #[cfg(unix)]
    pub(crate) fn duplicate_writer_lock_descriptor(store: &EncryptedStore) -> std::fs::File {
        store._writer_lock.file.try_clone().unwrap()
    }

    pub(crate) fn key_statement(key: &[u8; 32]) -> zeroize::Zeroizing<String> {
        super::key_pragma(key)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::make_store;
    use super::*;
    use crate::custody::dev_fallback::DevFallbackCustody;
    use crate::event::{CapturedAt, Modality, MonotonicSequence};
    use crate::store::roots::RootGuard;
    use crate::store::{SegmentStore, DEFAULT_RETENTION};

    #[test]
    fn the_key_pragma_is_built_in_one_allocation_that_never_grows() {
        let pragma = key_pragma(&[0xab; 32]);
        assert_eq!(pragma.capacity(), pragma.len());
        assert!(pragma.starts_with(KEY_PRAGMA_HEAD) && pragma.ends_with(KEY_PRAGMA_TAIL));
        let hex = hex_lower(&[0xab; 32]);
        assert_eq!(hex.capacity(), hex.len());
    }

    #[cfg(unix)]
    #[test]
    fn a_write_ahead_log_that_cannot_be_read_fails_the_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open(&Dir::open(dir.path()).unwrap(), &[7u8; 32]).unwrap();
        db.log_name = "no-such-wal".into();
        assert!(
            db.checkpoint_truncate().is_ok(),
            "a missing write-ahead log is not an empty one"
        );
        let looping = dir.path().join("looping-wal");
        std::os::unix::fs::symlink(&looping, &looping).unwrap();
        db.log_name = "looping-wal".into();
        let checkpoint = db.checkpoint_truncate();
        assert!(
            checkpoint.is_err(),
            "a write-ahead log that cannot be read let the checkpoint succeed: {checkpoint:?}"
        );
    }

    #[test]
    fn an_event_whose_index_row_fails_leaves_no_event_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, key_id) = store
            .write_segment(&mut std::io::Cursor::new(b"segment"))
            .unwrap();
        store
            .db()
            .unwrap()
            .connection()
            .execute_batch("DROP TABLE events_fts;")
            .unwrap();
        let event = CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: SystemTime::now(),
                sequence: MonotonicSequence::new(1),
            },
            Modality::ScreenFrame,
            "app".to_string(),
            seg_ref,
            key_id,
        );
        let appended = store.append_event(&event);
        let rows: i64 = store
            .db()
            .unwrap()
            .connection()
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert!(
            appended.is_err(),
            "the append with no index table succeeded"
        );
        assert_eq!(rows, 0, "a failed append left its event row");
    }

    #[cfg(unix)]
    #[test]
    fn a_store_directory_that_cannot_be_read_fails_the_size_and_does_not_shrink_it() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, _) = store
            .write_segment(&mut std::io::Cursor::new(b"segment"))
            .unwrap();
        let shard = segment::segment_path(dir.path(), seg_ref.segment_id)
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o000)).unwrap();
        let size = store.size_bytes();
        std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            matches!(size, Err(StoreError::Io(ErrorKind::PermissionDenied))),
            "{size:?}"
        );
    }

    #[test]
    fn a_checkpoint_that_leaves_bytes_in_the_wal_is_incomplete_and_no_sqlite_code() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open(&Dir::open(dir.path()).unwrap(), &[7u8; 32]).unwrap();
        let wal = dir.path().join("a-wal-that-keeps-bytes");
        std::fs::write(&wal, [1u8; 100]).unwrap();
        db.log_name = "a-wal-that-keeps-bytes".into();
        let checkpoint = db.checkpoint_truncate();
        assert!(
            matches!(
                checkpoint,
                Err(StoreError::CheckpointIncomplete {
                    busy: 0,
                    wal_bytes: 100
                })
            ),
            "{checkpoint:?}"
        );
    }

    #[test]
    fn a_key_row_that_holds_a_key_is_not_committed_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (_, key_id) = store
            .write_segment(&mut std::io::Cursor::new(b"segment"))
            .unwrap();
        let again = store.db().unwrap().commit_segment_key(key_id, &[1u8; 72]);
        assert!(matches!(again, Err(StoreError::Corrupt(_))), "{again:?}");
    }

    #[test]
    fn a_wrapped_segment_key_that_does_not_open_under_the_store_key_reads_as_corrupt_not_sealed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (segment, _) = store
            .write_segment(&mut std::io::Cursor::new(b"segment"))
            .unwrap();
        store
            .db()
            .unwrap()
            .connection()
            .execute(
                "UPDATE segment_keys SET wrapped_key = ?1 WHERE segment_id = ?2",
                rusqlite::params![vec![7u8; 72], segment.segment_id.simple().to_string()],
            )
            .unwrap();
        let read = store.read_segment(segment.segment_id, &mut Vec::new());
        assert!(matches!(read, Err(StoreError::Corrupt(_))), "{read:?}");
    }

    #[test]
    fn pragmas_are_set() {
        let dir = tempfile::tempdir().unwrap();
        let store = make_store(dir.path());
        let db = store.db().unwrap();
        assert_eq!(db.pragma_int("secure_delete").unwrap(), 1);
        assert_eq!(db.pragma_int("temp_store").unwrap(), 2);
        assert_eq!(db.fts_secure_delete().unwrap(), 1);
        assert_eq!(db.pragma_int("checkpoint_fullfsync").unwrap(), 1);
        let text = |pragma: &str| -> String {
            db.connection().query_row(pragma, [], |r| r.get(0)).unwrap()
        };
        assert_eq!(text("PRAGMA journal_mode"), "wal");
        assert_eq!(text("PRAGMA main.locking_mode"), "exclusive");
        assert_eq!(text("PRAGMA cipher_log_level"), "NONE");
        assert_eq!(db.cipher_provider().unwrap(), "libtomcrypt");
        assert!(
            text("PRAGMA cipher_version").starts_with("4.19.0"),
            "{}",
            text("PRAGMA cipher_version")
        );
        assert!(
            halvo_sqlcipher::os_entropy_hook_installed(),
            "LibTomCrypt's random source is not the OS entropy hook"
        );
    }

    #[test]
    fn a_cipher_other_than_the_pinned_one_is_refused() {
        let cases = [
            ("libtomcrypt", "4.19.0 community", true, None),
            ("openssl", "4.19.0 community", true, Some("cipher_provider")),
            (
                "libtomcrypt",
                "4.6.1 community",
                true,
                Some("cipher_version"),
            ),
            (
                "libtomcrypt",
                "4.19.01 community",
                true,
                Some("cipher_version"),
            ),
            (
                "libtomcrypt",
                "4.19.0 community",
                false,
                Some("os_entropy_hook"),
            ),
        ];
        for (provider, version, hook, refused) in cases {
            let answer = cipher_in_effect(provider, version, hook);
            match refused {
                None => assert!(answer.is_ok(), "{provider} {version} {hook}: {answer:?}"),
                Some(name) => assert!(
                    matches!(answer, Err(StoreError::SettingNotInEffect(n)) if n == name),
                    "{provider} {version} {hook}: {answer:?}"
                ),
            }
        }
    }

    fn persistent_store(dir: &Path) -> (EncryptedStore, RootId) {
        let root_id = RootId::fresh();
        let custody = Box::new(DevFallbackCustody::new(root_id));
        let store = EncryptedStore::create(dir, custody, DEFAULT_RETENTION).unwrap();
        (store, root_id)
    }

    #[test]
    fn a_second_writer_on_the_same_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (first, root_id) = persistent_store(dir.path());
        let _root = RootGuard::new(root_id);
        assert!(matches!(
            EncryptedStore::open(dir.path(), DEFAULT_RETENTION),
            Err(StoreError::Locked(_))
        ));
        drop(first);
        EncryptedStore::open(dir.path(), DEFAULT_RETENTION)
            .expect("a dropped writer leaves the store free to reopen");
        DevFallbackCustody::new(root_id).destroy_root().unwrap();
    }

    #[test]
    #[ignore = "a measure of contention, 1,000,000 takes of the lock while another thread unlinks the file every 1 ms, 30 to 50 s on a Mac; the seam tests of the retry run by default; run it by hand when the code of WriterLock changes: cargo test -p halvo-capture --lib -- --ignored --exact store::db::tests::a_take_of_the_writer_lock_never_answers_not_found_while_another_thread_unlinks_the_file --nocapture"]
    fn a_take_of_the_writer_lock_never_answers_not_found_while_another_thread_unlinks_the_file() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;

        const TAKERS: usize = 5;
        #[cfg(not(windows))]
        const TAKES: u64 = 1_000_000;
        #[cfg(windows)]
        const TAKES: u64 = 20_000;

        let dir = tempfile::tempdir().unwrap();
        let taken = Arc::new(AtomicU64::new(0));
        let takers: Vec<_> = (0..TAKERS)
            .map(|_| {
                let (dir, taken) = (dir.path().to_path_buf(), Arc::clone(&taken));
                std::thread::spawn(move || {
                    let (mut not_found, mut first_not_found, mut denied) = (0u64, None, 0u64);
                    let mut other = Vec::new();
                    loop {
                        let take = taken.fetch_add(1, Ordering::Relaxed) + 1;
                        if take > TAKES {
                            break;
                        }
                        durable::make_private(&dir).unwrap();
                        match WriterLock::take(&dir) {
                            Ok(lock) => drop(lock),
                            Err(StoreError::Locked(_)) => {}
                            Err(StoreError::Io(ErrorKind::NotFound)) => {
                                not_found += 1;
                                first_not_found.get_or_insert(take);
                            }
                            Err(StoreError::Io(ErrorKind::PermissionDenied)) if cfg!(windows) => {
                                denied += 1
                            }
                            Err(e) => other.push(format!("{e:?}")),
                        }
                    }
                    (not_found, first_not_found, denied, other)
                })
            })
            .collect();
        let lock_file = dir.path().join("writer.lock");
        let mut unlinks = 0u64;
        while !takers.iter().all(|taker| taker.is_finished()) {
            if std::fs::remove_file(&lock_file).is_ok() {
                unlinks += 1;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let (mut not_found, mut first_not_found, mut denied, mut other) =
            (0u64, None::<u64>, 0u64, Vec::new());
        for taker in takers {
            let (n, first, d, o) = taker.join().unwrap();
            not_found += n;
            first_not_found = match (first_not_found, first) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            denied += d;
            other.extend(o);
        }
        println!(
            "TAKES takes={TAKES} takers={TAKERS} unlinks={unlinks} not_found={not_found} \
             first_not_found_at_take={first_not_found:?} permission_denied={denied} other_errors={}",
            other.len()
        );
        assert!(unlinks > 0, "the file was never unlinked during the takes");
        assert_eq!(
            not_found, 0,
            "{not_found} of {TAKES} takes answered Io(NotFound), the first at take {first_not_found:?}"
        );
        assert!(other.is_empty(), "takes answered {other:?}");
    }

    fn take_whose_first_opens_answer(failing: u32, kind: ErrorKind) -> (StoreResult<()>, u32) {
        let dir = tempfile::tempdir().unwrap();
        let mut tries = 0;
        let taken = WriterLock::take_with(dir.path(), &mut |path| {
            tries += 1;
            if tries <= failing {
                Err(std::io::Error::from(kind))
            } else {
                WriterLock::open_lock_file(path)
            }
        })
        .map(drop);
        (taken, tries)
    }

    #[test]
    fn a_take_tries_the_open_of_the_lock_file_again_while_it_answers_not_found() {
        let mut wrong = Vec::new();
        for failing in 0..=3 {
            let (taken, tries) = take_whose_first_opens_answer(failing, ErrorKind::NotFound);
            if taken.is_err() || tries != failing + 1 {
                wrong.push(format!(
                    "{failing} opens that answer NotFound: {taken:?} after {tries} tries"
                ));
            }
        }
        let (taken, tries) = take_whose_first_opens_answer(4, ErrorKind::NotFound);
        if !matches!(taken, Err(StoreError::Io(ErrorKind::NotFound))) || tries != 4 {
            wrong.push(format!(
                "4 opens that answer NotFound: {taken:?} after {tries} tries"
            ));
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn a_take_hands_on_an_open_error_of_another_kind_after_one_try() {
        let (taken, tries) = take_whose_first_opens_answer(1, ErrorKind::PermissionDenied);
        assert!(
            matches!(taken, Err(StoreError::Io(ErrorKind::PermissionDenied))) && tries == 1,
            "an open that answered PermissionDenied: {taken:?} after {tries} tries"
        );
    }

    #[test]
    fn a_take_of_the_writer_lock_in_a_directory_that_is_gone_answers_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        let taken = WriterLock::take(&gone).map(drop);
        assert!(
            matches!(taken, Err(StoreError::Io(ErrorKind::NotFound))),
            "a take in a directory that is gone answered {taken:?}"
        );
        assert!(!gone.exists(), "a take made the directory that was gone");
    }

    #[test]
    fn a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let first = WriterLock::take(dir.path()).unwrap();
        let second_beside_the_first = WriterLock::take(dir.path()).map(drop);
        assert!(
            matches!(second_beside_the_first, Err(StoreError::Locked(_))),
            "a second take beside a live writer lock answered {second_beside_the_first:?}"
        );
        drop(first);
        WriterLock::take(dir.path())
            .expect("a dropped writer lock leaves the directory free to lock");
    }

    #[cfg(unix)]
    #[test]
    fn a_dropped_writer_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open() {
        let dir = tempfile::tempdir().unwrap();
        let never_put_in_a_store = WriterLock::take(dir.path()).unwrap();
        let held_as_a_spawned_child_holds_it = never_put_in_a_store.file.try_clone().unwrap();

        drop(never_put_in_a_store);
        let taken_again = WriterLock::take(dir.path()).map(drop);

        drop(held_as_a_spawned_child_holds_it);
        assert!(
            taken_again.is_ok(),
            "a writer lock dropped outside a store was still held: {:?}",
            taken_again.err()
        );
    }

    #[test]
    fn create_refuses_a_directory_that_already_holds_a_store() {
        let dir = tempfile::tempdir().unwrap();
        drop(make_store(dir.path()));
        let second_root = RootGuard::fresh();
        let again = EncryptedStore::create(
            dir.path(),
            Box::new(DevFallbackCustody::new(second_root.id())),
            DEFAULT_RETENTION,
        );
        assert!(matches!(again, Err(StoreError::AlreadyExists(_))));
    }

    #[test]
    fn open_reports_a_missing_root_as_root_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (store, root_id) = persistent_store(dir.path());
        let _root = RootGuard::new(root_id);
        drop(store);
        DevFallbackCustody::new(root_id).destroy_root().unwrap();
        assert!(matches!(
            EncryptedStore::open(dir.path(), DEFAULT_RETENTION),
            Err(StoreError::Custody(CustodyError::RootMissing))
        ));
    }

    #[test]
    fn open_rebuilds_the_custody_from_the_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, root_id) = persistent_store(dir.path());
        let _root = RootGuard::new(root_id);
        let (seg_ref, _) = store
            .write_segment(&mut std::io::Cursor::new(b"persisted"))
            .unwrap();
        drop(store);
        let reopened = EncryptedStore::open(dir.path(), DEFAULT_RETENTION).unwrap();
        let mut plain = Vec::new();
        reopened
            .read_segment(seg_ref.segment_id, &mut plain)
            .unwrap();
        assert_eq!(plain, b"persisted");
        reopened.destroy_root().unwrap();
    }

    #[test]
    fn writes_and_reads_a_segment_and_event() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, key_id) = store
            .write_segment(&mut std::io::Cursor::new(b"screen frame bytes"))
            .unwrap();
        let event = CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: SystemTime::now(),
                sequence: MonotonicSequence::new(1),
            },
            Modality::ScreenFrame,
            "com.apple.Terminal".to_string(),
            seg_ref.clone(),
            key_id,
        );
        store.append_event(&event).unwrap();
        let wrapped: Vec<u8> = store
            .db()
            .unwrap()
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE id=?1",
                [key_id.0],
                |r| r.get(0),
            )
            .unwrap();
        let seg_key =
            segment::unwrap_segment_key(store.store_key().unwrap().expose(), &wrapped).unwrap();
        let mut plain = Vec::new();
        segment::read_segment_file(store.handle(), &seg_key, seg_ref.segment_id, &mut plain)
            .unwrap();
        assert_eq!(plain, b"screen frame bytes");
        let mut again = Vec::new();
        assert_eq!(
            store.read_segment(seg_ref.segment_id, &mut again).unwrap(),
            18
        );
        assert_eq!(again, b"screen frame bytes");
    }

    #[test]
    fn the_open_flag_constants_carry_no_follow_and_neither_uri_nor_create() {
        for flags in [DATABASE_OPEN, PROBE_OPEN] {
            assert!(flags.contains(OpenFlags::SQLITE_OPEN_NOFOLLOW), "{flags:?}");
            assert!(!flags.contains(OpenFlags::SQLITE_OPEN_URI), "{flags:?}");
            assert!(!flags.contains(OpenFlags::SQLITE_OPEN_CREATE), "{flags:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn sqlite_under_the_constant_flags_refuses_a_link_and_the_store_maps_it_to_corrupt() {
        halvo_sqlcipher::initialize().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let resolved = Dir::open(dir.path()).unwrap();
        let target = resolved.path().join("target.db");
        Connection::open(&target)
            .unwrap()
            .execute_batch("CREATE TABLE t (x INTEGER);")
            .unwrap();
        let bytes = std::fs::read(&target).unwrap();
        let link = resolved.path().join("store.db");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        for flags in [DATABASE_OPEN, PROBE_OPEN] {
            let answer = Connection::open_with_flags(&link, flags)
                .and_then(|conn| {
                    conn.query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0))
                })
                .map_err(database_open_error);
            assert!(
                matches!(&answer, Err(StoreError::Corrupt(text)) if text == LINKED_DATABASE),
                "{flags:?}: {answer:?}"
            );
        }
        let mut names: Vec<String> = std::fs::read_dir(resolved.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["store.db", "target.db"]);
        assert!(std::fs::read(&target).unwrap() == bytes);
    }
}
