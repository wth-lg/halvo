//! SQLCipher metadata store and the [`EncryptedStore`].

use std::fs::{File, TryLockError};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use halvo_sqlcipher::rusqlite::{self, Connection, ErrorCode};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::custody::{
    custody_for, CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey,
};
use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

use super::segment::{self, SegmentHeader, SegmentKey};
use super::{SegmentStore, StoreError, StoreResult};

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

    fn write(&self, store_dir: &Path) -> StoreResult<()> {
        let json =
            serde_json::to_vec_pretty(self).map_err(|_| StoreError::Io(ErrorKind::InvalidData))?;
        std::fs::write(Self::path(store_dir), json).map_err(io_error)
    }

    pub(crate) fn read(store_dir: &Path) -> StoreResult<Self> {
        let json = match std::fs::read(Self::path(store_dir)) {
            Ok(json) => json,
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(StoreError::Sealed),
            Err(e) => return Err(io_error(e)),
        };
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
        match File::options().write(true).open(&path) {
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

#[cfg(test)]
pub(crate) mod failpoint {
    use std::cell::Cell;

    use super::{StoreError, StoreResult};

    thread_local! {
        static ARMED: Cell<Option<&'static str>> = const { Cell::new(None) };
    }

    pub(crate) fn arm(name: &'static str) {
        ARMED.with(|armed| armed.set(Some(name)));
    }

    pub(crate) fn hit(name: &'static str) -> StoreResult<()> {
        if ARMED.with(|armed| armed.get()) == Some(name) {
            ARMED.with(|armed| armed.set(None));
            return Err(StoreError::Io(std::io::ErrorKind::Other));
        }
        Ok(())
    }
}

#[cfg(not(test))]
mod failpoint {
    pub(super) fn hit(_name: &'static str) -> super::StoreResult<()> {
        Ok(())
    }
}

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

fn remove_dir_if_present(path: &Path) -> StoreResult<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}

struct WriterLock {
    file: File,
}

impl WriterLock {
    fn take(store_dir: &Path) -> StoreResult<Self> {
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(store_dir.join("writer.lock"))
            .map_err(io_error)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(TryLockError::WouldBlock) => Err(StoreError::Locked(store_dir.to_path_buf())),
            Err(TryLockError::Error(e)) => Err(io_error(e)),
        }
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
    wal_path: PathBuf,
}

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

    pub(crate) fn open(store_dir: &Path, key: &[u8; 32]) -> StoreResult<Self> {
        let path = Self::path(store_dir);
        let conn = Connection::open(&path).map_err(db_error)?;
        conn.busy_timeout(Duration::ZERO).map_err(db_error)?;
        conn.execute_batch(&key_pragma(key)).map_err(db_error)?;
        conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE;")
            .map_err(db_error)?;
        verify_key(&conn, store_dir)?;
        conn.execute_batch(
            "PRAGMA secure_delete = ON;\n\
             PRAGMA temp_store = MEMORY;\n\
             PRAGMA journal_mode = WAL;\n\
             PRAGMA foreign_keys = ON;",
        )
        .map_err(db_error)?;
        require_secure_delete(&conn)?;
        require_exclusive_locking(&conn)?;
        let db = Self {
            conn,
            wal_path: path.with_extension("db-wal"),
        };
        db.init_schema()?;
        db.enable_fts_secure_delete()?;
        failpoint::hit("db_open")?;
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
        match std::fs::metadata(&self.wal_path) {
            Ok(metadata) => Ok(metadata.len()),
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
        Some(ErrorCode::NotADatabase) => StoreError::Sealed,
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
            StoreError::Locked(store_dir.to_path_buf())
        }
        _ => db_error(e),
    })
}

fn require_secure_delete(conn: &Connection) -> StoreResult<()> {
    match pragma_int(conn, "secure_delete")? {
        1 => Ok(()),
        _ => Err(StoreError::SettingNotInEffect("secure_delete")),
    }
}

