use super::{connect, Mode, Scratch};

#[test]
fn a_new_database_of_this_build_reads_auto_vacuum_0_after_its_first_table() {
    crate::initialize().unwrap();
    let scratch = Scratch::new(0, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY, body TEXT);")
        .unwrap();
    let auto_vacuum: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        auto_vacuum, 0,
        "a new database of this build has auto_vacuum {auto_vacuum}"
    );
}
