pub(crate) const SYSTEM: &str = "S-1-5-18";
pub(crate) const ADMINISTRATORS: &str = "S-1-5-32-544";
pub(crate) const FILE_NOT_FOUND: i32 = 2;
pub(crate) const PATH_NOT_FOUND: i32 = 3;

pub(crate) fn store_dir_sddl(user: &str) -> String {
    format!("D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;{SYSTEM})(A;OICI;FA;;;{ADMINISTRATORS})")
}

pub(crate) fn owner_is_trusted(owner: &str, user: &str) -> bool {
    owner == user || owner == SYSTEM || owner == ADMINISTRATORS
}

/// Whether a failed open of the store directory or of the item says that the entry is
/// absent: ERROR_FILE_NOT_FOUND alone, read from the system's own code. ERROR_PATH_NOT_FOUND
/// says that a step above is missing, and the codes that the standard library folds into
/// NotFound with it (an invalid drive, a network path or name that is not found, a drive
/// that is not ready) are a place that is away, so each of them answers a fault.
pub(crate) fn is_absent(code: Option<i32>) -> bool {
    code == Some(FILE_NOT_FOUND)
}

/// What a walk answers for a store directory whose open, following a reparse point, failed
/// with `code`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gone {
    /// The store directory is absent from a parent that is a directory.
    Absent,
    /// A step above the store directory is missing (ERROR_PATH_NOT_FOUND).
    Missing,
    /// A fault with this code, never `NotStored`.
    Fault(i32),
    /// The failure answers as its own code does.
    AsIs,
}

/// `entry` looks at the store directory without following a reparse point: `Ok` when an
/// entry stands there, so that what it points to is gone, or the code of its failure.
/// `parent` gives whether the parent is a directory, or the code of its failure. Each is
/// asked only when the answer still depends on it.
pub(crate) fn store_dir_gone(
    code: Option<i32>,
    entry: impl FnOnce() -> Result<(), Option<i32>>,
    parent: impl FnOnce() -> Result<bool, Option<i32>>,
) -> Gone {
    if code == Some(PATH_NOT_FOUND) {
        return Gone::Missing;
    }
    if !is_absent(code) {
        return Gone::AsIs;
    }
    match entry() {
        Ok(()) => return Gone::Fault(FILE_NOT_FOUND),
        Err(failed) if is_absent(failed) => {}
        Err(failed) => return Gone::Fault(failed.unwrap_or(-1)),
    }
    match parent() {
        Ok(true) => Gone::Absent,
        Ok(false) => Gone::Fault(PATH_NOT_FOUND),
        Err(code) => Gone::Fault(code.unwrap_or(-1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "S-1-5-21-1-2-3-1001";

    #[test]
    fn the_names_are_the_well_known_sids_and_codes_of_windows() {
        assert_eq!([SYSTEM, ADMINISTRATORS], ["S-1-5-18", "S-1-5-32-544"]);
        assert_eq!([FILE_NOT_FOUND, PATH_NOT_FOUND], [2, 3]);
    }

    #[test]
    fn the_store_directory_is_protected_and_names_the_person_system_and_administrators() {
        assert_eq!(
            store_dir_sddl(USER),
            "D:P(A;OICI;FA;;;S-1-5-21-1-2-3-1001)(A;OICI;FA;;;S-1-5-18)(A;OICI;FA;;;S-1-5-32-544)"
        );
    }

    #[test]
    fn the_owner_is_the_person_system_or_administrators() {
        for owner in [USER, SYSTEM, ADMINISTRATORS] {
            assert!(owner_is_trusted(owner, USER), "{owner}");
        }
        for owner in [
            "S-1-5-21-1-2-3-1002",
            "S-1-5-32-545",
            "S-1-1-0",
            "S-1-5-11",
            "S-1-5-20",
            "",
        ] {
            assert!(!owner_is_trusted(owner, USER), "{owner}");
        }
    }

    #[test]
    fn only_a_file_that_is_not_found_is_absent() {
        assert!(is_absent(Some(2)));
        for code in [
            None,
            Some(3),
            Some(5),
            Some(15),
            Some(21),
            Some(53),
            Some(67),
            Some(123),
            Some(0),
        ] {
            assert!(!is_absent(code), "{code:?}");
        }
    }

    fn unasked<T>() -> T {
        panic!("asked although the answer no longer depends on it")
    }

    #[test]
    fn a_store_directory_that_is_not_found_under_a_parent_directory_is_absent() {
        assert_eq!(
            store_dir_gone(Some(2), || Err(Some(2)), || Ok(true)),
            Gone::Absent
        );
    }

    #[test]
    fn a_path_that_is_not_found_is_a_missing_step_above_the_store_directory() {
        assert_eq!(store_dir_gone(Some(3), unasked, unasked), Gone::Missing);
    }

    #[test]
    fn a_place_that_is_away_or_refused_answers_as_its_code() {
        for code in [None, Some(5), Some(15), Some(21), Some(53), Some(67)] {
            assert_eq!(
                store_dir_gone(code, unasked, unasked),
                Gone::AsIs,
                "{code:?}"
            );
        }
    }

    #[test]
    fn a_reparse_point_whose_target_is_gone_answers_a_fault() {
        assert_eq!(store_dir_gone(Some(2), || Ok(()), unasked), Gone::Fault(2));
    }

    #[test]
    fn a_failed_look_at_the_entry_or_the_parent_answers_a_fault_with_its_code() {
        assert_eq!(
            store_dir_gone(Some(2), || Err(Some(5)), unasked),
            Gone::Fault(5)
        );
        assert_eq!(
            store_dir_gone(Some(2), || Err(Some(3)), unasked),
            Gone::Fault(3)
        );
        assert_eq!(
            store_dir_gone(Some(2), || Err(None), unasked),
            Gone::Fault(-1)
        );
        for parent in [Err(Some(3)), Err(Some(2)), Err(Some(53))] {
            let code = parent.unwrap_err().unwrap_or(-1);
            assert_eq!(
                store_dir_gone(Some(2), || Err(Some(2)), move || parent),
                Gone::Fault(code)
            );
        }
        assert_eq!(
            store_dir_gone(Some(2), || Err(Some(2)), || Ok(false)),
            Gone::Fault(3)
        );
    }
}
