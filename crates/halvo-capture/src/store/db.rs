//! SQLCipher metadata store and the sole-writer [`EncryptedStore`].

use std::fs::{File, TryLockError};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

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
        let json = serde_json::to_vec_pretty(self).map_err(|e| StoreError::Io(e.to_string()))?;
        std::fs::write(Self::path(store_dir), json).map_err(io_error)
    }

    fn read(store_dir: &Path) -> StoreResult<Self> {
        let json = match std::fs::read(Self::path(store_dir)) {
            Ok(json) => json,
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(StoreError::Sealed),
            Err(e) => return Err(io_error(e)),
        };
        serde_json::from_slice(&json).map_err(|e| StoreError::Io(e.to_string()))
    }

    fn wrapped_store_key(&self) -> StoreResult<WrappedKey> {
        BASE64
            .decode(self.wrapped_store_key_b64.as_bytes())
            .map(WrappedKey)
            .map_err(|e| StoreError::Io(e.to_string()))
    }
}

fn io_error(e: std::io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

fn db_error(e: rusqlite::Error) -> StoreError {
    StoreError::Io(e.to_string())
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

struct WriterLock {
    _file: File,
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
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(StoreError::Locked(store_dir.to_path_buf())),
            Err(TryLockError::Error(e)) => Err(io_error(e)),
        }
    }
}

pub(crate) struct Db {
    conn: Connection,
}

impl Db {
    pub(crate) fn open(path: &Path, key: &[u8; 32]) -> StoreResult<Self> {
        let conn = Connection::open(path).map_err(db_error)?;
        let hex = hex_lower(key);
        conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
            .map_err(|e| StoreError::Io(format!("set key: {e}")))?;
        conn.execute_batch(
            "PRAGMA secure_delete = ON;\n\
             PRAGMA temp_store = MEMORY;\n\
             PRAGMA journal_mode = WAL;\n\
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|e| StoreError::Io(format!("pragmas: {e}")))?;
        verify_key(&conn)?;
        require_secure_delete(&conn)?;
        let db = Self { conn };
        db.init_schema()?;
        db.enable_fts_secure_delete()?;
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
            .map_err(|e| StoreError::Io(format!("schema: {e}")))
    }

    fn enable_fts_secure_delete(&self) -> StoreResult<()> {
        self.conn
            .execute_batch("INSERT INTO events_fts(events_fts, rank) VALUES('secure-delete', 1);")
            .map_err(|e| StoreError::Io(format!("fts secure-delete: {e}")))
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    fn insert_segment_key(
        &self,
        segment_id: uuid::Uuid,
        wrapped: &[u8],
        created_at: u64,
        expires_at: u64,
    ) -> StoreResult<SegmentKeyId> {
        self.conn
            .execute(
                "INSERT INTO segment_keys (segment_id, wrapped_key, created_at, expires_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    segment_id.simple().to_string(),
                    wrapped,
                    created_at as i64,
                    expires_at as i64
                ],
            )
            .map_err(db_error)?;
        Ok(SegmentKeyId(self.conn.last_insert_rowid()))
    }

    pub(crate) fn insert_event(&self, event: &CaptureStreamEvent) -> StoreResult<()> {
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

    pub(crate) fn checkpoint_truncate(&self) -> StoreResult<()> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .map_err(db_error)
    }
}

fn verify_key(conn: &Connection) -> StoreResult<()> {
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|_| ())
    .map_err(|_| StoreError::Sealed)
}

fn require_secure_delete(conn: &Connection) -> StoreResult<()> {
    match pragma_int(conn, "secure_delete")? {
        1 => Ok(()),
        other => Err(StoreError::Io(format!(
            "secure_delete is {other}, so key destruction would not scrub the freed blob"
        ))),
    }
}

fn pragma_int(conn: &Connection, name: &str) -> StoreResult<i64> {
    conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, i64>(0))
        .map_err(db_error)
}

fn hex_lower(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

enum Keyed {
    Open { db: Db, store_key: StoreKey },
    Sealed,
}

/// The sole-writer encrypted store: the SQLCipher database, the per-segment files
/// under `dir`, and the one store key in the clear only in memory.
pub struct EncryptedStore {
    dir: PathBuf,
    retention: Duration,
    custody: Box<dyn KeyCustody + Send>,
    keyed: Keyed,
    _writer_lock: WriterLock,
}

impl EncryptedStore {
    /// Create a fresh store in `dir`: generate a random store key, wrap it with
    /// `custody`, write the store-key file, and open the database.
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
        let store_key_bytes = crate::custody::dev_fallback::random_key()?;
        let store_key = StoreKey::from_bytes(store_key_bytes);
        let wrapped = custody.wrap_store_key(&store_key)?;
        KeyFile {
            version: 1,
            custody_kind: custody.kind(),
            root_id: custody.root_id(),
            wrapped_store_key_b64: BASE64.encode(&wrapped.0),
        }
        .write(dir)?;
        let db = Db::open(&dir.join("store.db"), store_key.expose())?;
        Ok(Self {
            dir: dir.to_path_buf(),
            retention,
            custody,
            keyed: Keyed::Open { db, store_key },
            _writer_lock,
        })
    }

    /// Open an existing store: read the store-key file, **unwrap the store key
    /// through the custody it names** (the owner's no-prompt unwrap) and open the
    /// database. Fails [`StoreError::Sealed`] if the key file is gone (delete-all
    /// ran) or the custody cannot unwrap (a non-owner).
    pub fn open(dir: &Path, retention: Duration) -> StoreResult<Self> {
        let _writer_lock = WriterLock::take(dir)?;
        let key_file = KeyFile::read(dir)?;
        let custody = custody_for(key_file.custody_kind, key_file.root_id)?;
        let store_key = match custody.unwrap_store_key(&key_file.wrapped_store_key()?) {
            Ok(key) => key,
            Err(CustodyError::Crypto(_)) => return Err(StoreError::Sealed),
            Err(e) => return Err(StoreError::Custody(e)),
        };
        let db = Db::open(&dir.join("store.db"), store_key.expose())?;
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

    /// Read and decrypt a segment's plaintext: unwrap its per-segment key with the
    /// store key, then stream-decrypt the file. Fails [`StoreError::Sealed`] if the
    /// segment has been crypto-shredded (its key row is NULL or gone).
    pub fn read_segment(&self, segment_id: uuid::Uuid, out: &mut dyn Write) -> StoreResult<u64> {
        let (db, store_key) = self.keyed()?;
        let wrapped: Option<Vec<u8>> = db
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
                [segment_id.simple().to_string()],
                |r| r.get(0),
            )
            .map_err(|_| StoreError::Sealed)?;
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
        let segment_key_id = db.insert_segment_key(segment_id, &wrapped, created_at, expires_at)?;
        let header = SegmentHeader {
            segment_id,
            segment_key_id,
        };
        let byte_len = segment::write_segment_file(&self.dir, &seg_key, &header, bytes)?;
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
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
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
}

#[cfg(test)]
mod tests {
    use super::test_support::make_store;
    use super::*;
    use crate::custody::dev_fallback::DevFallbackCustody;
    use crate::event::{CapturedAt, Modality, MonotonicSequence};
    use crate::store::{SegmentStore, DEFAULT_RETENTION};

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
        assert!(EncryptedStore::open(dir.path(), DEFAULT_RETENTION).is_ok());
        DevFallbackCustody::new(root_id).destroy_root().unwrap();
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
