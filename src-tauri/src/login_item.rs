use std::path::Path;

use halvo_control::paths::{DataDir, LoginItemDir};
use halvo_control::service::{install_login_item, InstallOutcome, LoginItemSpec, SpecError};

/// The sentence that a person reads where Halvo runs under an AppTranslocation path, and so
/// writes no login item.
pub const MOVE_TO_APPLICATIONS: &str = "Halvo cannot start at login while macOS runs it from a temporary location. Move Halvo to your Applications folder, open it from there, and try again.";

/// Why the manager writes no login item for a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Translocated,
    Spec(SpecError),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Translocated => f.write_str(MOVE_TO_APPLICATIONS),
            Refusal::Spec(error) => error.fmt(f),
        }
    }
}

/// The app's one manager of its login item. It takes the label, the program, the login item's
/// directory and, for a scratch run, the data directory as values that main resolves, and
/// writes through the one writer of halvo-control; a write starts no process.
#[derive(Debug)]
pub struct LoginItem {
    spec: LoginItemSpec,
    dir: LoginItemDir,
}

impl LoginItem {
    /// Refuses a program under an AppTranslocation path, before any read or write of the login
    /// item.
    pub fn new(
        label: &str,
        program: &Path,
        dir: LoginItemDir,
        data_dir: Option<&DataDir>,
    ) -> Result<Self, Refusal> {
        let spec = LoginItemSpec::new(label, program, data_dir).map_err(|error| match error {
            SpecError::Translocated => Refusal::Translocated,
            other => Refusal::Spec(other),
        })?;
        Ok(Self { spec, dir })
    }

    /// Writes the plist through `install_login_item`, atomically with mode 0644.
    pub fn write(&self) -> std::io::Result<InstallOutcome> {
        install_login_item(&self.spec, &self.dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halvo_control::paths::{AccountLookup, DarwinUserTempDir, PathError};
    use halvo_control::service::LOGIN_ITEM_LABEL;
    use std::ffi::OsString;
    use std::path::PathBuf;

    const PROGRAM: &str = "/Applications/Halvo.app/Contents/MacOS/halvo";
    const TRANSLOCATED: &str =
        "/private/var/folders/zz/T/AppTranslocation/0A1B2C3D/d/Halvo.app/Contents/MacOS/halvo";

    struct Home(PathBuf);

    impl AccountLookup for Home {
        fn base_dir(&self) -> Result<Option<OsString>, PathError> {
            Ok(Some(self.0.clone().into_os_string()))
        }

        fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError> {
            Ok(None)
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::current_exe()
            .expect("the test binary has a path")
            .ancestors()
            .nth(3)
            .expect("the test binary sits under target/<profile>/deps")
            .join("tmp");
        halvo_test_dir::make(&base, name).expect("a scratch directory")
    }

    fn entries(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                found.extend(entries(&path));
            }
            found.push(path);
        }
        found.sort();
        found
    }

    fn agents_of(home: &Path) -> LoginItemDir {
        LoginItemDir::for_account(&Home(home.to_path_buf()))
            .expect("the scratch home's LaunchAgents")
    }

    fn plist_of(program: &str) -> String {
        LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new(program), None)
            .expect("a spec")
            .plist()
    }

    #[test]
    fn the_manager_writes_through_the_one_writer_and_refuses_a_translocated_path() {
        let home = scratch("login-item-home");
        let item = LoginItem::new(LOGIN_ITEM_LABEL, Path::new(PROGRAM), agents_of(&home), None)
            .expect("a manager");
        assert_eq!(item.write().expect("the write"), InstallOutcome::Written);
        let plist = home.join("Library/LaunchAgents/com.halvo.desktop.plist");
        assert_eq!(
            std::fs::read_to_string(&plist).expect("the plist stands"),
            plist_of(PROGRAM)
        );

        let other = scratch("login-item-translocated");
        let refused = LoginItem::new(
            LOGIN_ITEM_LABEL,
            Path::new(TRANSLOCATED),
            agents_of(&other),
            None,
        )
        .expect_err("a refusal");
        assert_eq!(refused, Refusal::Translocated);
        assert_eq!(refused.to_string(), MOVE_TO_APPLICATIONS);
        assert_eq!(entries(&other), Vec::<PathBuf>::new());
    }
}
