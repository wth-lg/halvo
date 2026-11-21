use halvo_sqlcipher::rusqlite::Connection;

#[test]
fn the_library_starts_with_its_log_level_at_none_in_a_process_that_opened_no_capture_store() {
    halvo_sqlcipher::initialize().unwrap();
    let level: String = Connection::open_in_memory()
        .unwrap()
        .query_row("PRAGMA cipher_log_level", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        level, "NONE",
        "the log level of a process that set none after the library's start"
    );
}
