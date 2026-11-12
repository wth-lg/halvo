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

/// The key of the per-user start at logon, under HKEY_CURRENT_USER (RD-43).
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The name of Halvo's value under the Run key.
pub const RUN_VALUE_NAME: &str = "Halvo";

/// The Windows start registration (contracts.md C14.7): a value under a key of
/// HKEY_CURRENT_USER whose data is the quoted absolute path of the app
/// executable, a space and --background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartValueSpec {
    key: String,
    value_name: String,
    program: String,
}

impl StartValueSpec {
    /// The value `Halvo` under the Run key for the app executable at `program`.
    pub fn new(program: &std::path::Path) -> Result<Self, SpecError> {
        Self::with_key(RUN_KEY, RUN_VALUE_NAME, program)
    }

    /// The same value under another key and name, for a check that must not
    /// write the real one.
    pub fn with_key(
        key: &str,
        value_name: &str,
        program: &std::path::Path,
    ) -> Result<Self, SpecError> {
        let program = program.to_str().ok_or(SpecError::Text)?;
        if !is_windows_absolute(program) {
            return Err(SpecError::RelativeProgram);
        }
        if program.contains('"') || program.chars().any(char::is_control) {
            return Err(SpecError::Text);
        }
        if key.is_empty() || value_name.is_empty() {
            return Err(SpecError::Label);
        }
        Ok(Self {
            key: key.to_string(),
            value_name: value_name.to_string(),
            program: program.to_string(),
        })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value_name(&self) -> &str {
        &self.value_name
    }

    /// The value's data: the quoted program, a space and --background.
    pub fn data(&self) -> String {
        format!("\"{}\" {BACKGROUND_ARG}", self.program)
    }
}

fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive =
        bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    let unc = path
        .strip_prefix(r"\\")
        .and_then(|rest| rest.split_once('\\'))
        .is_some_and(|(server, rest)| {
            !server.is_empty()
                && rest
                    .split_once('\\')
                    .is_some_and(|(share, _)| !share.is_empty())
        });
    drive || unc
}

#[cfg(windows)]
mod registry {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    };

    use super::{InstallOutcome, RemoveOutcome, StartValueSpec};

    pub fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn os_error(code: WIN32_ERROR) -> std::io::Error {
        std::io::Error::from_raw_os_error(code.0 as i32)
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            let _ = unsafe { RegCloseKey(self.0) };
        }
    }

    pub fn read(spec: &StartValueSpec) -> std::io::Result<Option<String>> {
        let (key, name) = (wide(spec.key()), wide(spec.value_name()));
        let mut size: u32 = 0;
        let found = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            )
        };
        if found == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if found != ERROR_SUCCESS {
            return Err(os_error(found));
        }
        let mut data = vec![0u16; (size as usize).div_ceil(2)];
        let mut size = u32::try_from(data.len() * 2).unwrap_or(u32::MAX);
        let read = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(data.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if read != ERROR_SUCCESS {
            return Err(os_error(read));
        }
        data.truncate(size as usize / 2);
        while data.last() == Some(&0) {
            data.pop();
        }
        String::from_utf16(&data)
            .map(Some)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))
    }

    pub fn write(spec: &StartValueSpec) -> std::io::Result<InstallOutcome> {
        let data = spec.data();
        if read(spec)?.as_deref() == Some(data.as_str()) {
            return Ok(InstallOutcome::Unchanged);
        }
        let (key, name) = (wide(spec.key()), wide(spec.value_name()));
        let mut handle = HKEY::default();
        let created = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut handle,
                None,
            )
        };
        if created != ERROR_SUCCESS {
            return Err(os_error(created));
        }
        let handle = Key(handle);
        let bytes: Vec<u8> = wide(&data)
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        let set =
            unsafe { RegSetValueExW(handle.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)) };
        if set != ERROR_SUCCESS {
            return Err(os_error(set));
        }
        Ok(InstallOutcome::Written)
    }

    pub fn remove(spec: &StartValueSpec) -> std::io::Result<RemoveOutcome> {
        let (key, name) = (wide(spec.key()), wide(spec.value_name()));
        let deleted = unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
            )
        };
        if deleted == ERROR_FILE_NOT_FOUND {
            Ok(RemoveOutcome::Absent)
        } else if deleted == ERROR_SUCCESS {
            Ok(RemoveOutcome::Removed)
        } else {
            Err(os_error(deleted))
        }
    }
}