fn require_exclusive_locking(conn: &Connection) -> StoreResult<()> {
    let mode: String = conn
        .query_row("PRAGMA locking_mode", [], |r| r.get(0))
        .map_err(db_error)?;
    if mode == "exclusive" {
        Ok(())
    } else {
        Err(StoreError::SettingNotInEffect("locking_mode"))
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
    dir: PathBuf,
    retention: Duration,
    custody: Box<dyn KeyCustody + Send>,
    keyed: Keyed,
    _writer_lock: WriterLock,
}

impl EncryptedStore {
    /// Create a fresh store in `dir`: open a new database under a random store
    /// key, wrap that key with `custody` and write the store-key file. A
    /// directory with a store-key file already holds a store; one without is
    /// free, and any keyless leftovers in it are removed. When a step fails, the
    /// database and key files that the call made are removed and the root that
    /// it made is destroyed; the directory and its `writer.lock` stay.
    pub fn create(
        dir: &Path,
        custody: Box<dyn KeyCustody + Send>,
        retention: Duration,
    ) -> StoreResult<Self> {
        std::fs::create_dir_all(dir).map_err(io_error)?;
        let _writer_lock = WriterLock::take(dir)?;
        if KeyFile::path(dir).exists() {
            return Err(StoreError::AlreadyExists(dir.to_path_buf()));
        }
        Db::destroy(dir)?;
        remove_dir_if_present(&segment::segments_root(dir))?;
        let store_key = StoreKey::random()?;
        let db = Db::open(dir, store_key.expose()).map_err(|e| Self::without_database(dir, e))?;
        let db = Self::under_a_new_root(dir, custody.as_ref(), db)?;
        match Self::write_key_file(dir, custody.as_ref(), &store_key) {
            Ok(()) => Ok(Self {
                dir: dir.to_path_buf(),
                retention,
                custody,
                keyed: Keyed::Open { db, store_key },
                _writer_lock,
            }),
            Err(e) => {
                drop(db);
                Err(Self::abandon_create(dir, custody.as_ref(), e))
            }
        }
    }

    fn under_a_new_root(dir: &Path, custody: &dyn KeyCustody, db: Db) -> StoreResult<Db> {
        match custody.create_root() {
            Ok(()) => Ok(db),
            Err(e) => {
                drop(db);
                Err(Self::without_database(dir, StoreError::Custody(e)))
            }
        }
    }

    fn write_key_file(
        dir: &Path,
        custody: &dyn KeyCustody,
        store_key: &StoreKey,
    ) -> StoreResult<()> {
        let wrapped = custody.wrap_store_key(store_key)?;
        KeyFile {
            version: 1,
            custody_kind: custody.kind(),
            root_id: custody.root_id(),
            wrapped_store_key_b64: BASE64.encode(&wrapped.0),
        }
        .write(dir)
    }

    fn without_database(dir: &Path, cause: StoreError) -> StoreError {
        match Db::destroy(dir) {
            Ok(()) => cause,
            Err(left_behind) => left_behind,
        }
    }

    fn abandon_create(dir: &Path, custody: &dyn KeyCustody, cause: StoreError) -> StoreError {
        let root = custody.destroy_root().map_err(StoreError::Custody);
        let key_file = KeyFile::destroy(dir);
        let database = Db::destroy(dir);
        match root.and(key_file).and(database) {
            Ok(()) => cause,
            Err(left_behind) => left_behind,
        }
    }

    /// Open an existing store: read the store-key file, **unwrap the store key
    /// through the custody it names** and open the database. A `store.db` that
    /// is missing or empty is made as a new, empty database: the store opens,
    /// and none of the segments or events that it held reads again. Fails
    /// [`StoreError::Locked`] while another writer holds the directory, and
    /// [`StoreError::Sealed`] when `store.key` is missing (delete-all ran, or no
    /// store was made here) and when the unwrapped key does not open the
    /// database: a key of another store, a damaged first page, or a file
    /// shorter than one page. Fails [`StoreError::KeyFileUnparseable`] for a key
    /// file that does not parse or whose wrapped key has another length than a
    /// wrap, and [`StoreError::Custody`] with the custody's own variant when the
    /// custody kind is not in this build or the unwrap fails:
    /// [`CustodyError::RootMissing`], [`CustodyError::RootCorrupt`] and
    /// [`CustodyError::WrapAuthFailed`] mean that the key cannot come back.
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
        let _writer_lock = WriterLock::take(dir)?;
        let key_file = KeyFile::read(dir)?;
        let wrapped = key_file.wrapped_store_key()?;
        let custody = custody_for(key_file.custody_kind, key_file.root_id)?;
        let store_key = custody.unwrap_store_key(&wrapped)?;
        let db = Db::open(dir, store_key.expose())?;
        Ok(Self {
            dir: dir.to_path_buf(),
            retention,
            custody,
            keyed: Keyed::Open { db, store_key },
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
        &self.dir
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
        let byte_len = segment::write_segment_file(&self.dir, &seg_key, &header, bytes)?;
        if let Err(e) = db.commit_segment_key(segment_key_id, &wrapped) {
            remove_if_present(&segment::segment_path(&self.dir, segment_id))?;
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

pub(crate) fn dir_size(dir: &Path) -> StoreResult<u64> {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        let entries = match std::fs::read_dir(&p) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(io_error(e)),
        };
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(io_error(e)),
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
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
    use crate::custody::RootId;
    use crate::store::DEFAULT_RETENTION;

    pub(crate) struct TestStore(EncryptedStore);

    impl Deref for TestStore {
        type Target = EncryptedStore;
        fn deref(&self) -> &EncryptedStore {
            &self.0
        }
    }

    impl DerefMut for TestStore {
        fn deref_mut(&mut self) -> &mut EncryptedStore {
            &mut self.0
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            self.0
                .destroy_root()
                .expect("the test store's platform root is destroyed when the test ends");
        }
    }

    pub(crate) fn make_store(dir: &Path) -> TestStore {
        let custody = DevFallbackCustody::new(RootId::fresh());
        TestStore(EncryptedStore::create(dir, Box::new(custody), DEFAULT_RETENTION).unwrap())
    }

    #[cfg(unix)]
    pub(crate) fn duplicate_writer_lock_descriptor(store: &EncryptedStore) -> std::fs::File {
        store._writer_lock.file.try_clone().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::make_store;
    use super::*;
    use crate::custody::dev_fallback::DevFallbackCustody;
    use crate::event::{CapturedAt, Modality, MonotonicSequence};
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
        let mut db = Db::open(dir.path(), &[7u8; 32]).unwrap();
        db.wal_path = dir.path().join("no-such-wal");
        assert!(
            db.checkpoint_truncate().is_ok(),
            "a missing write-ahead log is not an empty one"
        );
        let looping = dir.path().join("looping-wal");
        std::os::unix::fs::symlink(&looping, &looping).unwrap();
        db.wal_path = looping;
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
        let mut db = Db::open(dir.path(), &[7u8; 32]).unwrap();
        let wal = dir.path().join("a-wal-that-keeps-bytes");
        std::fs::write(&wal, [1u8; 100]).unwrap();
        db.wal_path = wal;
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
        let again = EncryptedStore::create(
            dir.path(),
            Box::new(DevFallbackCustody::new(RootId::fresh())),
            DEFAULT_RETENTION,
        );
        assert!(matches!(again, Err(StoreError::AlreadyExists(_))));
    }

    #[test]
    fn open_reports_a_missing_root_as_root_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (store, root_id) = persistent_store(dir.path());
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
        let (mut store, _) = persistent_store(dir.path());
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
        segment::read_segment_file(store.dir(), &seg_key, seg_ref.segment_id, &mut plain).unwrap();
        assert_eq!(plain, b"screen frame bytes");
        let mut again = Vec::new();
        assert_eq!(
            store.read_segment(seg_ref.segment_id, &mut again).unwrap(),
            18
        );
        assert_eq!(again, b"screen frame bytes");
    }
}
