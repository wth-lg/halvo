//! SQLCipher metadata store and the sole-writer [`EncryptedStore`].

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::custody::{CustodyError, CustodyKind, KeyCustody, StoreKey};
use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

use super::segment::{self, SegmentHeader, SegmentKey};
use super::{SegmentStore, StoreError, StoreResult};

const DEFAULT_RETENTION_SECS: u64 = 14 * 24 * 60 * 60;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct KeyFile {
    pub version: u32,
    pub custody_kind: CustodyKind,
    pub root_id: String,
    pub wrapped_store_key_b64: String,
}

impl KeyFile {
    pub(crate) fn path(store_dir: &Path) -> PathBuf {
        store_dir.join("store.key")
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_nanos(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

fn retention_secs() -> u64 {
    std::env::var("HALVO_RETENTION_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_RETENTION_SECS)
}

pub(crate) struct Db {
    conn: Connection,
}

impl Db {
    pub(crate) fn open(path: &Path, key: &[u8; 32]) -> StoreResult<Self> {
        let conn = Connection::open(path).map_err(|e| StoreError::Io(e.to_string()))?;
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
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|_| StoreError::Sealed)?;
        let db = Self { conn };
        db.init_schema()?;
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
            .map_err(|e| StoreError::Io(format!("schema: {e}")))?;
        self.conn
            .execute_batch("INSERT INTO events_fts(events_fts, rank) VALUES('secure-delete', 1);")
            .map_err(|e| StoreError::Io(format!("fts secure-delete: {e}")))?;
        Ok(())
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
            .map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(SegmentKeyId(self.conn.last_insert_rowid()))
    }

    fn insert_event(&self, event: &CaptureStreamEvent) -> StoreResult<()> {
        self.conn
            .execute(
                "INSERT INTO events (event_id, wall_clock_nanos, sequence, modality, \
                   foreground_app, segment_id, byte_offset, byte_len, segment_key_id, \
                   is_derived, retention_class) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![
                    event.event_id.simple().to_string(),
                    now_nanos(event.captured_at.wall_clock),
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
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let rowid = self.conn.last_insert_rowid();
        self.conn
            .execute(
                "INSERT INTO events_fts (rowid, foreground_app) VALUES (?1, ?2)",
                rusqlite::params![rowid, event.foreground_app],
            )
            .map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(())
    }

    pub(crate) fn pragma_int(&self, name: &str) -> StoreResult<i64> {
        self.conn
            .query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, i64>(0))
            .map_err(|e| StoreError::Io(e.to_string()))
    }

    pub(crate) fn fts_secure_delete(&self) -> StoreResult<i64> {
        self.conn
            .query_row(
                "SELECT CAST(v AS INTEGER) FROM events_fts_config WHERE k='secure-delete'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| StoreError::Io(e.to_string()))
    }
}

fn hex_lower(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// The sole-writer encrypted store: the SQLCipher database, the per-segment files
/// under `dir`, and the one store key in the clear only in memory.
pub struct EncryptedStore {
    db: Db,
    store_key: StoreKey,
    dir: PathBuf,
    destroy_root: Box<dyn Fn() -> Result<(), CustodyError> + Send>,
}

impl EncryptedStore {
    /// Create a fresh store in `dir`: generate a random store key, wrap it with
    /// `custody`, write the store-key file, and open the database.
    pub fn create<C: KeyCustody>(
        dir: &Path,
        custody: &C,
        root_id: &str,
        destroy_root: Box<dyn Fn() -> Result<(), CustodyError> + Send>,
    ) -> StoreResult<Self> {
        std::fs::create_dir_all(dir).map_err(|e| StoreError::Io(e.to_string()))?;
        let store_key_bytes = crate::custody::dev_fallback::random_key()
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let store_key = StoreKey::from_bytes(store_key_bytes);
        let wrapped = custody
            .wrap_store_key(&store_key)
            .map_err(|e| StoreError::Io(e.to_string()))?;

        let key_file = KeyFile {
            version: 1,
            custody_kind: custody.kind(),
            root_id: root_id.to_string(),
            wrapped_store_key_b64: BASE64.encode(&wrapped.0),
        };
        let json =
            serde_json::to_vec_pretty(&key_file).map_err(|e| StoreError::Io(e.to_string()))?;
        std::fs::write(KeyFile::path(dir), json).map_err(|e| StoreError::Io(e.to_string()))?;

        let db = Db::open(&dir.join("store.db"), &store_key_bytes)?;
        Ok(Self {
            db,
            store_key,
            dir: dir.to_path_buf(),
            destroy_root,
        })
    }

    /// Open an existing store: read the store-key file, **unwrap the store key
    /// through `custody`** (the owner's no-prompt unwrap) and open the database.
    /// Fails [`StoreError::Sealed`] if the key file is gone (delete-all ran) or the
    /// custody cannot unwrap (a non-owner).
    pub fn open<C: KeyCustody>(
        dir: &Path,
        custody: &C,
        destroy_root: Box<dyn Fn() -> Result<(), CustodyError> + Send>,
    ) -> StoreResult<Self> {
        let json = std::fs::read(KeyFile::path(dir)).map_err(|_| StoreError::Sealed)?;
        let kf: KeyFile =
            serde_json::from_slice(&json).map_err(|e| StoreError::Io(e.to_string()))?;
        let wrapped_bytes = BASE64
            .decode(kf.wrapped_store_key_b64.as_bytes())
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let store_key = custody
            .unwrap_store_key(&crate::custody::WrappedKey(wrapped_bytes))
            .map_err(|_| StoreError::Sealed)?;
        let db = Db::open(&dir.join("store.db"), store_key.expose())?;
        Ok(Self {
            db,
            store_key,
            dir: dir.to_path_buf(),
            destroy_root,
        })
    }

    pub(crate) fn db(&self) -> &Db {
        &self.db
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    #[cfg(test)]
    pub(crate) fn store_key_bytes(&self) -> &[u8; 32] {
        self.store_key.expose()
    }

    /// Read and decrypt a segment's plaintext: unwrap its per-segment key with the
    /// store key, then stream-decrypt the file. Fails [`StoreError::Sealed`] if the
    /// segment has been crypto-shredded (its key row is NULL or gone).
    pub fn read_segment(&self, segment_id: uuid::Uuid, out: &mut dyn Write) -> StoreResult<u64> {
        let wrapped: Option<Vec<u8>> = self
            .db
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
                [segment_id.simple().to_string()],
                |r| r.get(0),
            )
            .map_err(|_| StoreError::Sealed)?;
        let wrapped = wrapped.ok_or(StoreError::Sealed)?;
        let seg_key = segment::unwrap_segment_key(self.store_key.expose(), &wrapped)?;
        segment::read_segment_file(&self.dir, &seg_key, segment_id, out)
    }

    pub(crate) fn destroy_root(&self) -> Result<(), CustodyError> {
        (self.destroy_root)()
    }

    pub(crate) fn write_segment_with_expiry(
        &mut self,
        bytes: &mut dyn Read,
        created_at: u64,
        expires_at: u64,
    ) -> StoreResult<(SegmentRef, SegmentKeyId)> {
        let segment_id = uuid::Uuid::now_v7();
        let seg_key = SegmentKey::random()?;
        let wrapped = segment::wrap_segment_key(self.store_key.expose(), &seg_key)?;
        let segment_key_id = self
            .db
            .insert_segment_key(segment_id, &wrapped, created_at, expires_at)?;
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
        self.db.insert_event(event)
    }

    fn write_segment(&mut self, bytes: &mut dyn Read) -> StoreResult<(SegmentRef, SegmentKeyId)> {
        let created = now_secs();
        let expires = created + retention_secs();
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
mod tests {
    use super::*;
    use crate::custody::dev_fallback::DevFallbackCustody;
    use crate::event::{CapturedAt, Modality, MonotonicSequence};
    use crate::store::SegmentStore;

    fn make_store(dir: &Path) -> EncryptedStore {
        let root_id = format!("halvo-test-{}", uuid::Uuid::now_v7());
        let custody = DevFallbackCustody::new(root_id.clone());
        #[cfg(target_os = "macos")]
        let destroyer = {
            let c = DevFallbackCustody::new(root_id.clone());
            Box::new(move || c.destroy_root()) as Box<dyn Fn() -> Result<(), CustodyError> + Send>
        };
        #[cfg(not(target_os = "macos"))]
        let destroyer = Box::new(|| Ok(())) as Box<dyn Fn() -> Result<(), CustodyError> + Send>;
        EncryptedStore::create(dir, &custody, &root_id, destroyer).unwrap()
    }

    #[test]
    fn pragmas_are_set() {
        let dir = tempfile::tempdir().unwrap();
        let store = make_store(dir.path());
        assert_eq!(store.db().pragma_int("secure_delete").unwrap(), 1);
        assert_eq!(store.db().pragma_int("temp_store").unwrap(), 2);
        assert_eq!(store.db().fts_secure_delete().unwrap(), 1);
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
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE id=?1",
                [key_id.0],
                |r| r.get(0),
            )
            .unwrap();
        let seg_key = segment::unwrap_segment_key(store.store_key_bytes(), &wrapped).unwrap();
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
