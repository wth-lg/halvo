//! The state that the page receives (I8.1, I8.2): the whole state with this run's own count
//! `seq`, which rises by one whenever the rows, the items or the note change, so that a
//! snapshot and the changes that follow it give the state of a new snapshot.

use crate::types::{Item, Row, Snapshot};

/// The last state that the coordinator composed, with its `seq`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sync {
    seq: u32,
    rows: Vec<Row>,
    items: Vec<Item>,
    note: Option<String>,
}

/// The answer to a page that holds the state of a `seq` (I8.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// The whole state, whose `seq` is not the page's.
    Changed(Snapshot),
    /// No change since the page's `seq`: the app's waiting call answers `same` once
    /// `next_wait_s` have passed, or the whole state at the next change.
    Same,
}

impl Default for Sync {
    fn default() -> Sync {
        Sync::new()
    }
}

impl Sync {
    /// The state before the first move: `seq` 1, no row, no item and no note.
    pub fn new() -> Sync {
        Sync {
            seq: 1,
            rows: Vec::new(),
            items: Vec::new(),
            note: None,
        }
    }

    /// Takes the state that a move left; `seq` rises by one when it differs from the last, and
    /// the answer says whether it did.
    pub fn publish(&mut self, rows: Vec<Row>, items: Vec<Item>, note: Option<String>) -> bool {
        if rows == self.rows && items == self.items && note == self.note {
            return false;
        }
        self.rows = rows;
        self.items = items;
        self.note = note;
        self.seq = self.seq.saturating_add(1);
        true
    }

    /// The reply of `agents_snapshot`: the whole state.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            seq: self.seq,
            rows: self.rows.clone(),
            items: self.items.clone(),
            note: self.note.clone(),
        }
    }

    /// The reply of `agents_next` for a page that holds the state of `after`: the whole state
    /// when `seq` is another, else `Same`.
    pub fn next(&self, after: u32) -> Next {
        if after == self.seq {
            Next::Same
        } else {
            Next::Changed(self.snapshot())
        }
    }

    /// The reply of `agents_next` after `next_wait_s` with no change: the old `seq`, empty
    /// lists and no note.
    pub fn same(&self) -> Snapshot {
        Snapshot {
            seq: self.seq,
            rows: Vec::new(),
            items: Vec::new(),
            note: None,
        }
    }
}
