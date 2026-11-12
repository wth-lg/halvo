use crate::model::APP_BUNDLE_ID;

pub const DAEMON_ARG: &str = "--service-run";

pub const BACKGROUND_ARG: &str = "--background";

pub const LOGIN_ITEM_LABEL: &str = APP_BUNDLE_ID;

pub const CAPTURE_AGENT_LABEL: &str = "com.halvo.capture";

pub const DATA_DIR_ARG: &str = "--data-dir";

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
        let capture_agent = label
            .strip_prefix(CAPTURE_AGENT_LABEL)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'));
        let label_ok = !label.is_empty()
            && !capture_agent
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

    pub fn file_name(&self) -> String {
        format!("{}.plist", self.label)
    }

    pub fn program_arguments(&self) -> Vec<&str> {
        let mut arguments = vec![self.program.as_str(), BACKGROUND_ARG];
        if let Some(dir) = &self.data_dir {
            arguments.extend([DATA_DIR_ARG, dir.as_str()]);
        }
        arguments
    }

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
{keep_alive}\
</dict>\n\
</plist>\n",
            label = xml_escape(&self.label),
            keep_alive = KEEP_ALIVE_RESTART_AFTER_FAILURE_ONLY,
        )
    }
}

#[cfg(target_os = "macos")]
const KEEP_ALIVE_RESTART_AFTER_FAILURE_ONLY: &str = "\t<key>KeepAlive</key>
\t<dict>
\t\t<key>SuccessfulExit</key>
\t\t<false/>
\t</dict>
";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItemState {
    Current,
    Stale,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Written,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveOutcome {
    Removed,
    Absent,
}

#[cfg(target_os = "macos")]
pub fn login_item_path(
    spec: &LoginItemSpec,
    dir: &crate::paths::LoginItemDir,
) -> std::path::PathBuf {
    dir.path().join(spec.file_name())
}

#[cfg(target_os = "macos")]
pub fn login_item_state(
    spec: &LoginItemSpec,
    dir: &crate::paths::LoginItemDir,
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

#[cfg(target_os = "macos")]
pub fn install_login_item(
    spec: &LoginItemSpec,
    dir: &crate::paths::LoginItemDir,
) -> std::io::Result<InstallOutcome> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    if login_item_state(spec, dir)? == LoginItemState::Current {
        return Ok(InstallOutcome::Unchanged);
    }
    if dir.create_missing() && !dir.path().exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(dir.path())?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))?;
    }
    let path = login_item_path(spec, dir);
    let temp = dir.path().join(temp_file_name(spec));
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
fn temp_file_name(spec: &LoginItemSpec) -> String {
    format!(".{}.{}.tmp", spec.file_name(), std::process::id())
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

#[cfg(target_os = "macos")]
pub fn remove_login_item(
    spec: &LoginItemSpec,
    dir: &crate::paths::LoginItemDir,
) -> std::io::Result<RemoveOutcome> {
    match std::fs::remove_file(login_item_path(spec, dir)) {
        Ok(()) => Ok(RemoveOutcome::Removed),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RemoveOutcome::Absent),
        Err(e) => Err(e),
    }
}

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub const RUN_VALUE_NAME: &str = "Halvo";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartValueSpec {
    key: String,
    value_name: String,
    program: String,
}

impl StartValueSpec {
    pub fn new(program: &std::path::Path) -> Result<Self, SpecError> {
        Self::under(RUN_KEY, RUN_VALUE_NAME, program)
    }

    #[cfg(test)]
    pub fn with_key(
        key: &str,
        value_name: &str,
        program: &std::path::Path,
    ) -> Result<Self, SpecError> {
        Self::under(key, value_name, program)
    }

    fn under(key: &str, value_name: &str, program: &std::path::Path) -> Result<Self, SpecError> {
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

#[cfg(windows)]
pub fn read_start_value(spec: &StartValueSpec) -> std::io::Result<Option<String>> {
    registry::read(spec)
}

#[cfg(windows)]
pub fn write_start_value(spec: &StartValueSpec) -> std::io::Result<InstallOutcome> {
    registry::write(spec)
}

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
    use crate::paths::{
        AccountLookup, DarwinUserTempDir, DataDir, LoginItemDir, PathError, ScratchDir,
    };

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

        fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError> {
            Ok(None)
        }
    }

