use halvo_schema_core::record::Ordinal;

use crate::error::Fault;
use crate::sql;
use crate::Store;

fn as_i64(ordinal: Ordinal) -> Result<i64, Fault> {
    i64::try_from(ordinal.get()).map_err(|_| Fault::Row)
}

impl Store {
    /// Takes the ordinals of segments that the capture store has shredded and removes, in one transaction, every
    /// window row, of the evidence and of every registered window kind, that names one of them. It returns the number
    /// of rows removed, raises the revision of every family that lost a row and refuses nothing. The store keeps each
    /// ordinal at or above its high-water mark, so that a later commit stores no window row that names it; the mark
    /// rises over the kept ordinals that follow it. The truncating checkpoint runs after it.
    pub fn purge(&mut self, ordinals: &[Ordinal]) -> Result<u64, Fault> {
        let db = &self.db;
        db.arm();
        db.write_checkpointed(|tx| {
            let mark = tx.mark()?;
            let mut removed = 0;
            for ordinal in ordinals {
                let n = as_i64(*ordinal)?;
                removed += tx.remove(sql::PURGE_ROWS, [n])?;
                if ordinal.get() >= mark {
                    tx.exec(sql::PURGED_INSERT, [n])?;
                }
            }
            tx.raise_mark(mark)?;
            Ok::<_, Fault>(removed)
        })
    }

    /// Does what `purge` does for every ordinal below `first_live`, the lowest live ordinal of the capture store, and
    /// raises the high-water mark to it.
    pub fn purge_below(&mut self, first_live: Ordinal) -> Result<u64, Fault> {
        let db = &self.db;
        db.arm();
        db.write_checkpointed(|tx| {
            let removed = tx.remove(sql::PURGE_BELOW_ROWS, [as_i64(first_live)?])?;
            tx.raise_mark(first_live.get())?;
            Ok::<_, Fault>(removed)
        })
    }
}
