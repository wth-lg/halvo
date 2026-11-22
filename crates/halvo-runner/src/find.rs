//! The person's browser, found at fixed places of each system (the coordinator's ruling of 2026-11-22, U9.10): a run
//! given no `--browser` takes the first place of a closed list at which a file stands. The runner asks the system
//! nothing: no registry, no index of programs and no `PATH`. A Mac: Google Chrome, then Microsoft Edge, under
//! `/Applications` alone. Windows: Microsoft Edge, then Google Chrome, under the folders that three variables of the
//! runner's environment name. No other browser.

use std::path::{Path, PathBuf};

/// The folders under which the places of a browser stand, each none where it is not known: the roots of the system
/// that the runner was built for (`Roots::of_this_system`), or roots that a test makes up. The list of a Mac stands
/// under `applications`, the list of Windows under the other three; both lists build on both systems.
#[derive(Clone, Debug, Default)]
pub(crate) struct Roots {
    /// A Mac's applications folder.
    pub(crate) applications: Option<PathBuf>,
    /// The folder that the Windows variable `ProgramFiles(x86)` names.
    pub(crate) program_files_x86: Option<PathBuf>,
    /// The folder that the Windows variable `ProgramFiles` names.
    pub(crate) program_files: Option<PathBuf>,
    /// The folder that the Windows variable `LOCALAPPDATA` names.
    pub(crate) local_app_data: Option<PathBuf>,
}

/// The place of a program under `root`, by its parts.
fn under(root: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

const CHROME_MAC: &[&str] = &["Google Chrome.app", "Contents", "MacOS", "Google Chrome"];
const EDGE_MAC: &[&str] = &["Microsoft Edge.app", "Contents", "MacOS", "Microsoft Edge"];
const EDGE_WINDOWS: &[&str] = &["Microsoft", "Edge", "Application", "msedge.exe"];
const CHROME_WINDOWS: &[&str] = &["Google", "Chrome", "Application", "chrome.exe"];

impl Roots {
    /// The roots of this system: on Windows the folders that the variables `ProgramFiles(x86)`, `ProgramFiles` and
    /// `LOCALAPPDATA` of the runner's environment name, each none where the variable is not set or names no absolute
    /// path; elsewhere `/Applications`.
    #[cfg_attr(
        test,
        expect(
            dead_code,
            reason = "a unit test's runner has no roots unless the test gives some"
        )
    )]
    pub(crate) fn of_this_system() -> Roots {
        #[cfg(windows)]
        {
            let root = |name: &str| {
                std::env::var_os(name)
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
            };
            Roots {
                applications: None,
                program_files_x86: root("ProgramFiles(x86)"),
                program_files: root("ProgramFiles"),
                local_app_data: root("LOCALAPPDATA"),
            }
        }
        #[cfg(not(windows))]
        {
            Roots {
                applications: Some(PathBuf::from("/Applications")),
                ..Roots::default()
            }
        }
    }

    /// The closed list of places, in the order in which `find` reads them: Google Chrome, then Microsoft Edge, under
    /// `applications`; then Microsoft Edge under `program_files_x86` and `program_files`, then Google Chrome under
    /// `program_files`, `program_files_x86` and `local_app_data`.
    fn places(&self) -> Vec<PathBuf> {
        let mac = self
            .applications
            .iter()
            .flat_map(|root| [under(root, CHROME_MAC), under(root, EDGE_MAC)]);
        let edge = [&self.program_files_x86, &self.program_files]
            .into_iter()
            .flatten()
            .map(|root| under(root, EDGE_WINDOWS));
        let chrome = [
            &self.program_files,
            &self.program_files_x86,
            &self.local_app_data,
        ]
        .into_iter()
        .flatten()
        .map(|root| under(root, CHROME_WINDOWS));
        mac.chain(edge).chain(chrome).collect()
    }
}

/// The first place of the list of `roots` at which a file stands; none when no file stands at any of them.
pub(crate) fn find(roots: &Roots) -> Option<PathBuf> {
    roots.places().into_iter().find(|place| place.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes an empty file at `place`, with the folders above it.
    fn file_at(place: &Path) {
        std::fs::create_dir_all(place.parent().expect("a folder above the place"))
            .expect("make the folders");
        std::fs::write(place, b"").expect("write the file");
    }

    /// A folder of its own under the target's temporary directory.
    fn scratch(tag: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("the test binary");
        let target = exe
            .ancestors()
            .nth(3)
            .expect("the target directory above deps");
        halvo_test_dir::make(&target.join("tmp").join("halvo-runner-unit"), tag)
            .expect("make the scratch directory")
    }

    /// U9.10 (1): of each list the first place at which a file stands wins, a folder at a place is passed over, the
    /// root of a variable that is not set is passed over, and a list with no file finds none. The files are written
    /// in the reverse of the list's order, so that each new file becomes the one found.
    #[test]
    fn the_first_place_with_a_file_wins() {
        let dir = scratch("find-mac");
        let mac = Roots {
            applications: Some(dir.clone()),
            ..Roots::default()
        };
        assert_eq!(find(&mac), None, "an empty folder");
        let edge = under(&dir, EDGE_MAC);
        let chrome = under(&dir, CHROME_MAC);
        std::fs::create_dir_all(&chrome).expect("a folder at Chrome's place");
        assert_eq!(find(&mac), None, "a folder at a place is no file");
        file_at(&edge);
        assert_eq!(find(&mac), Some(edge), "Edge alone");
        std::fs::remove_dir(&chrome).expect("remove the folder");
        file_at(&chrome);
        assert_eq!(find(&mac), Some(chrome), "Chrome before Edge");

        let dir = scratch("find-windows");
        let (x86, files, local) = (dir.join("x86"), dir.join("files"), dir.join("local"));
        let windows = Roots {
            applications: None,
            program_files_x86: Some(x86.clone()),
            program_files: Some(files.clone()),
            local_app_data: Some(local.clone()),
        };
        assert_eq!(find(&windows), None, "three empty folders");
        let order = [
            under(&x86, EDGE_WINDOWS),
            under(&files, EDGE_WINDOWS),
            under(&files, CHROME_WINDOWS),
            under(&x86, CHROME_WINDOWS),
            under(&local, CHROME_WINDOWS),
        ];
        for place in order.iter().rev() {
            file_at(place);
            assert_eq!(find(&windows).as_ref(), Some(place), "{}", place.display());
        }
        let unset = Roots {
            local_app_data: Some(local.clone()),
            ..Roots::default()
        };
        assert_eq!(
            find(&unset),
            Some(under(&local, CHROME_WINDOWS)),
            "the roots of variables that are not set are passed over"
        );
        assert_eq!(find(&Roots::default()), None, "no root");
    }
}