    fn agents(home: &Path) -> LoginItemDir {
        LoginItemDir::for_account(&Home(home.to_path_buf())).unwrap()
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
    fn the_scratch_form_writes_label_plist_into_the_given_directory_through_the_same_writer() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        let user = DataDir::for_account(&Home(PathBuf::from("/Users/a b"))).unwrap();
        let scratch = ScratchDir::new(root.path(), &user).unwrap();
        let target =
            LoginItemDir::scratch_form(&scratch, &Home(PathBuf::from("/Users/a b"))).unwrap();
        let data = DataDir::from_flag(root.path()).unwrap();
        let spec = LoginItemSpec::new("com.halvo.check-l", Path::new(APP), Some(&data)).unwrap();
        assert_eq!(
            install_login_item(&spec, &target).unwrap(),
            InstallOutcome::Written
        );
        let path = login_item_path(&spec, &target);
        assert_eq!(path, root.path().join("com.halvo.check-l.plist"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), spec.plist());
        assert!(spec
            .plist()
            .contains("<key>SuccessfulExit</key>\n\t\t<false/>"));
        assert_eq!(mode(&path), 0o644);
        assert!(!root.path().join("Library").exists());
        assert_eq!(
            login_item_state(&spec, &target).unwrap(),
            LoginItemState::Current
        );
        let user_dir = std::fs::read_dir(root.path()).unwrap().count();
        assert_eq!(user_dir, 1);
        let gone = ScratchDir::new(root.path().join("missing"), &user).unwrap();
        let gone_dir =
            LoginItemDir::scratch_form(&gone, &Home(PathBuf::from("/Users/a b"))).unwrap();
        assert!(install_login_item(&spec, &gone_dir).is_err());
    }

    #[test]
    fn the_scratch_form_refuses_the_account_launch_agents_directory_and_anything_inside_it() {
        let home = tempfile::tempdir().unwrap();
        let account = Home(home.path().to_path_buf());
        let agents = LoginItemDir::for_account(&account).unwrap();
        std::fs::create_dir_all(agents.path()).unwrap();
        let user = DataDir::for_account(&account).unwrap();
        let link = home.path().join("link");
        std::os::unix::fs::symlink(agents.path(), &link).unwrap();
        let upper = PathBuf::from(agents.path().to_str().unwrap().to_uppercase());
        for inside in [
            agents.path().to_path_buf(),
            agents.path().join("check"),
            upper.join("check"),
            link.join("check"),
        ] {
            let scratch = ScratchDir::new(&inside, &user).unwrap();
            assert_eq!(
                LoginItemDir::scratch_form(&scratch, &account),
                Err(PathError::InsideLaunchAgents),
                "{}",
                inside.display()
            );
        }
        let outside = ScratchDir::new(home.path().join("scratch"), &user).unwrap();
        assert_eq!(
            LoginItemDir::scratch_form(&outside, &account)
                .unwrap()
                .path(),
            outside.path()
        );
    }

    #[test]
    fn the_temp_file_of_an_install_is_hidden_and_never_ends_in_plist() {
        for label in [LOGIN_ITEM_LABEL, "com.halvo.check-l"] {
            let spec = LoginItemSpec::new(label, Path::new(APP), None).unwrap();
            let name = temp_file_name(&spec);
            assert!(name.starts_with('.'), "{name}");
            assert!(!name.ends_with(".plist"), "{name}");
            assert_ne!(name, spec.file_name());
        }
    }

    #[test]
    fn the_builder_refuses_the_capture_agent_label() {
        assert_eq!(CAPTURE_AGENT_LABEL, "com.halvo.capture");
        assert_eq!(crate::paths::FALLBACK_SOCKET_DIR, CAPTURE_AGENT_LABEL);
        for label in ["com.halvo.capture", "com.halvo.capture.test"] {
            assert_eq!(
                LoginItemSpec::new(label, Path::new(APP), None),
                Err(SpecError::Label),
                "{label}"
            );
        }
        assert!(LoginItemSpec::new("com.halvo.capturing-check", Path::new(APP), None).is_ok());
    }

    #[test]
    fn an_empty_or_relative_home_gives_no_launch_agents_directory() {
        for home in ["", "Users/a"] {
            assert!(LoginItemDir::for_account(&Home(PathBuf::from(home))).is_err());
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
