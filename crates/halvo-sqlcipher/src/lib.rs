//! SQLCipher, compiled by this crate's build script from the pinned source, and
//! the rusqlite binding that links it. A crate that opens an encrypted database
//! takes rusqlite from here, so one manifest holds the pin and the binding.

pub use rusqlite;
