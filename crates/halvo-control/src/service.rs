//! How the app and the daemon start: the service arguments, the login item's
//! plist and the Windows start value.

use crate::model::APP_BUNDLE_ID;

/// The argument that starts the capture daemon (contracts.md C3.1).
pub const DAEMON_ARG: &str = "--service-run";

/// The argument of an app started at login, which shows no window (C14.7).
pub const BACKGROUND_ARG: &str = "--background";

/// The label of the app's login item (RD-08).
pub const LOGIN_ITEM_LABEL: &str = APP_BUNDLE_ID;

/// The argument that names the data directory of a start.
pub const DATA_DIR_ARG: &str = "--data-dir";

/// Why a start registration was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SpecError {
    #[error(
        "the label must be letters, digits, dots, hyphens and underscores, not starting with a dot"
    )]
    Label,
    #[error("the program path must be absolute")]
    RelativeProgram,
    #[error("the program path lies under an AppTranslocation directory")]
    Translocated,
    #[error("the path holds a character that the start registration cannot carry")]
    Text,
}

/// The app's login item (contracts.md C14.7): its label, the app executable and,
/// when the app was started with one, a data directory. KeepAlive is always
/// {SuccessfulExit: false}, so a quit is not restarted and a crash is.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginItemSpec {
    label: String,
    program: String,
    data_dir: Option<String>,
}

#[cfg(target_os = "macos")]
impl LoginItemSpec {
    pub fn new(
        label: &str,
        program: &std::path::Path,
        data_dir: Option<&crate::paths::DataDir>,
    ) -> Result<Self, SpecError> {
        let label_ok = !label.is_empty()
            && !label.starts_with('.')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
        if !label_ok {
            return Err(SpecError::Label);
        }
        if !program.is_absolute() {
            return Err(SpecError::RelativeProgram);
        }
        if program
            .components()
            .any(|component| component.as_os_str() == "AppTranslocation")
        {
            return Err(SpecError::Translocated);
        }
        let program = plist_text(program)?;
        let data_dir = data_dir.map(|dir| plist_text(dir.path())).transpose()?;
        Ok(Self {
            label: label.to_string(),
            program,
            data_dir,
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// The plist's file name in the LaunchAgents directory.
    pub fn file_name(&self) -> String {
        format!("{}.plist", self.label)
    }

    /// [app executable, --background], then --data-dir DIR when a data
    /// directory was given.
    pub fn program_arguments(&self) -> Vec<&str> {
        let mut arguments = vec![self.program.as_str(), BACKGROUND_ARG];
        if let Some(dir) = &self.data_dir {
            arguments.extend([DATA_DIR_ARG, dir.as_str()]);
        }
        arguments
    }

    /// The whole plist text.
    pub fn plist(&self) -> String {
        let arguments: String = self
            .program_arguments()
            .iter()
            .map(|argument| format!("\t\t<string>{}</string>\n", xml_escape(argument)))
            .collect();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
\t<key>Label</key>\n\
\t<string>{label}</string>\n\
\t<key>ProgramArguments</key>\n\
\t<array>\n\
{arguments}\
\t</array>\n\
\t<key>RunAtLoad</key>\n\
\t<true/>\n\
\t<key>LimitLoadToSessionType</key>\n\
\t<string>Aqua</string>\n\
\t<key>KeepAlive</key>\n\
\t<dict>\n\
\t\t<key>SuccessfulExit</key>\n\
\t\t<false/>\n\
\t</dict>\n\
</dict>\n\
</plist>\n",
            label = xml_escape(&self.label),
        )
    }
}

#[cfg(target_os = "macos")]
fn plist_text(path: &std::path::Path) -> Result<String, SpecError> {
    let text = path.to_str().ok_or(SpecError::Text)?;
    if text.chars().any(char::is_control) {
        return Err(SpecError::Text);
    }
    Ok(text.to_string())
}

#[cfg(target_os = "macos")]
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// What the plist on disk is against its spec: the same bytes with mode 0644
/// and the current user as owner, anything else, or no file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItemState {
    Current,
    Stale,
    Absent,
}

/// What an install did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Written,
    Unchanged,
}

/// What a remove did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveOutcome {
    Removed,
    Absent,
}

/// The plist file of `spec` in `dir`.
#[cfg(target_os = "macos")]
pub fn login_item_path(
    spec: &LoginItemSpec,
    dir: &crate::paths::LaunchAgentsDir,
) -> std::path::PathBuf {
    dir.path().join(spec.file_name())
}

/// Read the state of the plist of `spec` in `dir`.
#[cfg(target_os = "macos")]
pub fn login_item_state(
    spec: &LoginItemSpec,
    dir: &crate::paths::LaunchAgentsDir,
) -> std::io::Result<LoginItemState> {
    use std::os::unix::fs::MetadataExt;

    let path = login_item_path(spec, dir);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LoginItemState::Absent),
        Err(e) => return Err(e),
    };
    let owner = unsafe { libc::geteuid() };
    if !meta.is_file() || meta.mode() & 0o7777 != 0o644 || meta.uid() != owner {
        return Ok(LoginItemState::Stale);
    }
    if std::fs::read(&path)? == spec.plist().as_bytes() {
        Ok(LoginItemState::Current)
    } else {
        Ok(LoginItemState::Stale)
    }
}

