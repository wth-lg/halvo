//! `route-gate import` (contract T5.4): not built yet.

use std::io::Write;

/// Why `write_files` left no file of its own in the directory.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WriteFault {
    Exists(String),
    Unwritable,
}

/// Not built yet.
pub fn blob_id(_content: &[u8]) -> String {
    String::new()
}

/// Not built yet.
pub fn write_files(
    _dir: &str,
    _files: &[(&str, String)],
    _before_link: &mut dyn FnMut(&str),
) -> Result<(), WriteFault> {
    Err(WriteFault::Unwritable)
}

/// Not built yet: refuses every call.
pub fn command(_args: &[String], _stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let _ = writeln!(stderr, "REFUSED usage: not built");
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_known() {
        assert_eq!(blob_id(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
        assert_eq!(
            blob_id(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
    }
}
