//! The ruling of contract T1.5: the owner's word on what may leave the machine, held as one line.
//!
//! This file is pinned as leave.rs is: the test `ruling_rs_is_pinned` holds the SHA-256 of its
//! code from its first byte to its test module, and requires that the test module is its last
//! item, so a change of that code writes the new hash in the same commit.

use crate::tier::Tier;

/// The refusal of `Ruling::parse`: the line is not a ruling line.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RulingError;

impl RulingError {
    pub const fn as_str(self) -> &'static str {
        "ruling"
    }
}

/// What the owner has ruled. Every build that does not set `cfg(test)` has the one value `None`:
/// no tier but `on_device` may be picked and no origin may be reached. A build that sets
/// `cfg(test)` also has the test value `TestHosted`, which the crate's own code alone can make,
/// whatever flags the build sets: its payload has a private field and no public maker, and its
/// maker is private to the crate.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Ruling {
    None,
    #[cfg(test)]
    TestHosted(TestOrigins),
}

/// The payload of the test value: the origins that it allows. Its one field is private and it has
/// no public maker, so no code outside this crate can make one.
#[cfg(test)]
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TestOrigins {
    origins: Vec<String>,
}

impl Ruling {
    /// `None` for the line `ruling=none`, and `RulingError` for every other line.
    pub fn parse(line: &str) -> Result<Ruling, RulingError> {
        if line == "ruling=none" {
            Ok(Ruling::None)
        } else {
            Err(RulingError)
        }
    }

    /// The tiers that a pick may name: `on_device` alone for `None`.
    pub fn allowed(&self) -> &'static [Tier] {
        match self {
            Ruling::None => &[Tier::OnDevice],
            #[cfg(test)]
            Ruling::TestHosted(_) => &Tier::ALL,
        }
    }

    /// The origins that a request may go to: none for `None`.
    pub fn origins(&self) -> &[String] {
        match self {
            Ruling::None => &[],
            #[cfg(test)]
            Ruling::TestHosted(test) => &test.origins,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_hosted(origins: &[&str]) -> Ruling {
        Ruling::TestHosted(TestOrigins {
            origins: origins.iter().map(|origin| (*origin).to_owned()).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "ruling=none";

    fn refused_near(line: &str) -> Vec<String> {
        let bytes = line.as_bytes();
        let mut near = Vec::new();
        for at in 0..=bytes.len() {
            for byte in 0..=127u8 {
                let mut inserted = bytes.to_vec();
                inserted.insert(at, byte);
                near.push(String::from_utf8(inserted).expect("ascii"));
                if at < bytes.len() && bytes[at] != byte {
                    let mut replaced = bytes.to_vec();
                    replaced[at] = byte;
                    near.push(String::from_utf8(replaced).expect("ascii"));
                }
            }
            if at < bytes.len() {
                near.push(line[..at].to_owned());
                let mut removed = bytes.to_vec();
                removed.remove(at);
                near.push(String::from_utf8(removed).expect("ascii"));
            }
            near.push(line[at..].to_owned());
        }
        near.retain(|text| text != line);
        near
    }

    #[test]
    fn ruling_test_value() {
        let given = [
            "https://a.example",
            "https://a.example",
            "https://A.example",
            "https://a.example/",
            "",
            "127.0.0.1:1",
        ];
        let test = Ruling::test_hosted(&given);
        assert_eq!(test.allowed(), &Tier::ALL);
        assert_eq!(test.origins(), given.map(str::to_owned));
        assert_eq!(Ruling::test_hosted(&[]).allowed(), &Tier::ALL);
        assert!(Ruling::test_hosted(&[]).origins().is_empty());

        assert_eq!(Ruling::parse(LINE), Ok(Ruling::None));
        assert_eq!(Ruling::None.allowed(), &[Tier::OnDevice]);
        assert!(Ruling::None.origins().is_empty());
        assert_eq!(RulingError.as_str(), "ruling");

        let mut lines = refused_near(LINE);
        for line in [
            "ruling=none\n",
            "ruling=none\r",
            "ruling=none\r\n",
            "\u{feff}ruling=none",
            "ruling=None",
            "RULING=NONE",
            "ruling=test_hosted",
            "ruling=hosted",
            "ruling=all",
            "ruling = none",
            "ruling=none ruling=none",
            "ruling=none\u{e9}",
            "ruling=\u{ff4e}one",
        ] {
            lines.push(line.to_owned());
        }
        assert!(lines.len() > 2_000, "{}", lines.len());
        for line in &lines {
            assert_eq!(Ruling::parse(line), Err(RulingError), "{line:?}");
        }
    }

    #[test]
    fn ruling_rs_is_pinned() {
        fn sha256_hex(bytes: &[u8]) -> String {
            use sha2::{Digest, Sha256};
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        }

        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ruling.rs"))
            .expect("the crate's ruling.rs");
        let mark = "\n#[cfg(test)]\nmod tests {\n";
        assert_eq!(text.matches(mark).count(), 1);
        let end = text.find(mark).expect("the test module") + mark.len();
        assert_eq!(
            sha256_hex(&text.as_bytes()[..end]),
            RULING_ABOVE_TESTS_SHA256,
            "ruling.rs changed above its test module: write the new hash in the same commit, and the reviewer of that commit reads the change"
        );
        let after: Vec<&str> = text[end..].lines().collect();
        assert_eq!(after.last(), Some(&"}"));
        for line in &after[..after.len() - 1] {
            assert!(
                line.is_empty() || line.starts_with(' '),
                "an item after the tests: {line}"
            );
        }
    }

    const RULING_ABOVE_TESTS_SHA256: &str =
        "c832ae1e001f69669ae0ae19a59fd383ecb02f632c06f5824691aa4199b7f5fc";
}
