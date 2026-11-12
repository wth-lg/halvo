//! SQLCipher, compiled by this crate's build script from the pinned source, and
//! the rusqlite binding that links it. A crate that opens an encrypted database
//! takes rusqlite from here, so one manifest holds the pin and the binding.

use std::ffi::c_int;
use std::sync::OnceLock;

pub use rusqlite;

/// Initialize SQLite and SQLCipher once for the process, and return the
/// result code of that one call. A thread that calls it while the first call
/// runs waits until the first call has returned, so no connection is opened
/// while SQLCipher is still starting. Call it before the first connection.
pub fn initialize() -> Result<(), c_int> {
    static INITIALIZED: OnceLock<c_int> = OnceLock::new();
    match *INITIALIZED.get_or_init(|| unsafe { rusqlite::ffi::sqlite3_initialize() }) {
        rusqlite::ffi::SQLITE_OK => Ok(()),
        code => Err(code),
    }
}

extern "C" {
    fn halvo_sqlcipher_os_entropy_hook_installed() -> c_int;
}

/// Whether LibTomCrypt draws its random bytes from the operating system's
/// entropy source through this crate's hook. SQLite installs the hook when it
/// initializes, so this reads false until the process has opened its first
/// connection.
pub fn os_entropy_hook_installed() -> bool {
    unsafe { halvo_sqlcipher_os_entropy_hook_installed() != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn the_os_entropy_hook_is_installed_once_sqlite_has_initialized() {
        drop(rusqlite::Connection::open_in_memory().unwrap());
        assert!(
            os_entropy_hook_installed(),
            "LibTomCrypt's random source is not the OS entropy hook"
        );
    }
}
