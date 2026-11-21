//! Window-expiry crypto-shred.

use std::collections::{BTreeSet, HashSet};
use std::time::{Duration, SystemTime};

use halvo_sqlcipher::rusqlite::{self, Connection, OptionalExtension};
use uuid::Uuid;

use super::db::{db_error, Db, EncryptedStore};
use super::segment;
use super::{
    DeleteReport, Ordinal, Pass, Reconciled, SegmentExpiry, ShredReport, StoreError, StoreResult,
};

const STORE_KEY_COUNT: u64 = 1;

const LEGACY_CHUNK: usize = 64;

#[cfg(test)]
pub(in crate::store) const SEAM_BEFORE_THE_RETRIED_CHECKPOINT: &str =
    "an expiry chunk, before its retried checkpoint";

const NANOS_PER_SECOND: i64 = 1_000_000_000;

const SUBTRACT_TOTALS: &str = "UPDATE daily_totals \
     SET events = daily_totals.events - t.events, bytes = daily_totals.bytes - t.bytes \
     FROM segment_totals AS t \
     WHERE t.segment = ?1 AND daily_totals.day = ?2 AND daily_totals.kind = t.kind";

/// What one sweep of the keyless segment files did: the files it removed,
/// and the entries at the two walked levels of segments/ that the walk does
/// not take as files of the store (a link or a directory with a store name
/// among them, nothing below the two levels, and no live segment that the
/// sweep keeps), which it leaves in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sweep {
    pub removed: u64,
    pub not_store_files: u64,
}

/// The second, in whole Unix seconds, at which a segment first captured at
/// `first_capture` (signed Unix nanoseconds) falls due under `retention`: its
/// first capture floored to whole seconds, plus the window.
pub fn expiry_instant(first_capture: i64, retention: Duration) -> i64 {
    first_capture
        .div_euclid(NANOS_PER_SECOND)
        .saturating_add(i64::try_from(retention.as_secs()).unwrap_or(i64::MAX))
}

