use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs::{File, TryLockError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Once};
use std::time::Duration;

use halvo_schema_core::ids::Position;
use halvo_schema_core::kinds::{self, KindEntry, RetentionClass};
use halvo_schema_core::record::Instant;
use halvo_sqlcipher::rusqlite::{self, Connection, ErrorCode, Params, Row};

use crate::error::{db_fault, io_fault, Fault, OpenError};
use crate::host::{Clock, DbKey, IdSource, ReportSink};
use crate::report::{Counts, Report};
use crate::sql::{self, Sql};

pub(crate) const PAGE_CACHE_KIB: i64 = 4096;
pub(crate) const CALL_BUDGET: Duration = Duration::from_secs(30);
const PROGRESS_OPS: i32 = 10_000;

struct WriterLock {
    file: File,
}

impl WriterLock {
    fn take(dir: &Path) -> Result<WriterLock, OpenError> {
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("writer.lock"))
            .map_err(|e| OpenError::Fault(io_fault(e)))?;
        match file.try_lock() {
            Ok(()) => Ok(WriterLock { file }),
            Err(TryLockError::WouldBlock) => Err(OpenError::Locked),
            Err(TryLockError::Error(e)) => Err(OpenError::Fault(io_fault(e))),
        }
    }
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub(crate) struct Paths {
    pub(crate) db: PathBuf,
    pub(crate) wal: PathBuf,
    pub(crate) shm: PathBuf,
}

impl Paths {
    fn of(dir: &Path) -> Paths {
        let db = dir.join("derived.db");
        Paths {
            wal: dir.join("derived.db-wal"),
            shm: dir.join("derived.db-shm"),
            db,
        }
    }
}

pub(crate) struct Progress {
    base: std::time::Instant,
    deadline: Arc<AtomicU64>,
}

impl Progress {
    pub(crate) fn install(conn: &mut Connection, budget: Duration) -> Result<Progress, Fault> {
        let base = std::time::Instant::now();
        let deadline = Arc::new(AtomicU64::new(0));
        let progress = Progress { base, deadline };
        progress.arm(budget);
        let watched = Arc::clone(&progress.deadline);
        conn.progress_handler(
            PROGRESS_OPS,
            Some(move || elapsed_nanos(base) > watched.load(Ordering::Relaxed)),
        )
        .map_err(db_fault)?;
        Ok(progress)
    }

    pub(crate) fn arm(&self, budget: Duration) {
        let end = elapsed_nanos(self.base).saturating_add(budget.as_nanos() as u64);
        self.deadline.store(end, Ordering::Relaxed);
    }
}

