pub(crate) const SCHEMA_VERSION: i64 = 1;

pub(crate) const SCHEMA: &str = "\
CREATE TABLE meta (one INTEGER PRIMARY KEY CHECK (one = 1), schema INTEGER NOT NULL, site_key BLOB NOT NULL, \
store TEXT, seg INTEGER, idx INTEGER, mark INTEGER NOT NULL) STRICT;
CREATE TABLE revisions (family TEXT PRIMARY KEY NOT NULL, n INTEGER NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE purged (ordinal INTEGER PRIMARY KEY) STRICT;
CREATE TABLE rows (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, family TEXT NOT NULL, class INTEGER NOT NULL, \
day INTEGER, expires INTEGER, workflow TEXT, summary TEXT, wkey TEXT, body TEXT NOT NULL) STRICT;
CREATE TABLE sources (ordinal INTEGER NOT NULL, row INTEGER NOT NULL REFERENCES rows(id) ON DELETE CASCADE, \
PRIMARY KEY (ordinal, row)) STRICT, WITHOUT ROWID;
CREATE INDEX sources_row ON sources(row);
CREATE INDEX rows_class ON rows(class);
CREATE INDEX rows_day ON rows(day);
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
}
