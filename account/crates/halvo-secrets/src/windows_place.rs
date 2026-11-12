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

pub(crate) fn is_absent(code: Option<i32>) -> bool {
    matches!(code, Some(FILE_NOT_FOUND | PATH_NOT_FOUND))
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
    fn only_a_file_or_a_path_that_is_not_found_is_absent() {
        assert!(is_absent(Some(2)));
        assert!(is_absent(Some(3)));
        for code in [
            None,
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
}
