const DEVICES: [&str; 4] = ["con", "prn", "aux", "nul"];
const NUMBERED_DEVICES: [&str; 2] = ["com", "lpt"];

/// The name of one stored item: 1 to 64 bytes of `a` to `z`, `0` to `9`, `-` and `.`,
/// whose first and last byte is a letter or a digit, so that it is a file name on macOS
/// and Windows and an attribute of a Keychain item. A name whose part before its first
/// `.` is a device name of Windows (`con`, `prn`, `aux`, `nul`, `com0` to `com9`, `lpt0`
/// to `lpt9`) is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name(String);

impl Name {
    /// Parses `name`, or answers `None` when it is not of the form above.
    pub fn new(name: &str) -> Option<Name> {
        let bytes = name.as_bytes();
        let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
            return None;
        };
        if bytes.len() > 64 || !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
            return None;
        }
        if !bytes
            .iter()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.'))
        {
            return None;
        }
        let stem = name.split('.').next().unwrap_or(name);
        if is_device(stem) {
            return None;
        }
        Some(Name(name.to_string()))
    }

    /// The name as it was parsed.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_device(stem: &str) -> bool {
    if DEVICES.contains(&stem) {
        return true;
    }
    NUMBERED_DEVICES.iter().any(|prefix| {
        stem.strip_prefix(prefix)
            .is_some_and(|rest| rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit())
    })
}