fn elapsed_nanos(base: std::time::Instant) -> u64 {
    u64::try_from(base.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// A transaction of the store. It records each family that lost, gained or changed a row, and its commit raises
/// the revision of each of them by one.
pub(crate) struct Tx<'c> {
    pub(crate) conn: &'c Connection,
    changed: RefCell<BTreeSet<String>>,
}

impl Tx<'_> {
    pub(crate) fn exec<P: Params>(&self, sql: Sql, params: P) -> Result<usize, Fault> {
        exec(self.conn, sql, params)
    }

    pub(crate) fn changed(&self, family: &str) {
        self.changed.borrow_mut().insert(family.to_owned());
    }

    /// Removes the rows that `select` names by their id and family, and returns how many went.
    pub(crate) fn remove<P: Params>(&self, select: Sql, params: P) -> Result<u64, Fault> {
        let doomed: Vec<(i64, String)> =
            rows(self.conn, select, params, |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut removed = 0;
        for (id, family) in &doomed {
            if self.exec(sql::ROW_DELETE, [id])? > 0 {
                removed += 1;
                self.changed(family);
            }
        }
        Ok(removed)
    }

    /// The high-water mark of purges: every ordinal below it is purged.
    pub(crate) fn mark(&self) -> Result<u64, Fault> {
        let mark = one(self.conn, sql::MARK_READ, [], |r| r.get::<_, i64>(0))?.ok_or(Fault::Row)?;
        u64::try_from(mark).map_err(|_| Fault::Row)
    }

    /// Whether one of `segments` is a purged ordinal: below the mark or in the set of purged ordinals above it.
    pub(crate) fn names_purged(&self, mark: u64, segments: &[u64]) -> Result<bool, Fault> {
        for segment in segments {
            if *segment < mark {
                return Ok(true);
            }
            let segment = i64::try_from(*segment).map_err(|_| Fault::Row)?;
            if one(self.conn, sql::PURGED_HAS, [segment], |r| {
                r.get::<_, i64>(0)
            })?
            .is_some()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Raises the mark to `floor` where it lies below, drops the purged ordinals below the mark, and raises the mark
    /// over each purged ordinal that follows it.
    pub(crate) fn raise_mark(&self, floor: u64) -> Result<(), Fault> {
        let mut mark = self.mark()?.max(floor);
        let as_i64 = |n: u64| i64::try_from(n).map_err(|_| Fault::Row);
        self.exec(sql::PURGED_BELOW_DELETE, [as_i64(mark)?])?;
        while one(self.conn, sql::PURGED_HAS, [as_i64(mark)?], |r| {
            r.get::<_, i64>(0)
        })?
        .is_some()
        {
            self.exec(sql::PURGED_BELOW_DELETE, [as_i64(mark + 1)?])?;
            mark += 1;
        }
        self.exec(sql::MARK_SET, [as_i64(mark)?])?;
        Ok(())
    }

    /// Inserts one row with its source ordinals and marks its family as changed.
    pub(crate) fn insert(&self, row: &NewRow<'_>) -> Result<i64, Fault> {
        self.exec(
            sql::ROW_INSERT,
            rusqlite::params![
                row.entry.name,
                row.entry.family,
                class_code(row.entry.class),
                row.day,
                gone(row.entry.class, row.day),
                row.expires,
                row.workflow,
                row.summary,
                row.wkey,
                row.body
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        for ordinal in row.segments {
            let ordinal = i64::try_from(*ordinal).map_err(|_| Fault::Row)?;
            self.exec(sql::SOURCE_INSERT, [ordinal, id])?;
        }
        self.changed(row.entry.family);
        Ok(id)
    }

    /// The expiry by class, from the store's own rows: every bounded row whose bound has passed on the local day of
    /// the clock or whose day lies after it, every window row whose expiry instant has passed, and then every row
    /// that names a summary or a workflow that is not stored. It returns the number of rows removed.
    pub(crate) fn expire(&self, now: Now) -> Result<u64, Fault> {
        Ok(self.remove(sql::EXPIRE_GONE, [now.today])?
            + self.remove(sql::EXPIRE_AHEAD, [now.today])?
            + self.remove(sql::EXPIRE_WINDOW, [now.now])?
            + self.sweep()?)
    }

    /// Removes every evidence or support row whose summary is not stored and every row of another kind than
    /// `workflow` that names a workflow that is not stored.
    pub(crate) fn sweep(&self) -> Result<u64, Fault> {
        Ok(self.remove(sql::ORPHAN_EVIDENCE, [])? + self.remove(sql::ORPHAN_WORKFLOW, [])?)
    }
}

/// A row to insert: its kind's entry, the columns that the store's rules read, the serialized record and, for a
/// window row, its source ordinals.
pub(crate) struct NewRow<'a> {
    pub(crate) entry: &'a KindEntry,
    pub(crate) day: Option<u32>,
    pub(crate) expires: Option<i64>,
    pub(crate) workflow: Option<&'a str>,
    pub(crate) summary: Option<&'a str>,
    pub(crate) wkey: Option<&'a str>,
    pub(crate) body: &'a str,
    pub(crate) segments: &'a [u64],
}

/// The first local day on which a bounded row of `day` is past its bound: its day plus the days of its entry.
fn gone(class: RetentionClass, day: Option<u32>) -> Option<i64> {
    match (class, day) {
        (RetentionClass::Bounded { days }, Some(day)) => Some(i64::from(day) + i64::from(days)),
        _ => None,
    }
}

pub(crate) fn class_code(class: RetentionClass) -> i64 {
    match class {
        RetentionClass::Window => 0,
        RetentionClass::Bounded { .. } => 1,
        RetentionClass::Kept => 2,
    }
}

/// The instant and the local day of the clock when a call starts.
#[derive(Clone, Copy)]
pub(crate) struct Now {
    pub(crate) now: i64,
    pub(crate) today: i64,
}

impl Now {
    /// A bounded row of `day` stays while today minus its day is from 0 to `days` minus 1.
    pub(crate) fn keeps_day(self, days: u16, day: u32) -> bool {
        (0..i64::from(days)).contains(&(self.today - i64::from(day)))
    }

    /// A window row stays while its expiry instant lies after now.
    pub(crate) fn keeps_expiry(self, expires: i64) -> bool {
        expires > self.now
    }
}

/// The store id that the database is bound to, from its first commit, and the committed position.
pub(crate) struct Meta {
    pub(crate) store: Option<String>,
    pub(crate) position: Option<Position>,
}

pub(crate) fn exec<P: Params>(conn: &Connection, sql: Sql, params: P) -> Result<usize, Fault> {
    let mut statement = conn.prepare_cached(sql.text()).map_err(db_fault)?;
    statement.execute(params).map_err(db_fault)
}

pub(crate) fn rows<T, P: Params>(
    conn: &Connection,
    sql: Sql,
    params: P,
    mut map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, Fault> {
    let mut statement = conn.prepare_cached(sql.text()).map_err(db_fault)?;
    let mut found = statement.query(params).map_err(db_fault)?;
    let mut out = Vec::new();
    while let Some(row) = found.next().map_err(db_fault)? {
        out.push(map(row).map_err(db_fault)?);
    }
    Ok(out)
}

pub(crate) fn one<T, P: Params>(
    conn: &Connection,
    sql: Sql,
    params: P,
    map: impl FnOnce(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Option<T>, Fault> {
    let mut statement = conn.prepare_cached(sql.text()).map_err(db_fault)?;
    let mut found = statement.query(params).map_err(db_fault)?;
    match found.next().map_err(db_fault)? {
        Some(row) => Ok(Some(map(row).map_err(db_fault)?)),
        None => Ok(None),
    }
}

pub(crate) struct Db {
    pub(crate) conn: Connection,
    pub(crate) entries: Vec<&'static KindEntry>,
    pub(crate) families: Vec<&'static str>,
    pub(crate) clock: Box<dyn Clock>,
    pub(crate) paths: Paths,
    pub(crate) site_key: [u8; 32],
    sink: RefCell<Option<ReportSink>>,
    progress: Progress,
    _lock: WriterLock,
}

impl Drop for Db {
    fn drop(&mut self) {
        self.site_key.fill(0);
        std::hint::black_box(&self.site_key);
    }
}

impl Db {
    pub(crate) fn open(
        data_dir: &Path,
        key: &DbKey,
        entries: &[&'static KindEntry],
        clock: Box<dyn Clock>,
        sink: Option<ReportSink>,
        ids: &mut dyn IdSource,
    ) -> Result<Db, OpenError> {
        check_entries(entries)?;
        initialize_once();
        let dir = data_dir.join("derived");
        std::fs::create_dir_all(&dir).map_err(|e| OpenError::Fault(io_fault(e)))?;
        let lock = WriterLock::take(&dir)?;
        let paths = Paths::of(&dir);
        let mut conn = Connection::open(&paths.db).map_err(|e| OpenError::Fault(db_fault(e)))?;
        conn.busy_timeout(Duration::ZERO)
            .map_err(|e| OpenError::Fault(db_fault(e)))?;
        set_key(&conn, key)?;
        pragma(&conn, "PRAGMA locking_mode = EXCLUSIVE;")?;
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|e| match e.sqlite_error_code() {
            Some(ErrorCode::NotADatabase) => OpenError::Sealed,
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => OpenError::Locked,
            _ => OpenError::Fault(db_fault(e)),
        })?;
        pragma(
            &conn,
            &format!(
                "PRAGMA secure_delete = ON;\n\
                 PRAGMA temp_store = MEMORY;\n\
                 PRAGMA journal_mode = WAL;\n\
                 PRAGMA foreign_keys = ON;\n\
                 PRAGMA cache_size = -{PAGE_CACHE_KIB};"
            ),
        )?;
        require(&conn, "secure_delete", "1")?;
        require(&conn, "locking_mode", "exclusive")?;
        require(&conn, "journal_mode", "wal")?;
        require(&conn, "foreign_keys", "1")?;
        require(&conn, "cache_size", &format!("-{PAGE_CACHE_KIB}"))?;
        let progress = Progress::install(&mut conn, CALL_BUDGET)?;
        let families = families_of(entries);
        let mut db = Db {
            conn,
            entries: entries.to_vec(),
            families,
            clock,
            paths,
            site_key: [0; 32],
            sink: RefCell::new(sink),
            progress,
            _lock: lock,
        };
        db.schema(ids)?;
        let site_key: Vec<u8> =
            one(&db.conn, sql::META_SITE_KEY, [], |r| r.get(0))?.unwrap_or_default();
        db.site_key = site_key.try_into().map_err(|_| OpenError::Schema)?;
        db.reconcile()?;
        Ok(db)
    }

    /// The instant and the local day of the clock.
    pub(crate) fn now(&self) -> Now {
        let now = self.clock.now();
        Now {
            now: now.0,
            today: i64::from(self.clock.day(now)),
        }
    }

    pub(crate) fn meta(&self) -> Result<Meta, Fault> {
        let found = one(&self.conn, sql::META_READ, [], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })?;
        let (store, seg, idx) = found.ok_or(Fault::Row)?;
        let position = match (seg, idx) {
            (Some(seg), Some(idx)) => Some(Position {
                seg: u64::try_from(seg).map_err(|_| Fault::Row)?,
                idx: u64::try_from(idx).map_err(|_| Fault::Row)?,
            }),
            _ => None,
        };
        Ok(Meta { store, position })
    }

    /// Starts a call of the library: the progress handler interrupts a statement of this call once its budget is
    /// spent.
    pub(crate) fn arm(&self) {
        self.progress.arm(CALL_BUDGET);
    }

    fn schema(&self, ids: &mut dyn IdSource) -> Result<(), OpenError> {
        let objects = master(&self.conn).map_err(OpenError::Fault)?;
        if objects.is_empty() {
            let mut site_key = [0u8; 32];
            ids.fill(&mut site_key);
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| OpenError::Fault(db_fault(e)))?;
            tx.execute_batch(sql::SCHEMA)
                .map_err(|e| OpenError::Fault(db_fault(e)))?;
            exec(
                &tx,
                sql::META_INSERT,
                rusqlite::params![sql::SCHEMA_VERSION, &site_key[..]],
            )?;
            site_key.fill(0);
            tx.commit().map_err(|e| OpenError::Fault(db_fault(e)))?;
            return Ok(());
        }
        if objects != reference_schema().map_err(OpenError::Fault)? {
            return Err(OpenError::Schema);
        }
        match one(&self.conn, sql::META_SCHEMA, [], |r| r.get::<_, i64>(0))? {
            Some(sql::SCHEMA_VERSION) => Ok(()),
            _ => Err(OpenError::Schema),
        }
    }

    fn reconcile(&self) -> Result<(), Fault> {
        self.checkpoint()?;
        let now = self.now();
        self.write_checkpointed(|tx| {
            for family in &self.families {
                tx.exec(sql::REVISION_ENSURE, [family])?;
            }
            tx.sweep()?;
            tx.expire(now)
        })?;
        Ok(())
    }

    /// Runs `f` in one transaction and commits it when `f` returns `Ok`, raising the revision of each family that
    /// `f` marked as changed; on `Err` nothing of `f` stays.
    pub(crate) fn write<T, E: From<Fault>>(
        &self,
        f: impl FnOnce(&Tx<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let out = self.commit(f)?;
        self.give_report()?;
        Ok(out)
    }

    /// `write`, with the truncating checkpoint after the commit and before the report.
    pub(crate) fn write_checkpointed<T, E: From<Fault>>(
        &self,
        f: impl FnOnce(&Tx<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let out = self.commit(f)?;
        self.checkpoint()?;
        self.give_report()?;
        Ok(out)
    }

    fn give_report(&self) -> Result<(), Fault> {
        if self.sink.borrow().is_none() {
            return Ok(());
        }
        let report = self.report()?;
        if let Some(sink) = self.sink.borrow_mut().as_mut() {
            sink(&report);
        }
        Ok(())
    }

    /// The four values of the owner's report: the revision of every family of the entries in their order, the count
    /// of rows per retention class, the bytes on disk of the database with its log and its shared-memory file, and
    /// the next expiry deadline.
    pub(crate) fn report(&self) -> Result<Report, Fault> {
        let mut revisions = Vec::new();
        for family in &self.families {
            let n = one(&self.conn, sql::REVISION_READ, [family], |r| {
                r.get::<_, i64>(0)
            })?
            .unwrap_or(0);
            revisions.push((*family, u64::try_from(n).map_err(|_| Fault::Row)?));
        }
        let count = |class: RetentionClass| -> Result<u64, Fault> {
            let n = one(&self.conn, sql::COUNT_CLASS, [class_code(class)], |r| {
                r.get::<_, i64>(0)
            })?
            .unwrap_or(0);
            u64::try_from(n).map_err(|_| Fault::Row)
        };
        let counts = Counts {
            window: count(RetentionClass::Window)?,
            bounded: count(RetentionClass::Bounded { days: 0 })?,
            kept: count(RetentionClass::Kept)?,
        };
        let disk_bytes =
            file_len(&self.paths.db)? + file_len(&self.paths.wal)? + file_len(&self.paths.shm)?;
        let min_expires = one(&self.conn, sql::MIN_EXPIRES, [], |r| {
            r.get::<_, Option<i64>>(0)
        })?
        .flatten();
        let min_gone = one(&self.conn, sql::MIN_GONE, [], |r| {
            r.get::<_, Option<i64>>(0)
        })?
        .flatten();
        let gone_at =
            min_gone.map(|day| self.clock.day_start(u32::try_from(day).unwrap_or(u32::MAX)));
        let expiry = [min_expires.map(Instant), gone_at]
            .into_iter()
            .flatten()
            .min();
        Ok(Report {
            revisions,
            counts,
            disk_bytes,
            expiry,
        })
    }

    fn commit<T, E: From<Fault>>(&self, f: impl FnOnce(&Tx<'_>) -> Result<T, E>) -> Result<T, E> {
        let tx = self.conn.unchecked_transaction().map_err(db_fault)?;
        let writer = Tx {
            conn: &tx,
            changed: RefCell::new(BTreeSet::new()),
        };
        let out = f(&writer)?;
        for family in writer.changed.borrow().iter() {
            exec(&tx, sql::REVISION_ENSURE, [family])?;
            exec(&tx, sql::REVISION_BUMP, [family])?;
        }
        tx.commit().map_err(db_fault)?;
        Ok(out)
    }

    /// The truncating checkpoint: every frame of the write-ahead log goes into the database and the log is cut to
    /// no byte.
    pub(crate) fn checkpoint(&self) -> Result<(), Fault> {
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .map_err(db_fault)?;
        if busy != 0 || file_len(&self.paths.wal)? != 0 {
            return Err(Fault::Checkpoint);
        }
        Ok(())
    }
}

/// Runs SQLite's start-up, SQLCipher's included, to its end on one thread before any store of the process opens its
/// database. SQLite marks itself started and lets other threads in before it runs SQLCipher's start-up hook, and a
/// thread that sets a key in that gap is refused (crates/halvo-sqlcipher builds the hook).
fn initialize_once() {
    static START: Once = Once::new();
    START.call_once(|| {
        let _ = Connection::open_in_memory();
    });
}

pub(crate) fn file_len(path: &Path) -> Result<u64, Fault> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(0),
        Err(e) => Err(io_fault(e)),
    }
}

fn check_entries(entries: &[&'static KindEntry]) -> Result<(), OpenError> {
    let reports = kinds::check_entries(entries);
    if !reports.is_empty() {
        return Err(OpenError::Entries(reports));
    }
    for kind in kinds::KINDS {
        if !entries.contains(&kind) {
            return Err(OpenError::MissingKind(kind.name));
        }
    }
    Ok(())
}

fn families_of(entries: &[&'static KindEntry]) -> Vec<&'static str> {
    let mut families: Vec<&'static str> = Vec::new();
    for entry in entries {
        if !families.contains(&entry.family) {
            families.push(entry.family);
        }
    }
    families
}

fn set_key(conn: &Connection, key: &DbKey) -> Result<(), OpenError> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = Vec::with_capacity(80);
    text.extend_from_slice(b"PRAGMA key = \"x'");
    for b in key.bytes() {
        text.push(DIGITS[usize::from(b >> 4)]);
        text.push(DIGITS[usize::from(b & 0x0f)]);
    }
    text.extend_from_slice(b"'\";");
    let result = match std::str::from_utf8(&text) {
        Ok(pragma) => conn
            .execute_batch(pragma)
            .map_err(|e| OpenError::Fault(db_fault(e))),
        Err(_) => Err(OpenError::Sealed),
    };
    text.fill(0);
    std::hint::black_box(&text);
    result
}

fn pragma(conn: &Connection, text: &str) -> Result<(), OpenError> {
    conn.execute_batch(text)
        .map_err(|e| OpenError::Fault(db_fault(e)))
}

pub(crate) fn require(conn: &Connection, name: &'static str, want: &str) -> Result<(), OpenError> {
    let value: String = conn
        .query_row(&format!("PRAGMA {name}"), [], |r| {
            let value: rusqlite::types::Value = r.get(0)?;
            Ok(match value {
                rusqlite::types::Value::Integer(n) => n.to_string(),
                rusqlite::types::Value::Text(s) => s,
                _ => String::new(),
            })
        })
        .map_err(|e| OpenError::Fault(db_fault(e)))?;
    if value == want {
        Ok(())
    } else {
        Err(OpenError::Setting(name))
    }
}

type Object = (String, String, String, Option<String>);

fn master(conn: &Connection) -> Result<Vec<Object>, Fault> {
    let mut objects: Vec<Object> = conn
        .prepare(sql::MASTER)
        .and_then(|mut statement| {
            statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect()
        })
        .map_err(db_fault)?;
    objects.sort();
    Ok(objects)
}

fn reference_schema() -> Result<Vec<Object>, Fault> {
    let conn = Connection::open_in_memory().map_err(db_fault)?;
    conn.execute_batch(sql::SCHEMA).map_err(db_fault)?;
    master(&conn)
}
