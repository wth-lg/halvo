//! The custody selection of contracts.md C20.5: one pure function from the
//! cargo feature `secure-enclave`, the system and the custody kind that a key
//! file records, to the kind that a create uses and to how a store that
//! stands opens. It reads no build profile: the feature comes in as an
//! argument.

/// The system that a build runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    Macos,
    Windows,
}

impl System {
    /// The system of this build.
    pub const fn this() -> System {
        if cfg!(target_os = "windows") {
            System::Windows
        } else {
            System::Macos
        }
    }
}

/// The custody kind that a key file records, as the policy reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Custody {
    /// A root file under the roots directory, protected by its mode alone:
    /// the custody of macOS in a build without the feature.
    RootFile,
    /// A root file under the roots directory, protected by DPAPI: the custody
    /// of Windows.
    Dpapi,
    /// A key under the Secure Enclave, which R1 does not build.
    SecureEnclave,
}

/// How a store that stands opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAs {
    /// Open, and capture into it.
    ReadWrite,
    /// Open for expiry, eviction, listing and delete-all, with no write.
    MaintenanceOnly,
    /// Not opened: the slot reads sealed with the cause custody_kind.
    Refused,
}

/// What the policy answers for one data directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// No key file stands: the custody that a create uses, or none, when the
    /// build makes no store.
    Create(Option<Custody>),
    /// A key file stands: how its store opens.
    Open(OpenAs),
}

/// The selection of a build with the feature `secure-enclave` or without it,
/// on `system`, for a key file that records `recorded`, or for none
/// (contracts.md C20.2 to C20.4). Each system creates and captures on its own
/// custody. A macOS build with the feature creates no store and opens a
/// root-file store for maintenance alone. A key file of any other kind is
/// refused.
pub const fn select(secure_enclave: bool, system: System, recorded: Option<Custody>) -> Selection {
    let own = match system {
        System::Macos => Custody::RootFile,
        System::Windows => Custody::Dpapi,
    };
    let captures = !(secure_enclave && matches!(system, System::Macos));
    match recorded {
        None if captures => Selection::Create(Some(own)),
        None => Selection::Create(None),
        Some(kind) => {
            let is_own = matches!(
                (kind, own),
                (Custody::RootFile, Custody::RootFile) | (Custody::Dpapi, Custody::Dpapi)
            );
            Selection::Open(match (is_own, captures) {
                (true, true) => OpenAs::ReadWrite,
                (true, false) => OpenAs::MaintenanceOnly,
                (false, _) => OpenAs::Refused,
            })
        }
    }
}

/// The selection of this build on this system.
pub const fn of_this_build(recorded: Option<Custody>) -> Selection {
    select(cfg!(feature = "secure-enclave"), System::this(), recorded)
}

#[cfg(test)]
mod tests {
    use super::Custody::{Dpapi, RootFile, SecureEnclave};
    use super::OpenAs::{MaintenanceOnly, ReadWrite, Refused};
    use super::Selection::{Create, Open};
    use super::System::{Macos, Windows};
    use super::*;

    fn index(kind: Custody) -> usize {
        match kind {
            RootFile => 0,
            Dpapi => 1,
            SecureEnclave => 2,
        }
    }

    #[test]
    fn the_table_is_total_and_takes_no_profile() {
        let kinds = [RootFile, Dpapi, SecureEnclave];
        assert_eq!(kinds.map(index), [0, 1, 2]);
        let table = [
            (false, Macos, None, Create(Some(RootFile))),
            (false, Macos, Some(RootFile), Open(ReadWrite)),
            (false, Macos, Some(Dpapi), Open(Refused)),
            (false, Macos, Some(SecureEnclave), Open(Refused)),
            (true, Macos, None, Create(None)),
            (true, Macos, Some(RootFile), Open(MaintenanceOnly)),
            (true, Macos, Some(Dpapi), Open(Refused)),
            (true, Macos, Some(SecureEnclave), Open(Refused)),
            (false, Windows, None, Create(Some(Dpapi))),
            (false, Windows, Some(RootFile), Open(Refused)),
            (false, Windows, Some(Dpapi), Open(ReadWrite)),
            (false, Windows, Some(SecureEnclave), Open(Refused)),
            (true, Windows, None, Create(Some(Dpapi))),
            (true, Windows, Some(RootFile), Open(Refused)),
            (true, Windows, Some(Dpapi), Open(ReadWrite)),
            (true, Windows, Some(SecureEnclave), Open(Refused)),
        ];
        for feature in [false, true] {
            for system in [Macos, Windows] {
                for recorded in [None, Some(RootFile), Some(Dpapi), Some(SecureEnclave)] {
                    let rows: Vec<_> = table
                        .iter()
                        .filter(|row| row.0 == feature && row.1 == system && row.2 == recorded)
                        .collect();
                    assert_eq!(rows.len(), 1, "{feature} {system:?} {recorded:?}");
                    assert_eq!(
                        select(feature, system, recorded),
                        rows[0].3,
                        "{feature} {system:?} {recorded:?}"
                    );
                }
            }
        }
        assert_eq!(table.len(), 2 * 2 * (kinds.len() + 1));
        assert_eq!(
            of_this_build(None),
            select(cfg!(feature = "secure-enclave"), System::this(), None)
        );
    }
}
