//! SQLCipher, compiled by this crate's build script from the pinned source, and
//! the rusqlite binding that links it. A crate that opens an encrypted database
//! takes rusqlite from here, so one manifest holds the pin and the binding.

pub use rusqlite;

#[cfg(test)]
mod tests {
    #[test]
    fn the_linked_library_is_the_sqlcipher_of_the_pinned_source() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let cipher: String = conn
            .query_row("PRAGMA cipher_version", [], |r| r.get(0))
            .unwrap();
        let sqlite: String = conn
            .query_row("SELECT sqlite_version()", [], |r| r.get(0))
            .unwrap();
        assert!(cipher.starts_with("4.19.0"), "cipher_version {cipher}");
        assert_eq!(sqlite, "3.53.4");
    }
}