/// Write the plist of `spec` into `dir` unless it is current: a temp file whose
/// name does not end in .plist, mode 0644, synced, renamed over the plist, then
/// the directory synced. A new LaunchAgents directory gets mode 0755. A failed
/// write leaves the old plist as it was.
#[cfg(target_os = "macos")]
pub fn install_login_item(
    spec: &LoginItemSpec,
    dir: &crate::paths::LaunchAgentsDir,
) -> std::io::Result<InstallOutcome> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    if login_item_state(spec, dir)? == LoginItemState::Current {
        return Ok(InstallOutcome::Unchanged);
    }
    if !dir.path().exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(dir.path())?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))?;
    }
    let path = login_item_path(spec, dir);
    let temp = dir
        .path()
        .join(format!(".{}.{}.tmp", spec.file_name(), std::process::id()));
    match std::fs::remove_file(&temp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    if let Err(e) = replace_with_temp(spec.plist().as_bytes(), &temp, &path, dir.path()) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(InstallOutcome::Written)
}

#[cfg(target_os = "macos")]
fn replace_with_temp(
    bytes: &[u8],
    temp: &std::path::Path,
    path: &std::path::Path,
    dir: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(temp)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(temp, path)?;
    std::fs::File::open(dir)?.sync_all()
}

/// Remove the plist of `spec` from `dir`.
#[cfg(target_os = "macos")]
pub fn remove_login_item(
    spec: &LoginItemSpec,
    dir: &crate::paths::LaunchAgentsDir,
) -> std::io::Result<RemoveOutcome> {
    match std::fs::remove_file(login_item_path(spec, dir)) {
        Ok(()) => Ok(RemoveOutcome::Removed),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RemoveOutcome::Absent),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_service_argument_has_one_spelling() {
        assert_eq!(DAEMON_ARG, "--service-run");
        assert_eq!(BACKGROUND_ARG, "--background");
        assert_eq!(LOGIN_ITEM_LABEL, "com.halvo.desktop");
    }
}

#[cfg(all(test, target_os = "macos"))]
mod login_item_tests {
    use std::ffi::OsString;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use super::*;
    use crate::paths::{AccountLookup, DataDir, LaunchAgentsDir, PathError};

    static PROCESS_STATE: Mutex<()> = Mutex::new(());

    const APP: &str = "/Applications/Halvo.app/Contents/MacOS/Halvo";

    const LOGIN_ITEM_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\">
<dict>
\t<key>Label</key>
\t<string>com.halvo.desktop</string>
\t<key>ProgramArguments</key>
\t<array>
\t\t<string>/Applications/Halvo.app/Contents/MacOS/Halvo</string>
\t\t<string>--background</string>
\t</array>
\t<key>RunAtLoad</key>
\t<true/>
\t<key>LimitLoadToSessionType</key>
\t<string>Aqua</string>
\t<key>KeepAlive</key>
\t<dict>
\t\t<key>SuccessfulExit</key>
\t\t<false/>
\t</dict>
</dict>
</plist>
";

    const CHECK_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\">
<dict>
\t<key>Label</key>
\t<string>com.halvo.check-l</string>
\t<key>ProgramArguments</key>
\t<array>
\t\t<string>/tmp/a &amp; b/Halvo.app/Contents/MacOS/Halvo</string>
\t\t<string>--background</string>
\t\t<string>--data-dir</string>
\t\t<string>/tmp/data &lt;1&gt;</string>
\t</array>
\t<key>RunAtLoad</key>
\t<true/>
\t<key>LimitLoadToSessionType</key>
\t<string>Aqua</string>
\t<key>KeepAlive</key>
\t<dict>
\t\t<key>SuccessfulExit</key>
\t\t<false/>
\t</dict>
</dict>
</plist>
";