fn due_before(now_secs: i64, retention: Duration) -> i64 {
    let bound =
        (i128::from(now_secs) - i128::from(retention.as_secs()) + 1) * i128::from(NANOS_PER_SECOND);
    bound.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn parse_segment_id(text: &str) -> StoreResult<Uuid> {
    Uuid::parse_str(text).map_err(|_| {
        StoreError::Corrupt("segment_keys holds a segment id that does not parse".to_string())
    })
}

fn shred_in_place(conn: &Connection, segment_id: &str) -> StoreResult<bool> {
    let live: Option<(i64, i64)> = conn
        .query_row(
            "SELECT id, day FROM segment_keys WHERE segment_id = ?1 AND wrapped_key IS NOT NULL",
            [segment_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    let Some((ordinal, day)) = live else {
        return Ok(false);
    };
    let cleared = conn
        .execute(
            "UPDATE segment_keys SET wrapped_key = NULL WHERE segment_id = ?1",
            [segment_id],
        )
        .map_err(db_error)?;
    if cleared != 1 {
        return Err(StoreError::Corrupt(
            "a segment id names more or less than one key row".to_string(),
        ));
    }
    conn.execute(SUBTRACT_TOTALS, rusqlite::params![ordinal, day])
        .map_err(db_error)?;
    Ok(true)
}

fn clear_keys(db: &Db, ids: &[Uuid]) -> StoreResult<()> {
    let tx = db.connection().unchecked_transaction().map_err(db_error)?;
    for segment_id in ids {
        shred_in_place(&tx, &segment_id.simple().to_string())?;
    }
    #[cfg(test)]
    super::db::code_seams::or_forced_at(super::db::code_seams::SHRED_COMMIT, Ok(()))
        .map_err(db_error)?;
    tx.commit().map_err(db_error)
}

impl EncryptedStore {
    fn live_segments(&self) -> StoreResult<Vec<(Uuid, i64)>> {
        let conn = self.db()?.connection();
        let mut keyed = conn
            .prepare(
                "SELECT segment_id, first_capture FROM segment_keys WHERE wrapped_key IS NOT NULL",
            )
            .map_err(db_error)?;
        let rows = keyed
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(db_error)?;
        rows.map(|row| {
            let (id, first) = row.map_err(db_error)?;
            Ok((parse_segment_id(&id)?, first))
        })
        .collect()
    }

    fn anchor_future_segments(&self, now: i64) -> StoreResult<()> {
        let db = self.db()?;
        let changes_before = db.connection().total_changes();
        db.connection()
            .execute(
                "UPDATE segment_keys SET first_capture = ?1 \
                 WHERE wrapped_key IS NOT NULL AND first_capture > ?1",
                [now],
            )
            .map_err(db_error)?;
        db.look_after_the_first_commit(changes_before)
    }

    fn due_chunk(
        &self,
        now_secs: i64,
        max_segments: usize,
    ) -> StoreResult<(Vec<(Uuid, Ordinal)>, bool)> {
        let conn = self.db()?.connection();
        let mut due = conn
            .prepare(
                "SELECT segment_id, id, first_capture FROM segment_keys \
                 WHERE wrapped_key IS NOT NULL AND first_capture < ?1 \
                 ORDER BY first_capture, id LIMIT ?2",
            )
            .map_err(db_error)?;
        let limit = i64::try_from(max_segments)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let rows = due
            .query_map(
                rusqlite::params![due_before(now_secs, self.retention()), limit],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .map_err(db_error)?;
        let mut chunk = Vec::new();
        for row in rows {
            let (id, ordinal, first) = row.map_err(db_error)?;
            if expiry_instant(first, self.retention()) <= now_secs {
                chunk.push((parse_segment_id(&id)?, Ordinal::of_row(ordinal)));
            }
        }
        let more_due = chunk.len() > max_segments;
        chunk.truncate(max_segments);
        Ok((chunk, more_due))
    }

    fn remove_shredded_files(&self, ids: &[Uuid]) -> (BTreeSet<Uuid>, Option<StoreError>) {
        let mut left = BTreeSet::new();
        let mut first_failure = None;
        for id in ids {
            if let Err(e) = segment::remove_shredded_file(self.handle(), self.fs(), *id) {
                left.insert(*id);
                first_failure.get_or_insert(e);
            }
        }
        (left, first_failure)
    }

    fn finish_owed(&mut self) -> StoreResult<Option<StoreError>> {
        self.maintained_db()?.finish_owed_checkpoint()?;
        let owed: Vec<Uuid> = self.owed_files.iter().copied().collect();
        let (left, first_failure) = self.remove_shredded_files(&owed);
        self.owed_files = left;
        Ok(first_failure)
    }

    fn shred_chunk(&mut self, chunk: &[(Uuid, Ordinal)]) -> StoreResult<Vec<Ordinal>> {
        if chunk.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = chunk.iter().map(|(id, _)| *id).collect();
        let (left, first_failure) = {
            let db = self.maintained_db()?;
            let changes_before = db.connection().total_changes();
            match clear_keys(db, &ids) {
                Err(StoreError::Db(code)) if code & 0xff == rusqlite::ffi::SQLITE_FULL => {
                    let _ = self.remove_shredded_files(&ids);
                    clear_keys(db, &ids)?;
                }
                cleared => cleared?,
            }
            db.look_after_the_first_commit(changes_before)?;
            let checkpointed = db
                .checkpoint_truncate()
                .map_err(|e| db.kept_after_a_shred(e, changes_before));
            let (left, removal_failure) = self.remove_shredded_files(&ids);
            if checkpointed.is_err() {
                #[cfg(test)]
                super::failpoint::seam(SEAM_BEFORE_THE_RETRIED_CHECKPOINT);
                let _ = db.finish_owed_checkpoint();
            }
            (left, checkpointed.err().or(removal_failure))
        };
        self.owed_files.extend(left);
        if let Some(first) = first_failure {
            return Err(first);
        }
        let mut shredded: Vec<Ordinal> = chunk.iter().map(|(_, ordinal)| *ordinal).collect();
        shredded.sort_unstable();
        Ok(shredded)
    }

    /// Whether a later expiry or eviction call on this handle finishes work
    /// that an earlier call left undone: the checkpoint after a shred's
    /// commit, or the removal of a shredded segment's file.
    pub fn owes_work(&self) -> bool {
        !self.owed_files.is_empty() || self.maintained_db().is_ok_and(|db| db.owes_a_checkpoint())
    }

    /// One expiry chunk at the caller's wall-clock `now`; the store reads no
    /// clock. It first finishes what an earlier call owes: the checkpoint
    /// after a shred's commit, which it must finish before anything else, and
    /// the removal of each shredded segment's file. It anchors every live
    /// segment whose first capture is later than `now` at `now` again, so
    /// that a record stamped in the future lives at most one window after
    /// the call that first saw it. Then it shreds at most
    /// `max_segments` live segments whose first capture, floored to whole
    /// seconds, plus this handle's window is at or before `now`, the oldest
    /// first capture first, in one transaction that clears each key in place
    /// and takes its totals out of the rollup. When that commit fails with
    /// SQLITE_FULL it removes the chunk's files and runs the transaction once
    /// more: the one case where a file goes before its key. After the commit
    /// it attempts the checkpoint and the removal of exactly that chunk's
    /// files, and the checkpoint once more after the removal when the first
    /// one failed, and answers the first error after every step ran; the
    /// handle keeps what the call owes, and a later call finishes it. With
    /// nothing due and nothing owed it writes nothing and reads no
    /// directory. It returns the ordinals of the chunk in ascending order,
    /// whether more segments are due and whether work is still owed.
    pub fn expire_due(&mut self, now: SystemTime, max_segments: usize) -> StoreResult<Pass> {
        let now = crate::event::unix_nanos(now);
        let owed_failure = self.finish_owed()?;
        self.anchor_future_segments(now)?;
        let (chunk, more_due) = self.due_chunk(now.div_euclid(NANOS_PER_SECOND), max_segments)?;
        let shredded = match self.shred_chunk(&chunk) {
            Ok(shredded) => shredded,
            Err(e) => return Err(owed_failure.unwrap_or(e)),
        };
        match owed_failure {
            Some(first) => Err(first),
            None => Ok(Pass {
                shredded,
                more_due,
                owed: self.owes_work(),
            }),
        }
    }

    fn oldest_live(&self) -> StoreResult<Option<(Uuid, Ordinal)>> {
        self.db()?
            .connection()
            .query_row(
                "SELECT segment_id, id FROM segment_keys WHERE wrapped_key IS NOT NULL \
                 ORDER BY first_capture, id LIMIT 1",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(db_error)?
            .map(|(id, ordinal)| Ok((parse_segment_id(&id)?, Ordinal::of_row(ordinal))))
            .transpose()
    }

    /// One eviction chunk against a raw byte bound. It first finishes what
    /// an earlier call owes, as [`EncryptedStore::expire_due`] does. Then,
    /// while the disk total of [`EncryptedStore::usage`] is above `bound`,
    /// it shreds the live segment of the oldest first capture, one segment
    /// at a time, each as an expiry chunk of one segment, at most
    /// `max_segments` of them; at or under the bound it shreds nothing. It
    /// ends when the disk total is at or under the bound or no live segment
    /// is left, with `more_due` false, or at `max_segments` with `more_due`
    /// true. The rows of shredded segments stay in the database, so a bound
    /// under the database's own bytes ends with no live segment and a disk
    /// total that `usage` still reads above it.
    pub fn evict_to(&mut self, bound: u64, max_segments: usize) -> StoreResult<Pass> {
        let owed_failure = self.finish_owed()?;
        let mut shredded = Vec::new();
        let more_due = loop {
            if self.usage()?.disk_bytes <= bound {
                break false;
            }
            let Some(oldest) = self.oldest_live()? else {
                break false;
            };
            if shredded.len() >= max_segments {
                break true;
            }
            match self.shred_chunk(&[oldest]) {
                Ok(one) => shredded.extend(one),
                Err(e) => return Err(owed_failure.unwrap_or(e)),
            }
        };
        if let Some(first) = owed_failure {
            return Err(first);
        }
        shredded.sort_unstable();
        Ok(Pass {
            shredded,
            more_due,
            owed: self.owes_work(),
        })
    }

    pub(in crate::store) fn destroy_segment_keys(&self, ids: &[Uuid]) -> StoreResult<u64> {
        let db = self.db()?;
        let changes_before = db.connection().total_changes();
        let tx = db.connection().unchecked_transaction().map_err(db_error)?;
        let mut shredded = 0;
        for segment_id in ids {
            if shred_in_place(&tx, &segment_id.simple().to_string())? {
                shredded += 1;
            }
        }
        tx.commit().map_err(db_error)?;
        db.look_after_the_first_commit(changes_before)?;
        db.checkpoint_truncate()
            .map_err(|e| db.kept_after_a_shred(e, changes_before))?;
        Ok(shredded)
    }

    pub(crate) fn remove_keyless_files(&self) -> StoreResult<Sweep> {
        let live: HashSet<Uuid> = self
            .live_segments()?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let swept =
            segment::sweep_segment_files(self.handle(), self.fs(), &|id| live.contains(&id))?;
        match swept.first_failure {
            Some(first) => Err(first),
            None => Ok(Sweep {
                removed: swept.removed,
                not_store_files: swept.not_store_files,
            }),
        }
    }

    /// The open-time reconcile: the shred in place, as an expiry shreds, of
    /// every live key row whose file is missing, with its totals taken out of
    /// the rollup, then the sweep of the segment files that have no live key.
    /// The shred comes first, so that a shred whose look after its commit
    /// fails, or whose checkpoint fails after its commit changed a row, leaves
    /// the sweep undone and the open, which then answers that look's answer
    /// or that checkpoint's error, has removed no file. It deletes no row, and it writes
    /// nothing when it finds no work. A step that fails leaves its work for
    /// the next open.
    pub(in crate::store) fn reconcile(&self) -> Reconciled {
        let mut reconciled = Reconciled::default();
        let missing: StoreResult<Vec<Uuid>> = self.live_segments().and_then(|live| {
            let mut missing = Vec::new();
            for (id, _) in live {
                if segment::segment_file_absent(self.handle(), id)? {
                    missing.push(id);
                }
            }
            Ok(missing)
        });
        match missing {
            Ok(missing) if missing.is_empty() => {}
            Ok(missing) => match self.destroy_segment_keys(&missing) {
                Ok(shredded) => reconciled.rows_shredded = shredded,
                Err(_) => reconciled.owed = true,
            },
            Err(_) => reconciled.owed = true,
        }
        match self.remove_keyless_files() {
            Ok(swept) => reconciled.files_removed = swept.removed,
            Err(_) => reconciled.owed = true,
        }
        reconciled
    }

    /// Delete-all on this handle, in the order of the contract, every step
    /// attempted whatever the others answered, each through the handle and
    /// never by its kept name, following no link: (1) the count of keyed
    /// segments, unknown when it cannot be read; (2) the root step, which goes
    /// through the roots directory that the store has held since its open or
    /// its create and, when that directory holds a regular file of the root
    /// file's identity at any name (the witness; for a custody that keeps no
    /// root file, the custody's own answer decides), destroys the store's own
    /// root first and then every root file there, and removes every regular
    /// file at any other name, zero-filling it first only when it has one name
    /// and a root file's length, which it reads by the name before any mode
    /// change (ruling ST-48 U1); whether a file of the root's identity is
    /// written through is decided by the name at which it stands, and a name
    /// removed with no write through it is no destruction of the root (ruling
    /// ST-48 U2); it removes a link or an entry of another kind as the entry
    /// that it is and enters no directory, and otherwise destroys nothing and
    /// reads the root as left;
    /// once a call on this handle read its root destroyed, a later call lists
    /// no roots directory again and reports from the listing that the earlier
    /// call kept; (3) the handle refuses writes from here on,
    /// whatever step 2 answered; (4) the key file, removed even when its
    /// zero-fill failed; (5) the three database files and the journal; (6)
    /// the segments tree. `Ok` with the report when the root reads destroyed,
    /// nothing stays in the roots directory at any name, that directory still
    /// stands at its path and nothing of the store is left,
    /// [`StoreError::DeleteIncomplete`] with it otherwise. A
    /// second call on the same handle runs the steps again where they are
    /// left, after an answer of [`StoreError::DeleteIncomplete`]. Once a call
    /// answered `Ok`, the handle is in the deleted state: every call but
    /// `delete_all` answers [`StoreError::Sealed`], and `delete_all` answers
    /// `Ok` with the report of the call that finished and touches nothing on
    /// disk or in the custody. It runs no bulk key delete, no checkpoint and no
    /// keyless sweep.
    pub fn delete_all(&mut self) -> StoreResult<DeleteReport> {
        if let Some(finished) = self.finished_report() {
            return Ok(finished);
        }
        let keyed_segments = self.db().ok().and_then(|db| db.keyed_segments());
        self.destroy_store_key()?;
        let mut report = self.deleting_report().unwrap_or_else(|| self.untouched());
        report.keyed_segments = keyed_segments;
        if report.complete() {
            self.mark_deleted(report.clone());
            Ok(report)
        } else {
            Err(StoreError::DeleteIncomplete(Box::new(report)))
        }
    }

    fn destroy_store_key(&mut self) -> StoreResult<()> {
        let root_step = self.root_step();
        #[cfg(test)]
        super::failpoint::seam(super::db::SEAM_AFTER_THE_ROOT_STEP);
        self.remove_files_after(root_step);
        Ok(())
    }
}

impl SegmentExpiry for EncryptedStore {
    fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport> {
        let now = SystemTime::now();
        let mut segments_shredded = 0;
        loop {
            let pass = self.expire_due(now, LEGACY_CHUNK)?;
            segments_shredded += pass.shredded.len() as u64;
            if !pass.more_due {
                return Ok(ShredReport {
                    segments_shredded,
                    keys_destroyed: segments_shredded,
                });
            }
        }
    }

    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport> {
        if self.finished_report().is_some() {
            return Ok(ShredReport::default());
        }
        let report = self.delete_all()?;
        Ok(match report.keyed_segments {
            Some(segments_shredded) => ShredReport {
                segments_shredded,
                keys_destroyed: segments_shredded + STORE_KEY_COUNT,
            },
            None => ShredReport::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::test_support::make_store;
    use crate::store::segment::segment_path;
    use crate::store::{SegmentStore, DEFAULT_RETENTION};
    use std::io::Cursor;

    fn wrapped_key_row(store: &EncryptedStore, segment_id: Uuid) -> Option<Vec<u8>> {
        store
            .db()
            .unwrap()
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE segment_id=?1",
                [segment_id.simple().to_string()],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn expired_segment_is_not_decryptable_after_shred() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, _key_id) = store
            .write_segment_with_expiry(&mut Cursor::new(b"PLAINTEXT-SECRET"), 0, 1)
            .unwrap();
        let (live, _) = store
            .write_segment(&mut Cursor::new(b"still live"))
            .unwrap();

        let report = store.crypto_shred_expired().unwrap();
        assert_eq!(report.segments_shredded, 1);

        assert!(!segment_path(dir.path(), seg_ref.segment_id).exists());
        assert!(matches!(
            store.read_segment(seg_ref.segment_id, &mut Vec::new()),
            Err(StoreError::Sealed)
        ));
        assert!(wrapped_key_row(&store, seg_ref.segment_id).is_none());
        assert!(wrapped_key_row(&store, live.segment_id).is_some());
    }

    #[test]
    fn shredding_a_segment_whose_file_is_already_gone_still_destroys_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, _) = store
            .write_segment_with_expiry(&mut Cursor::new(b"orphaned"), 0, 1)
            .unwrap();
        std::fs::remove_file(segment_path(dir.path(), seg_ref.segment_id)).unwrap();

        let report = store.crypto_shred_expired().unwrap();
        assert_eq!(report.segments_shredded, 1);
        assert!(wrapped_key_row(&store, seg_ref.segment_id).is_none());
        assert_eq!(store.crypto_shred_expired().unwrap().segments_shredded, 0);
    }

    #[test]
    fn delete_all_seals_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        store.write_segment(&mut Cursor::new(b"a")).unwrap();
        store.write_segment(&mut Cursor::new(b"b")).unwrap();
        let report = store.crypto_shred_all().unwrap();
        assert_eq!(report.segments_shredded, 2);
        assert_eq!(report.keys_destroyed, 3);
        assert!(matches!(
            store.write_segment(&mut Cursor::new(b"c")),
            Err(StoreError::Sealed)
        ));
        assert!(matches!(store.db(), Err(StoreError::Sealed)));
        drop(store);
        assert!(matches!(
            crate::store::roots::open(dir.path(), DEFAULT_RETENTION),
            Err(StoreError::Sealed)
        ));
    }
}