/// Read the start value of `spec`: `None` only when the key or the value does
/// not exist; any other failure is an error.
#[cfg(windows)]
pub fn read_start_value(spec: &StartValueSpec) -> std::io::Result<Option<String>> {
    registry::read(spec)
}

/// Write the start value of `spec` when the stored string differs.
#[cfg(windows)]
pub fn write_start_value(spec: &StartValueSpec) -> std::io::Result<InstallOutcome> {
    registry::write(spec)
}

/// Remove the start value of `spec`.
#[cfg(windows)]
pub fn remove_start_value(spec: &StartValueSpec) -> std::io::Result<RemoveOutcome> {
    registry::remove(spec)
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

#[cfg(test)]
mod start_value_tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn the_start_value_is_the_quoted_program_and_background_under_the_run_key() {
        assert_eq!(RUN_KEY, r"Software\Microsoft\Windows\CurrentVersion\Run");
        assert_eq!(RUN_VALUE_NAME, "Halvo");
        let spec =
            StartValueSpec::new(Path::new(r"C:\Users\a b\AppData\Local\Halvo\halvo.exe")).unwrap();
        assert_eq!(spec.key(), RUN_KEY);
        assert_eq!(spec.value_name(), "Halvo");
        assert_eq!(
            spec.data(),
            r#""C:\Users\a b\AppData\Local\Halvo\halvo.exe" --background"#
        );
        let unc = StartValueSpec::new(Path::new(r"\\server\share\Halvo\halvo.exe")).unwrap();
        assert_eq!(
            unc.data(),
            r#""\\server\share\Halvo\halvo.exe" --background"#
        );
    }

    #[test]
    fn a_relative_program_or_one_with_a_quote_is_an_error() {
        for relative in [
            "halvo.exe",
            r"Halvo\halvo.exe",
            r"\Halvo\halvo.exe",
            r"C:halvo.exe",
            r"\\server",
            "",
        ] {
            assert_eq!(
                StartValueSpec::new(Path::new(relative)),
                Err(SpecError::RelativeProgram),
                "{relative}"
            );
        }
        for bad in [r#"C:\a"b\halvo.exe"#, "C:\\a\nb\\halvo.exe"] {
            assert_eq!(
                StartValueSpec::new(Path::new(bad)),
                Err(SpecError::Text),
                "{bad}"
            );
        }
    }
}

#[cfg(all(test, windows))]
mod start_value_registry_tests {
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegDeleteTreeW, HKEY_CURRENT_USER};

    use super::*;

    struct ScratchKey(String);

    impl Drop for ScratchKey {
        fn drop(&mut self) {
            let key = registry::wide(&self.0);
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) };
        }
    }

    #[test]
    fn the_start_value_is_written_once_read_back_and_removed_under_a_scratch_key() {
        let scratch = ScratchKey(format!(r"Software\Halvo-Test-{}", std::process::id()));
        let program = Path::new(r"C:\Users\a b\AppData\Local\Halvo\halvo.exe");
        let spec = StartValueSpec::with_key(&scratch.0, RUN_VALUE_NAME, program).unwrap();
        assert_eq!(read_start_value(&spec).unwrap(), None);
        assert_eq!(write_start_value(&spec).unwrap(), InstallOutcome::Written);
        assert_eq!(read_start_value(&spec).unwrap(), Some(spec.data()));
        assert_eq!(write_start_value(&spec).unwrap(), InstallOutcome::Unchanged);
        let other =
            StartValueSpec::with_key(&scratch.0, RUN_VALUE_NAME, Path::new(r"D:\Halvo\halvo.exe"))
                .unwrap();
        assert_eq!(write_start_value(&other).unwrap(), InstallOutcome::Written);
        assert_eq!(read_start_value(&spec).unwrap(), Some(other.data()));
        assert_eq!(remove_start_value(&spec).unwrap(), RemoveOutcome::Removed);
        assert_eq!(read_start_value(&spec).unwrap(), None);
        assert_eq!(remove_start_value(&spec).unwrap(), RemoveOutcome::Absent);
    }
}
