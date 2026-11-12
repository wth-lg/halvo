//! The ruling of contract T1.5: the owner's word on what may leave the machine, held as one line.
//!
//! This file is pinned whole, its test module included, as lib.rs, leave.rs and tier.rs are: the
//! test `pinned_files_hold_their_hashes` of tests/no_grant_in_release.rs holds its SHA-256. A pin
//! stops no change. It makes each change of the file come with a new hash line in the same commit,
//! where the reviewer of that commit reads it.

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
/// `cfg(test)` also has the test value `TestHosted`. No file but this one can name its maker
/// `test_hosted` or its payload's type `TestOrigins`, or set its origins, whatever flags the build
/// sets: the maker is private to this file and the type stands in a module private to it. Other
/// files can name the variant `Ruling::TestHosted` and, from the value that
/// `admit_under_test_value` hands to the body of `admit`, hold or make again a test value with no
/// origin, under which `grant` makes no grant. The tests of leave.rs get a test value with origins
/// through a key that leave.rs alone can make.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Ruling {
    None,
    #[cfg(test)]
    TestHosted(test_value::TestOrigins),
}

#[cfg(test)]
mod test_value {
    /// The payload of the test value: the origins that it allows. No file but ruling.rs can name
    /// this type or set its one field.
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub struct TestOrigins {
        pub(super) origins: Vec<String>,
    }
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
    fn test_hosted(origins: &[&str]) -> Ruling {
        Ruling::TestHosted(test_value::TestOrigins {
            origins: origins.iter().map(|origin| (*origin).to_owned()).collect(),
        })
    }

    /// The test value that allows `origins`, for the tests of leave.rs, which alone can make the
    /// key.
    #[cfg(test)]
    pub(crate) fn test_hosted_for_leave(_key: crate::leave::TestKey, origins: &[&str]) -> Ruling {
        Ruling::test_hosted(origins)
    }
}

/// The one `Drop` of `Ruling` in a build that sets `cfg(test)`. It does nothing. It stands here so
/// that no other file of the crate can write a `Drop` of `Ruling` that keeps a test value as the
/// value is dropped: a second one does not compile (E0119).
#[cfg(test)]
impl Drop for Ruling {
    fn drop(&mut self) {}
}

/// `admit` under the test value that allows no origin, for the tests of admit.rs. The value reaches
/// the body of `admit` by reference; it allows no origin, so `grant` makes no grant under it or
/// under a value made again from it.
#[cfg(test)]
pub(crate) fn admit_under_test_value(
    room: Option<&crate::admit::Room>,
    estimate: crate::prices::Micro,
    running: &[crate::prices::Micro],
) -> crate::admit::Admission {
    crate::admit::admit(&Ruling::test_hosted(&[]), room, estimate, running)
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
}
