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

const COMPILE_OPTIONS: [&str; 50] = [
    "ATOMIC_INTRINSICS=1",
    "DEFAULT_AUTOVACUUM",
    "DEFAULT_CACHE_SIZE=-2000",
    "DEFAULT_FILE_FORMAT=4",
    "DEFAULT_FOREIGN_KEYS",
    "DEFAULT_JOURNAL_SIZE_LIMIT=-1",
    "DEFAULT_MMAP_SIZE=0",
    "DEFAULT_PAGE_SIZE=4096",
    "DEFAULT_PCACHE_INITSZ=20",
    "DEFAULT_RECURSIVE_TRIGGERS",
    "DEFAULT_SECTOR_SIZE=4096",
    "DEFAULT_SYNCHRONOUS=2",
    "DEFAULT_WAL_AUTOCHECKPOINT=1000",
    "DEFAULT_WAL_SYNCHRONOUS=2",
    "DEFAULT_WORKER_THREADS=0",
    "DIRECT_OVERFLOW_READ",
    "ENABLE_API_ARMOR",
    "ENABLE_COLUMN_METADATA",
    "ENABLE_DBSTAT_VTAB",
    "ENABLE_FTS3",
    "ENABLE_FTS3_PARENTHESIS",
    "ENABLE_FTS5",
    "ENABLE_RTREE",
    "EXTRA_INIT=halvo_sqlcipher_extra_init",
    "EXTRA_SHUTDOWN=sqlcipher_extra_shutdown",
    "HAS_CODEC",
    "HAVE_ISNAN",
    "MALLOC_SOFT_LIMIT=1024",
    "MAX_ATTACHED=10",
    "MAX_COLUMN=2000",
    "MAX_COMPOUND_SELECT=500",
    "MAX_DEFAULT_PAGE_SIZE=8192",
    "MAX_EXPR_DEPTH=1000",
    "MAX_FUNCTION_ARG=1000",
    "MAX_LENGTH=1000000000",
    "MAX_LIKE_PATTERN_LENGTH=50000",
    "MAX_MMAP_SIZE=0x7fff0000",
    "MAX_PAGE_COUNT=0xfffffffe",
    "MAX_PAGE_SIZE=65536",
    "MAX_SQL_LENGTH=1000000000",
    "MAX_TRIGGER_DEPTH=1000",
    "MAX_VARIABLE_NUMBER=32766",
    "MAX_VDBE_OP=250000000",
    "MAX_WORKER_THREADS=8",
    "MUTEX_PTHREADS",
    "SOUNDEX",
    "SYSTEM_MALLOC",
    "TEMP_STORE=2",
    "THREADSAFE=1",
    "USE_URI",
];

#[test]
fn the_compile_options_of_this_build_are_its_closed_list() {
    crate::initialize().unwrap();
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let mut statement = conn.prepare("PRAGMA compile_options").unwrap();
    let options: Vec<String> = statement
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .filter(|option| !option.starts_with("COMPILER="))
        .collect();
    let expected: Vec<&str> = COMPILE_OPTIONS
        .iter()
        .map(|option| match *option {
            "ATOMIC_INTRINSICS=1" if cfg!(target_env = "msvc") => "ATOMIC_INTRINSICS=0",
            "MUTEX_PTHREADS" if cfg!(windows) => "MUTEX_W32",
            other => other,
        })
        .collect();
    assert_eq!(options, expected);
}
