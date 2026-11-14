//! SQLCipher, compiled by this crate's build script from a copy of the pinned
//! source with two statements of Halvo's own (under secure delete, the gap of a
//! page that the database builds again or edits in place is zeroed;
//! docs/adr/0013), and the rusqlite binding that links it. A crate that opens an encrypted database takes
//! rusqlite from here, so one manifest holds the pin and the binding.

use std::ffi::c_int;
use std::sync::OnceLock;

pub use rusqlite;

/// Initialize SQLite and SQLCipher once for the process, and return the
/// result code of that one call. A thread that calls it while the first call
/// runs waits until the first call has returned.
/// Call it before the first connection.
pub fn initialize() -> Result<(), c_int> {
    static INITIALIZED: OnceLock<c_int> = OnceLock::new();
    match *INITIALIZED.get_or_init(|| unsafe { rusqlite::ffi::sqlite3_initialize() }) {
        rusqlite::ffi::SQLITE_OK => Ok(()),
        code => Err(code),
    }
}

extern "C" {
    fn halvo_sqlcipher_os_entropy_hook_installed() -> c_int;
    fn halvo_sqlcipher_page_gap_mend() -> c_int;
}

/// Whether LibTomCrypt draws its random bytes from the operating system's
/// entropy source through this crate's hook. SQLite installs the hook when it
/// initializes.
pub fn os_entropy_hook_installed() -> bool {
    unsafe { halvo_sqlcipher_os_entropy_hook_installed() != 0 }
}

/// Whether the linked SQLCipher is this crate's build, whose copy of the
/// pinned source zeroes, under secure delete, the gap of a page that the
/// database builds again or edits in place. Only that copy defines the
/// function that answers 2 here, in the same object as the database's code,
/// so a program that calls this links that object or does not link at all,
/// and a library of the earlier build with one statement, which answers 1,
/// reads `false`. Each store's open is to require `true`, as it requires
/// secure delete.
pub fn page_gap_mend_linked() -> bool {
    unsafe { halvo_sqlcipher_page_gap_mend() == 2 }
}

#[cfg(test)]
#[path = "../tests/page_gap/mod.rs"]
mod page_gap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_linked_library_is_the_pinned_sqlcipher_with_the_page_gap_mend() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let cipher: String = conn
            .query_row("PRAGMA cipher_version", [], |r| r.get(0))
            .unwrap();
        let sqlite: String = conn
            .query_row("SELECT sqlite_version()", [], |r| r.get(0))
            .unwrap();
        assert!(cipher.starts_with("4.19.0"), "cipher_version {cipher}");
        assert_eq!(sqlite, "3.53.4");
        assert!(
            page_gap_mend_linked(),
            "the linked SQLCipher lacks the mend"
        );
    }

    #[test]
    fn the_os_entropy_hook_is_installed_once_sqlite_has_initialized() {
        drop(rusqlite::Connection::open_in_memory().unwrap());
        assert!(
            os_entropy_hook_installed(),
            "LibTomCrypt's random source is not the OS entropy hook"
        );
    }

    #[test]
    fn the_os_entropy_hook_hands_libtomcrypt_bytes_that_differ_between_draws() {
        extern "C" {
            fn rng_get_bytes(
                out: *mut u8,
                outlen: std::ffi::c_ulong,
                callback: Option<extern "C" fn()>,
            ) -> std::ffi::c_ulong;
        }
        drop(rusqlite::Connection::open_in_memory().unwrap());
        assert!(os_entropy_hook_installed());
        let (mut first, mut second) = ([0u8; 32], [0u8; 32]);
        let drawn = unsafe {
            (
                rng_get_bytes(first.as_mut_ptr(), 32, None),
                rng_get_bytes(second.as_mut_ptr(), 32, None),
            )
        };
        assert_eq!(drawn, (32, 32));
        assert_ne!(first, [0u8; 32], "the hook handed LibTomCrypt zero bytes");
        assert_ne!(
            first, second,
            "two draws through the hook gave the same bytes"
        );
    }
}
