pub(crate) const SCHEMA_VERSION: i64 = 1;

pub(crate) const SCHEMA: &str = "\
CREATE TABLE meta (one INTEGER PRIMARY KEY CHECK (one = 1), schema INTEGER NOT NULL, site_key BLOB NOT NULL, \
store TEXT, seg INTEGER, idx INTEGER, mark INTEGER NOT NULL) STRICT;
CREATE TABLE revisions (family TEXT PRIMARY KEY NOT NULL, n INTEGER NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE purged (ordinal INTEGER PRIMARY KEY) STRICT;
CREATE TABLE rows (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, family TEXT NOT NULL, class INTEGER NOT NULL, \
day INTEGER, gone INTEGER, expires INTEGER, workflow TEXT, summary TEXT, wkey TEXT, body TEXT NOT NULL) STRICT;
CREATE TABLE sources (ordinal INTEGER NOT NULL, row INTEGER NOT NULL REFERENCES rows(id) ON DELETE CASCADE, \
PRIMARY KEY (ordinal, row)) STRICT, WITHOUT ROWID;
CREATE INDEX sources_row ON sources(row);
CREATE INDEX rows_class ON rows(class);
CREATE INDEX rows_day ON rows(day);
CREATE INDEX rows_gone ON rows(gone);
CREATE INDEX rows_expires ON rows(expires);
CREATE INDEX rows_workflow ON rows(workflow);
CREATE INDEX rows_summary ON rows(summary, kind);
CREATE INDEX rows_wkey ON rows(wkey, kind);
CREATE INDEX rows_kind_workflow ON rows(kind, workflow, summary);
CREATE INDEX rows_kind_day ON rows(kind, day, summary);
CREATE UNIQUE INDEX rows_workflow_id ON rows(workflow) WHERE kind = 'workflow';
CREATE UNIQUE INDEX rows_workflow_key ON rows(wkey) WHERE kind = 'workflow';
CREATE INDEX rows_read ON rows(kind, body) WHERE family <> 'workflow';
";

pub(crate) const MASTER: &str = "SELECT type, name, tbl_name, sql FROM sqlite_master";

#[derive(Clone, Copy)]
pub(crate) struct Sql(&'static str);

impl Sql {
    pub(crate) fn text(self) -> &'static str {
        self.0
    }
}

// When a stored row is due: an expiry at the clock removes it and no read hands it out (DS-4). Each of the four
// conditions is written once, here, for the table name or alias that it is given, and the expiry's statements and the
// reads' condition are both made from it, so that a change of one condition changes what the expiry removes and what
// each read hands out alike. The named parameters are one reading of the host's clock: `:today` its day, `:ahead` the
// last day on which a stored row may lie (two days after `:today`, `Now::ahead_limit`), `:now` its instant. The class
// code 1 is the bounded class (`db::class_code`).

/// A bounded row whose day of going has come.
macro_rules! gone_has_come {
    ($r:literal) => {
        concat!($r, ".gone <= :today")
    };
}

/// A row whose day lies more than two days after the clock's day.
macro_rules! too_far_ahead {
    ($r:literal) => {
        concat!($r, ".day > :ahead")
    };
}

/// A bounded row with no day.
macro_rules! bounded_without_day {
    ($r:literal) => {
        concat!("(", $r, ".class = 1 AND ", $r, ".day IS NULL)")
    };
}

/// A window row whose expiry instant has come.
macro_rules! expiry_has_come {
    ($r:literal) => {
        concat!($r, ".expires <= :now")
    };
}

/// A row that the expiry at the clock would leave: none of the four conditions holds. A condition on an empty
/// column is not true, so a kept row, which has no `gone`, no `day` and no `expires`, is always handed out.
macro_rules! handed_out {
    ($r:literal) => {
        concat!(
            "(",
            gone_has_come!($r),
            " OR ",
            too_far_ahead!($r),
            " OR ",
            bounded_without_day!($r),
            " OR ",
            expiry_has_come!($r),
            ") IS NOT TRUE"
        )
    };
}

/// An evidence or a support row whose summary is a row that the expiry would leave: the expiry's sweep, as the
/// reads hold it.
macro_rules! its_summary_handed_out {
    ($r:literal) => {
        concat!(
            "EXISTS (SELECT 1 FROM rows AS m WHERE m.summary = ",
            $r,
            ".summary AND m.kind = 'summary' AND ",
            handed_out!("m"),
            ")"
        )
    };
}

macro_rules! catalogue {
    ($($name:ident = $text:expr;)+) => {
        $(pub(crate) const $name: Sql = Sql($text);)+
        #[cfg(test)]
        pub(crate) const CATALOGUE: &[(&str, Sql)] = &[$((stringify!($name), $name),)+];
    };
}

catalogue! {
    META_INSERT = "INSERT INTO meta (one, schema, site_key, mark) VALUES (1, ?1, ?2, 0)";
    META_SCHEMA = "SELECT schema FROM meta WHERE one = 1";
    REVISION_ENSURE = "INSERT OR IGNORE INTO revisions (family, n) VALUES (?1, 0)";
    REVISION_BUMP = "UPDATE revisions SET n = n + 1 WHERE family = ?1";
    ORPHAN_EVIDENCE = "SELECT id, family FROM rows WHERE kind IN ('evidence', 'support') AND NOT EXISTS \
        (SELECT 1 FROM rows AS s WHERE s.summary = rows.summary AND s.kind = 'summary')";
    ORPHAN_WORKFLOW = "SELECT id, family FROM rows WHERE workflow IS NOT NULL AND kind <> 'workflow' AND NOT EXISTS \
        (SELECT 1 FROM rows AS w WHERE w.workflow = rows.workflow AND w.kind = 'workflow')";
    ROW_DELETE = "DELETE FROM rows WHERE id = ?1";
    META_READ = "SELECT store, seg, idx FROM meta WHERE one = 1";
    META_SITE_KEY = "SELECT site_key FROM meta WHERE one = 1";
    META_POSITION = "UPDATE meta SET store = ?1, seg = ?2, idx = ?3 WHERE one = 1";
    SUMMARY_STORED = "SELECT id FROM rows WHERE summary = ?1 AND kind = 'summary'";
    ROW_INSERT = "INSERT INTO rows (kind, family, class, day, gone, expires, workflow, summary, wkey, body) \
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";
    SOURCE_INSERT = "INSERT INTO sources (ordinal, row) VALUES (?1, ?2)";
    MARK_READ = "SELECT mark FROM meta WHERE one = 1";
    MARK_SET = "UPDATE meta SET mark = ?1 WHERE one = 1";
    PURGED_HAS = "SELECT ordinal FROM purged WHERE ordinal = ?1";
    PURGED_INSERT = "INSERT OR IGNORE INTO purged (ordinal) VALUES (?1)";
    PURGED_BELOW_DELETE = "DELETE FROM purged WHERE ordinal < ?1";
    PURGE_ROWS = "SELECT sources.row, rows.family FROM sources JOIN rows ON rows.id = sources.row \
        WHERE sources.ordinal = ?1";
    PURGE_BELOW_ROWS = "SELECT sources.row, rows.family FROM sources JOIN rows ON rows.id = sources.row \
        WHERE sources.ordinal < ?1";
    SUMMARY_READ = concat!(
        "SELECT body FROM rows WHERE kind = 'summary' AND day <= :last AND ",
        handed_out!("rows"),
        " ORDER BY day, summary"
    );
    SUMMARY_DAY_KEY = "SELECT day, wkey FROM rows WHERE summary = ?1 AND kind = 'summary'";
    WORKFLOW_ROW = "SELECT id FROM rows WHERE workflow = ?1 AND kind = 'workflow'";
    WORKFLOW_BY_KEY = "SELECT id, body FROM rows WHERE wkey = ?1 AND kind = 'workflow'";
    SUPPORT_OF = "SELECT id, summary FROM rows WHERE kind = 'support' AND workflow = ?1 ORDER BY summary";
    ROW_BODY_SET = "UPDATE rows SET body = ?2 WHERE id = ?1";
    WORKFLOW_BY_ID = "SELECT id, body FROM rows WHERE workflow = ?1 AND kind = 'workflow'";
    WORKFLOW_KEY = "SELECT wkey FROM rows WHERE workflow = ?1 AND kind = 'workflow'";
    SUPPORT_REMOVE = "SELECT id, family FROM rows WHERE kind = 'support' AND workflow = ?1";
    REMOVE_NAMING = "SELECT id, family FROM rows WHERE workflow = ?1";
    KEY_SUMMARIES = "SELECT summary FROM rows WHERE wkey = ?1 AND kind = 'summary'";
    REMOVE_OF_SUMMARY = "SELECT id, family FROM rows WHERE summary = ?1";
    ROWS_READ = concat!(
        "SELECT body FROM rows WHERE kind = :kind AND family <> 'workflow' AND ",
        handed_out!("rows"),
        " ORDER BY body"
    );
    EXPIRE_GONE = concat!("SELECT id, family FROM rows WHERE ", gone_has_come!("rows"));
    EXPIRE_AHEAD = concat!("SELECT id, family FROM rows WHERE ", too_far_ahead!("rows"));
    EXPIRE_NO_DAY = concat!("SELECT id, family FROM rows WHERE ", bounded_without_day!("rows"));
    EXPIRE_WINDOW = concat!("SELECT id, family FROM rows WHERE ", expiry_has_come!("rows"));
    BOUND_GONE = "UPDATE rows SET gone = day + ?2 WHERE kind = ?1 AND gone > day + ?2";
    STORED_OTHER_CLASS = "SELECT id FROM rows WHERE kind = ?1 AND class <> ?2 LIMIT 1";
    STORED_OTHER_WORKFLOW = "SELECT id FROM rows WHERE kind = ?1 AND workflow IS NOT NULL \
        AND json_extract(body, ?2) IS NOT workflow LIMIT 1";
    WORKFLOW_FILL = "UPDATE rows SET workflow = json_extract(body, ?2) WHERE kind = ?1 AND workflow IS NULL";
    REVISION_READ = "SELECT n FROM revisions WHERE family = ?1";
    COUNT_CLASS = "SELECT count(*) FROM rows WHERE class = ?1";
    MIN_EXPIRES = "SELECT min(expires) FROM rows";
    MIN_GONE = "SELECT min(gone) FROM rows";
    WORKFLOW_BODY = "SELECT body FROM rows WHERE workflow = ?1 AND kind = 'workflow'";
    EVIDENCE_OF = concat!(
        "SELECT e.body FROM rows AS e WHERE e.summary = :summary AND e.kind = 'evidence' AND ",
        handed_out!("e"),
        " AND ",
        its_summary_handed_out!("e")
    );
    SUPPORT_HANDED_OUT = concat!(
        "SELECT s.summary FROM rows AS s WHERE s.kind = 'support' AND s.workflow = :workflow AND ",
        handed_out!("s"),
        " AND ",
        its_summary_handed_out!("s"),
        " ORDER BY s.summary"
    );
    WORKFLOWS_AFTER = "SELECT workflow FROM rows WHERE kind = 'workflow' AND workflow > ?1 ORDER BY workflow";
    SUPPORT_WITH_EVIDENCE = concat!(
        "SELECT s.summary FROM rows AS s WHERE s.kind = 'support' AND s.workflow = :workflow ",
        "AND s.summary > :after AND ",
        handed_out!("s"),
        " AND ",
        its_summary_handed_out!("s"),
        " AND EXISTS (SELECT 1 FROM rows AS e WHERE e.summary = s.summary AND e.kind = 'evidence' AND ",
        handed_out!("e"),
        ") ORDER BY s.summary"
    );
}