    fn login_item() -> LoginItemSpec {
        LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new(APP), None).unwrap()
    }

    #[test]
    fn the_login_item_plist_is_this_whole_text() {
        let spec = login_item();
        assert_eq!(spec.plist(), LOGIN_ITEM_PLIST);
        assert_eq!(spec.program_arguments(), [APP, BACKGROUND_ARG]);
        assert_eq!(spec.file_name(), "com.halvo.desktop.plist");
    }

    #[test]
    fn a_label_and_a_data_directory_are_inputs_of_the_one_builder() {
        let data = DataDir::from_flag("/tmp/data <1>").unwrap();
        let spec = LoginItemSpec::new(
            "com.halvo.check-l",
            Path::new("/tmp/a & b/Halvo.app/Contents/MacOS/Halvo"),
            Some(&data),
        )
        .unwrap();
        assert_eq!(spec.plist(), CHECK_PLIST);
        assert_eq!(
            spec.program_arguments(),
            [
                "/tmp/a & b/Halvo.app/Contents/MacOS/Halvo",
                BACKGROUND_ARG,
                "--data-dir",
                "/tmp/data <1>"
            ]
        );
    }

    #[test]
    fn the_spec_refuses_a_relative_or_translocated_program_and_a_bad_label() {
        let translocated =
            "/private/var/folders/x/T/AppTranslocation/0A1B/d/Halvo.app/Contents/MacOS/Halvo";
        for program in ["Halvo.app/Contents/MacOS/Halvo", "", translocated] {
            assert!(
                LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new(program), None).is_err(),
                "{program}"
            );
        }
        assert!(LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new("/a\nb/Halvo"), None).is_err());
        for label in [
            "",
            ".",
            "..",
            "a/b",
            ".hidden",
            "a b",
            "com.halvo.desktop\n",
        ] {
            assert!(
                LoginItemSpec::new(label, Path::new(APP), None).is_err(),
                "{label:?}"
            );
        }
    }

    struct Home(PathBuf);

    impl AccountLookup for Home {
        fn base_dir(&self) -> Result<Option<OsString>, PathError> {
            Ok(Some(self.0.clone().into_os_string()))
        }

        fn user_temp_dir(&self) -> Result<Option<PathBuf>, PathError> {
            Ok(None)
        }
    }

    fn agents(home: &Path) -> LaunchAgentsDir {
        LaunchAgentsDir::for_account(&Home(home.to_path_buf())).unwrap()
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().mode() & 0o7777
    }

    #[test]
    fn an_install_writes_the_plist_once_and_reports_its_state() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        let spec = login_item();
        assert_eq!(
            login_item_state(&spec, &dir).unwrap(),
            LoginItemState::Absent
        );
        assert_eq!(
            install_login_item(&spec, &dir).unwrap(),
            InstallOutcome::Written
        );
        let path = login_item_path(&spec, &dir);
        assert_eq!(
            path,
            home.path()
                .join("Library/LaunchAgents/com.halvo.desktop.plist")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LOGIN_ITEM_PLIST);
        assert_eq!(mode(&path), 0o644);
        assert_eq!(std::fs::metadata(&path).unwrap().uid(), unsafe {
            libc::geteuid()
        });
        assert_eq!(
            login_item_state(&spec, &dir).unwrap(),
            LoginItemState::Current
        );
        assert_eq!(
            install_login_item(&spec, &dir).unwrap(),
            InstallOutcome::Unchanged
        );
        let names: Vec<OsString> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [OsString::from("com.halvo.desktop.plist")]);
        let other = LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new("/Other/Halvo"), None).unwrap();
        assert_eq!(
            login_item_state(&other, &dir).unwrap(),
            LoginItemState::Stale
        );
        assert_eq!(
            remove_login_item(&spec, &dir).unwrap(),
            RemoveOutcome::Removed
        );
        assert_eq!(
            login_item_state(&spec, &dir).unwrap(),
            LoginItemState::Absent
        );
        assert_eq!(
            remove_login_item(&spec, &dir).unwrap(),
            RemoveOutcome::Absent
        );
    }

    #[test]
    fn a_failed_rewrite_keeps_the_old_bytes() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        install_login_item(&login_item(), &dir).unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let other = LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new("/Other/Halvo"), None).unwrap();
        let result = install_login_item(&other, &dir);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        let path = login_item_path(&other, &dir);
        assert_eq!(std::fs::read_to_string(path).unwrap(), LOGIN_ITEM_PLIST);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn umask_000_gives_a_0644_plist_in_a_new_0755_directory() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        let before = unsafe { libc::umask(0) };
        let result = install_login_item(&login_item(), &dir);
        unsafe { libc::umask(before) };
        assert_eq!(result.unwrap(), InstallOutcome::Written);
        assert_eq!(mode(dir.path()), 0o755);
        assert_eq!(mode(&login_item_path(&login_item(), &dir)), 0o644);
    }

    #[test]
    fn a_0666_file_with_the_same_content_is_repaired() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        let spec = login_item();
        install_login_item(&spec, &dir).unwrap();
        let path = login_item_path(&spec, &dir);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert_eq!(
            login_item_state(&spec, &dir).unwrap(),
            LoginItemState::Stale
        );
        assert_eq!(
            install_login_item(&spec, &dir).unwrap(),
            InstallOutcome::Written
        );
        assert_eq!(mode(&path), 0o644);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LOGIN_ITEM_PLIST);
        assert_eq!(
            login_item_state(&spec, &dir).unwrap(),
            LoginItemState::Current
        );
    }

    #[test]
    fn an_empty_or_relative_home_gives_no_launch_agents_directory() {
        for home in ["", "Users/a"] {
            assert!(LaunchAgentsDir::for_account(&Home(PathBuf::from(home))).is_err());
        }
    }
}
