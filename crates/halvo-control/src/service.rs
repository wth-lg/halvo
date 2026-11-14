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
            .to_ascii_lowercase()
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
        if program.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("AppTranslocation"))
        }) {
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
    let path = login_item_path(spec, dir);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LoginItemState::Absent),
        Err(e) => return Err(e),
    };
    let owner = user::EffectiveUser::current();
    if !is_the_plist_of(&meta, owner) {
        return Ok(LoginItemState::Stale);
    }
    let Some(mut file) = open_the_entry_that_was_read(&path, &meta)? else {
        return Ok(LoginItemState::Stale);
    };
    if !is_the_plist_of(&file.metadata()?, owner) {
        return Ok(LoginItemState::Stale);
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes)?;
    if bytes == spec.plist().as_bytes() {
        Ok(LoginItemState::Current)
    } else {
        Ok(LoginItemState::Stale)
    }
}

/// Opens `path` without following a link and without waiting (`O_NOFOLLOW`, `O_NONBLOCK`), and
/// gives the handle only when it is the entry that `seen` describes; `None` when another entry
/// stands there now, such as a FIFO, a socket or a link put in its place after the look.
#[cfg(target_os = "macos")]
fn open_the_entry_that_was_read(
    path: &std::path::Path,
    seen: &std::fs::Metadata,
) -> std::io::Result<Option<std::fs::File>> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                || matches!(
                    e.raw_os_error(),
                    Some(libc::ELOOP | libc::ENXIO | libc::EOPNOTSUPP)
                ) =>
        {
            return Ok(None)
        }
        Err(e) => return Err(e),
    };
    let now = file.metadata()?;
    Ok((now.dev() == seen.dev() && now.ino() == seen.ino()).then_some(file))
}

#[cfg(target_os = "macos")]
mod user {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct EffectiveUser(u32);

    impl EffectiveUser {
        pub(crate) fn current() -> Self {
            // SAFETY: geteuid reads the process's effective user id; it takes no pointer and
            // cannot fail.
            Self(unsafe { libc::geteuid() })
        }

        pub(crate) fn uid(self) -> u32 {
            self.0
        }

        #[cfg(test)]
        pub(crate) fn another(self) -> Self {
            Self(self.0.wrapping_add(1))
        }
    }
}

#[cfg(target_os = "macos")]
fn is_the_plist_of(meta: &std::fs::Metadata, owner: user::EffectiveUser) -> bool {
    use std::os::unix::fs::MetadataExt;

    meta.is_file() && meta.mode() & 0o7777 == 0o644 && meta.uid() == owner.uid()
}

#[cfg(target_os = "macos")]
pub fn install_login_item(
    spec: &LoginItemSpec,
    dir: &crate::paths::LoginItemDir,
) -> std::io::Result<InstallOutcome> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    remove_temp_files_of_dead_installs(spec, dir)?;
    if login_item_state(spec, dir)? == LoginItemState::Current {
        return Ok(InstallOutcome::Unchanged);
    }
    if dir.when_missing() == crate::paths::WhenMissing::CreateWithMode0755 && !dir.path().exists() {
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
fn remove_temp_files_of_dead_installs(
    spec: &LoginItemSpec,
    dir: &crate::paths::LoginItemDir,
) -> std::io::Result<()> {
    let prefix = format!(".{}.", spec.file_name());
    let entries = match std::fs::read_dir(dir.path()) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let pid = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_prefix(prefix.as_str()))
            .and_then(|rest| rest.strip_suffix(".tmp"))
            .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|digits| digits.parse::<libc::pid_t>().ok());
        if pid.is_some_and(|pid| pid > 0 && !process_is_alive(pid)) && entry.file_type()?.is_file()
        {
            match std::fs::remove_file(entry.path()) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn process_is_alive(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 sends nothing; kill only checks that the process exists.
    let sent = unsafe { libc::kill(pid, 0) };
    sent == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK)
        .open(dir)?
        .sync_all()
}

/// What a remove of the login item did: its file's outcome, and the error of the sweep of
/// the temporary files of dead installs when that sweep failed.
#[cfg(target_os = "macos")]
#[derive(Debug)]
pub struct LoginItemRemoval {
    pub outcome: RemoveOutcome,
    pub sweep: Option<std::io::Error>,
}

/// Takes the login item's file away, then the temporary files of dead installs. An error
/// of the file's removal is the error of the call; an error of the sweep is reported beside
/// the outcome.
#[cfg(target_os = "macos")]
pub fn remove_login_item(
    spec: &LoginItemSpec,
    dir: &crate::paths::LoginItemDir,
) -> std::io::Result<LoginItemRemoval> {
    let outcome = match std::fs::remove_file(login_item_path(spec, dir)) {
        Ok(()) => RemoveOutcome::Removed,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => RemoveOutcome::Absent,
        Err(e) => return Err(e),
    };
    Ok(LoginItemRemoval {
        outcome,
        sweep: remove_temp_files_of_dead_installs(spec, dir).err(),
    })
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
    use windows::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, ERROR_UNSUPPORTED_TYPE, WIN32_ERROR,
    };
    use windows::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetValueExW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    use super::{InstallOutcome, RemoveOutcome, StartValueSpec};

    pub fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn os_error(code: WIN32_ERROR) -> std::io::Error {
        std::io::Error::from_raw_os_error(code.0 as i32)
    }

    mod key {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Registry::{
            RegCloseKey, RegCreateKeyExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
            REG_OPTION_NON_VOLATILE,
        };

        use super::{os_error, wide, StartValueSpec};

        /// The open key of a start value's spec under HKEY_CURRENT_USER; only `create` makes one.
        pub(super) struct Key(HKEY);

        impl Key {
            pub(super) fn create(spec: &StartValueSpec) -> std::io::Result<Key> {
                let key = wide(spec.key());
                let mut handle = HKEY::default();
                // SAFETY: `key` is a NUL-terminated wide string that lives across the call, the
                // class is null, and `handle` is a live HKEY that the call writes.
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
                Ok(Key(handle))
            }

            pub(super) fn handle(&self) -> HKEY {
                self.0
            }
        }

        impl Drop for Key {
            fn drop(&mut self) {
                // SAFETY: only `create` makes a Key, from a handle that RegCreateKeyExW opened,
                // and this drop closes it once.
                let _ = unsafe { RegCloseKey(self.0) };
            }
        }
    }

    pub fn read(spec: &StartValueSpec) -> std::io::Result<Option<String>> {
        let (key, name) = (wide(spec.key()), wide(spec.value_name()));
        let mut size: u32 = 0;
        // SAFETY: `key` and `name` are NUL-terminated wide strings that live across the call; with
        // no data buffer, RegGetValueW writes `size` alone.
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
        // SAFETY: `key` and `name` are NUL-terminated wide strings that live across the call, and
        // the length passed in `size` is at most the byte length of `data`, so the call writes
        // inside `data`.
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
        let stored = match read(spec) {
            Err(e) if e.raw_os_error() == Some(ERROR_UNSUPPORTED_TYPE.0 as i32) => None,
            stored => stored?,
        };
        if stored.as_deref() == Some(data.as_str()) {
            return Ok(InstallOutcome::Unchanged);
        }
        let name = wide(spec.value_name());
        let handle = key::Key::create(spec)?;
        let bytes: Vec<u8> = wide(&data)
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        // SAFETY: `handle` holds the key that RegCreateKeyExW opened with KEY_SET_VALUE, `name`
        // ends in a NUL, and `name` and `bytes` live across the call.
        let set = unsafe {
            RegSetValueExW(
                handle.handle(),
                PCWSTR(name.as_ptr()),
                None,
                REG_SZ,
                Some(&bytes),
            )
        };
        if set != ERROR_SUCCESS {
            return Err(os_error(set));
        }
        Ok(InstallOutcome::Written)
    }

    pub fn remove(spec: &StartValueSpec) -> std::io::Result<RemoveOutcome> {
        let (key, name) = (wide(spec.key()), wide(spec.value_name()));
        // SAFETY: `key` and `name` are NUL-terminated wide strings that live across the call.
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
        for spelling in ["apptranslocation", "APPTRANSLOCATION", "AppTransLocation"] {
            let program = format!(
                "/private/var/folders/x/T/{spelling}/0A1B/d/Halvo.app/Contents/MacOS/Halvo"
            );
            assert_eq!(
                LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new(&program), None),
                Err(SpecError::Translocated),
                "{spelling}"
            );
        }
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

    #[cfg(target_os = "macos")]
    #[test]
    fn a_fifo_or_a_socket_at_the_login_item_path_reads_stale_at_once() {
        use crate::instance_lock::planted;

        for kind in ["a FIFO", "a socket"] {
            let home = tempfile::Builder::new().prefix("li").tempdir().unwrap();
            let dir = agents(home.path());
            std::fs::create_dir_all(dir.path()).unwrap();
            let spec = login_item();
            let path = login_item_path(&spec, &dir);
            let _listener = if kind == "a FIFO" {
                planted::mkfifo(&path);
                None
            } else {
                Some(std::os::unix::net::UnixListener::bind(&path).unwrap())
            };
            let answer = planted::answer_within(&path, &path, move |_| {
                login_item_state(&spec, &dir).map_err(|e| e.kind())
            });
            assert_eq!(
                answer,
                Some(Ok(LoginItemState::Stale)),
                "{kind} at the login item's path"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_entry_put_in_place_of_the_login_item_after_the_look_is_not_taken_and_never_waited_on() {
        use crate::instance_lock::planted;

        for kind in [
            "a FIFO",
            "a link to the file that was looked at",
            "another regular file",
        ] {
            let base = tempfile::Builder::new().prefix("li").tempdir().unwrap();
            let path = base.path().join("p");
            std::fs::write(&path, b"x").unwrap();
            let seen = std::fs::symlink_metadata(&path).unwrap();
            let moved = base.path().join("m");
            std::fs::rename(&path, &moved).unwrap();
            match kind {
                "a FIFO" => planted::mkfifo(&path),
                "a link to the file that was looked at" => {
                    std::os::unix::fs::symlink(&moved, &path).unwrap()
                }
                _ => std::fs::write(&path, b"x").unwrap(),
            }
            let answer = planted::answer_within(&path, &path, move |path| {
                open_the_entry_that_was_read(path, &seen)
                    .map(|file| file.is_some())
                    .map_err(|e| e.kind())
            });
            assert_eq!(
                answer,
                Some(Ok(false)),
                "{kind} in place of the file after the look: the open takes no other entry than \
                 the one that was looked at and returns at once; the bound is {:?}",
                planted::BOUND
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_two_opens_of_an_install_are_an_error_at_once_on_a_planted_entry() {
        use crate::instance_lock::planted;

        for kind in ["a FIFO", "a link to a FIFO"] {
            let base = tempfile::Builder::new().prefix("li").tempdir().unwrap();
            let fifo = base.path().join("f");
            planted::mkfifo(&fifo);
            let temp = base.path().join("t");
            if kind == "a link to a FIFO" {
                std::os::unix::fs::symlink(&fifo, &temp).unwrap();
            } else {
                std::fs::rename(&fifo, &temp).unwrap();
            }
            let path = base.path().join("p");
            let dir = base.path().to_path_buf();
            let waits_on = if kind == "a FIFO" { &temp } else { &fifo };
            let answer = planted::answer_within(&temp, waits_on, move |temp| {
                replace_with_temp(b"x", temp, &path, &dir).map_err(|e| e.kind())
            });
            assert_eq!(
                answer,
                Some(Err(std::io::ErrorKind::AlreadyExists)),
                "{kind} at the temporary file's path: the open with create_new takes no entry \
                 and never waits; the bound is {:?}",
                planted::BOUND
            );
        }

        let base = tempfile::Builder::new().prefix("li").tempdir().unwrap();
        let fifo = base.path().join("f");
        planted::mkfifo(&fifo);
        let temp = base.path().join("t");
        let path = base.path().join("p");
        let answer = planted::answer_within(&fifo, &fifo, move |fifo| {
            replace_with_temp(b"x", &temp, &path, fifo).map_err(|e| e.kind())
        });
        assert_eq!(
            answer,
            Some(Err(std::io::ErrorKind::NotADirectory)),
            "a FIFO at the directory's path: the open of the directory for its sync is an error \
             at once and never waits; the bound is {:?}",
            planted::BOUND
        );
    }

    #[test]
    fn the_temp_file_of_a_live_process_of_another_account_is_kept() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        std::fs::create_dir_all(dir.path()).unwrap();
        let spec = login_item();
        let other = dir.path().join(format!(".{}.1.tmp", spec.file_name()));
        std::fs::write(&other, b"x").unwrap();
        // SAFETY: signal 0 sends nothing; kill only checks whether process 1 exists and whether
        // this user may signal it.
        let sent = unsafe { libc::kill(1, 0) };
        assert_eq!(
            (sent, std::io::Error::last_os_error().raw_os_error()),
            (-1, Some(libc::EPERM))
        );
        assert_eq!(
            install_login_item(&spec, &dir).unwrap(),
            InstallOutcome::Written
        );
        assert!(
            other.exists(),
            "the temp file of pid 1, alive under another account, was taken away"
        );
    }

    #[test]
    fn a_plist_that_is_a_link_to_the_right_bytes_is_stale_and_replaced() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        std::fs::create_dir_all(dir.path()).unwrap();
        let spec = login_item();
        let target = elsewhere.path().join("copy.plist");
        std::fs::write(&target, spec.plist()).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
        let path = login_item_path(&spec, &dir);
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert_eq!(
            login_item_state(&spec, &dir).unwrap(),
            LoginItemState::Stale
        );
        assert_eq!(
            install_login_item(&spec, &dir).unwrap(),
            InstallOutcome::Written
        );
        assert!(std::fs::symlink_metadata(&path).unwrap().is_file());
        assert_eq!(
            login_item_state(&spec, &dir).unwrap(),
            LoginItemState::Current
        );
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
        // SAFETY: geteuid reads the process's effective user id; it takes no pointer and cannot
        // fail.
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
            install_login_item(&other, &dir).unwrap(),
            InstallOutcome::Written
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("/Other/Halvo") && !text.contains(APP),
            "{text}"
        );
        assert_eq!(
            login_item_state(&spec, &dir).unwrap(),
            LoginItemState::Stale
        );
        assert_eq!(
            remove_login_item(&spec, &dir).unwrap().outcome,
            RemoveOutcome::Removed
        );
        assert_eq!(
            login_item_state(&spec, &dir).unwrap(),
            LoginItemState::Absent
        );
        assert_eq!(
            remove_login_item(&spec, &dir).unwrap().outcome,
            RemoveOutcome::Absent
        );
    }

    #[test]
    fn an_install_and_a_remove_take_away_the_temp_file_of_a_dead_install_alone() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        std::fs::create_dir_all(dir.path()).unwrap();
        let spec = login_item();
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        let live = std::os::unix::process::parent_id();
        let name = spec.file_name();
        let kept = [
            format!(".{name}.{live}.tmp"),
            format!(".{name}.x{dead}.tmp"),
            format!(".{name}.+{dead}.tmp"),
            format!(".{name}..tmp"),
            format!(".{name}.{dead}.tmpx"),
            format!("{name}.{dead}.tmp"),
            format!(".com.halvo.other.plist.{dead}.tmp"),
        ];
        for kept in &kept {
            std::fs::write(dir.path().join(kept), "kept").unwrap();
        }
        let left = dir.path().join(format!(".{name}.{dead}.tmp"));
        let mut steps = Vec::new();
        for (step, run) in [
            ("an install that writes", true),
            ("an install that finds the item current", true),
            ("a remove that removes", false),
            ("a remove that finds nothing", false),
        ] {
            std::fs::write(&left, "half of a plist").unwrap();
            if run {
                install_login_item(&spec, &dir).unwrap();
            } else {
                remove_login_item(&spec, &dir).unwrap();
            }
            steps.push((step, left.exists()));
        }
        assert_eq!(
            steps,
            [
                ("an install that writes", false),
                ("an install that finds the item current", false),
                ("a remove that removes", false),
                ("a remove that finds nothing", false),
            ]
        );
        for kept in &kept {
            assert!(dir.path().join(kept).exists(), "{kept} was taken away");
        }
    }

    #[test]
    fn an_install_writes_over_a_leftover_temp_file_of_its_own_pid() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        std::fs::create_dir_all(dir.path()).unwrap();
        let spec = login_item();
        let own = dir
            .path()
            .join(format!(".{}.{}.tmp", spec.file_name(), std::process::id()));
        std::fs::write(&own, b"half").unwrap();
        assert_eq!(
            install_login_item(&spec, &dir).unwrap(),
            InstallOutcome::Written
        );
        assert_eq!(
            std::fs::read(login_item_path(&spec, &dir)).unwrap(),
            spec.plist().as_bytes()
        );
        assert!(!own.exists());
    }

    #[test]
    fn a_directory_or_a_link_at_a_dead_install_s_name_is_kept_and_stops_no_install_or_remove() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        std::fs::create_dir_all(dir.path()).unwrap();
        let spec = login_item();
        let dead = || {
            let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
            let pid = child.id();
            child.wait().unwrap();
            pid
        };
        let odd = dir
            .path()
            .join(format!(".{}.{}.tmp", spec.file_name(), dead()));
        std::fs::create_dir(&odd).unwrap();
        let link = dir
            .path()
            .join(format!(".{}.{}.tmp", spec.file_name(), dead()));
        std::os::unix::fs::symlink(home.path(), &link).unwrap();
        assert_eq!(
            install_login_item(&spec, &dir).unwrap(),
            InstallOutcome::Written
        );
        assert_eq!(
            remove_login_item(&spec, &dir).unwrap().outcome,
            RemoveOutcome::Removed
        );
        assert!(!login_item_path(&spec, &dir).exists());
        assert!(odd.is_dir());
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    }

    fn under_a_link() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().canonicalize().unwrap().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = root.path().canonicalize().unwrap().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        (root, real, link)
    }

    #[test]
    fn the_sweep_keeps_a_link_to_a_regular_file_at_a_dead_install_s_name() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let (root, _real, link) = under_a_link();
        let dir = agents(&link.join("home"));
        std::fs::create_dir_all(dir.path()).unwrap();
        assert!(dir.path().starts_with(&link));
        let spec = login_item();
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        let target = root.path().join("a regular file");
        std::fs::write(&target, b"not the installer's").unwrap();
        let at_dead_name = dir.path().join(format!(".{}.{dead}.tmp", spec.file_name()));
        std::os::unix::fs::symlink(&target, &at_dead_name).unwrap();
        assert_eq!(
            install_login_item(&spec, &dir).unwrap(),
            InstallOutcome::Written
        );
        assert!(std::fs::symlink_metadata(&at_dead_name)
            .unwrap()
            .is_symlink());
        assert_eq!(
            remove_login_item(&spec, &dir).unwrap().outcome,
            RemoveOutcome::Removed
        );
        assert!(std::fs::symlink_metadata(&at_dead_name)
            .unwrap()
            .is_symlink());
        assert_eq!(std::fs::read(&target).unwrap(), b"not the installer's");
    }

    #[test]
    fn the_scratch_form_holds_its_directory_as_it_resolves_and_not_as_it_was_given() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let (_root, real, link) = under_a_link();
        let user = DataDir::for_account(&Home(PathBuf::from("/Users/a b"))).unwrap();
        let scratch = ScratchDir::against(&link, &user).unwrap();
        let target =
            LoginItemDir::scratch_form(&scratch, &Home(PathBuf::from("/Users/a b"))).unwrap();
        assert_eq!(target.path(), real);
        let data = DataDir::from_flag(&real).unwrap();
        let spec = LoginItemSpec::new("com.halvo.check-l", Path::new(APP), Some(&data)).unwrap();
        install_login_item(&spec, &target).unwrap();
        assert!(real.join("com.halvo.check-l.plist").is_file());
        let turned = link.with_file_name("turned");
        std::fs::create_dir(&turned).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&turned, &link).unwrap();
        assert_eq!(
            remove_login_item(&spec, &target).unwrap().outcome,
            RemoveOutcome::Removed
        );
        assert!(!real.join("com.halvo.check-l.plist").exists());
        assert_eq!(std::fs::read_dir(&turned).unwrap().count(), 0);
    }

    #[test]
    fn a_remove_that_cannot_take_the_file_away_says_so() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        let spec = login_item();
        install_login_item(&spec, &dir).unwrap();
        let removed = {
            let _back = ModeFor::set(dir.path(), 0o555, 0o755);
            remove_login_item(&spec, &dir)
        };
        assert_eq!(
            removed.map(|r| r.outcome).map_err(|e| e.kind()),
            Err(std::io::ErrorKind::PermissionDenied)
        );
        assert!(login_item_path(&spec, &dir).exists());
    }

    struct ModeFor<'a> {
        path: &'a Path,
        back: u32,
    }

    impl<'a> ModeFor<'a> {
        fn set(path: &'a Path, mode: u32, back: u32) -> Self {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
            Self { path, back }
        }
    }

    impl Drop for ModeFor<'_> {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(self.path, std::fs::Permissions::from_mode(self.back));
        }
    }

    #[test]
    fn a_mode_set_for_a_test_is_put_back_when_the_test_panics() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let unwound = std::panic::catch_unwind(|| {
            let _back = ModeFor::set(&dir, 0o300, 0o755);
            panic!("a panic inside the call");
        });
        assert!(unwound.is_err());
        assert_eq!(mode(&dir), 0o755);
    }

    #[test]
    fn an_error_of_the_sweep_does_not_keep_the_login_item() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        let spec = login_item();
        install_login_item(&spec, &dir).unwrap();
        let (removed, left) = {
            let _back = ModeFor::set(dir.path(), 0o300, 0o755);
            let removed = remove_login_item(&spec, &dir).unwrap();
            (removed, login_item_path(&spec, &dir).exists())
        };
        assert_eq!(removed.outcome, RemoveOutcome::Removed);
        assert_eq!(
            removed.sweep.map(|e| e.kind()),
            Some(std::io::ErrorKind::PermissionDenied)
        );
        assert!(!left, "the login item stayed");
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
    fn the_plist_is_a_regular_file_that_the_user_owns_with_mode_0644() {
        let root = tempfile::tempdir().unwrap();
        let made = |name: &str, mode: u32| {
            let path = root.path().join(name);
            std::fs::write(&path, LOGIN_ITEM_PLIST).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            std::fs::symlink_metadata(&path).unwrap()
        };
        let plist = made("plist", 0o644);
        let me = user::EffectiveUser::current();
        assert_eq!(plist.uid(), me.uid());
        assert!(is_the_plist_of(&plist, me));
        assert!(!is_the_plist_of(&plist, me.another()), "another owner");
        for mode in [0o664, 0o640, 0o600, 0o444, 0o755, 0o4644] {
            let other = made(&format!("mode-{mode:o}"), mode);
            assert!(!is_the_plist_of(&other, me), "mode {mode:o}");
        }
        let dir = root.path().join("dir");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o644)).unwrap();
        let dir_meta = std::fs::symlink_metadata(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!is_the_plist_of(&dir_meta, me), "a directory");
        let link = root.path().join("link");
        std::os::unix::fs::symlink(root.path().join("plist"), &link).unwrap();
        let link_meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(!is_the_plist_of(&link_meta, me), "a link");
    }

    #[test]
    fn umask_077_gives_a_0644_plist_in_a_new_0755_directory() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        // SAFETY: umask sets the process's file mode mask and returns the old one; it takes no
        // pointer and cannot fail.
        let before = unsafe { libc::umask(0o077) };
        let result = install_login_item(&login_item(), &dir);
        // SAFETY: umask sets the process's file mode mask and returns the old one; it takes no
        // pointer and cannot fail.
        unsafe { libc::umask(before) };
        assert_eq!(result.unwrap(), InstallOutcome::Written);
        assert_eq!(mode(dir.path()), 0o755);
        assert_eq!(mode(&login_item_path(&login_item(), &dir)), 0o644);
    }

    #[test]
    fn a_failed_rename_takes_its_temp_file_away() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        let spec = login_item();
        std::fs::create_dir_all(login_item_path(&spec, &dir)).unwrap();
        assert!(install_login_item(&spec, &dir).is_err());
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [spec.file_name()]);
    }

    #[test]
    fn a_data_directory_with_a_control_character_is_an_error() {
        let data = DataDir::from_flag("/Users/a b/Halvo\u{7}").unwrap();
        assert_eq!(
            LoginItemSpec::new(LOGIN_ITEM_LABEL, Path::new(APP), Some(&data)),
            Err(SpecError::Text)
        );
    }

    #[test]
    fn umask_000_gives_a_0644_plist_in_a_new_0755_directory() {
        let _guard = PROCESS_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let dir = agents(home.path());
        // SAFETY: umask sets the process's file mode mask and returns the old one; it takes no
        // pointer and cannot fail.
        let before = unsafe { libc::umask(0) };
        let result = install_login_item(&login_item(), &dir);
        // SAFETY: umask sets the process's file mode mask and returns the old one; it takes no
        // pointer and cannot fail.
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
        let scratch = ScratchDir::against(root.path(), &user).unwrap();
        let target =
            LoginItemDir::scratch_form(&scratch, &Home(PathBuf::from("/Users/a b"))).unwrap();
        let data = DataDir::from_flag(root.path()).unwrap();
        let spec = LoginItemSpec::new("com.halvo.check-l", Path::new(APP), Some(&data)).unwrap();
        assert_eq!(
            install_login_item(&spec, &target).unwrap(),
            InstallOutcome::Written
        );
        let path = login_item_path(&spec, &target);
        assert_eq!(
            path,
            root.path()
                .canonicalize()
                .unwrap()
                .join("com.halvo.check-l.plist")
        );
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
        assert_eq!(
            ScratchDir::against(root.path().join("missing"), &user),
            Err(PathError::Unresolved)
        );
        let gone = root.path().join("gone");
        std::fs::create_dir(&gone).unwrap();
        let scratch = ScratchDir::against(&gone, &user).unwrap();
        let target =
            LoginItemDir::scratch_form(&scratch, &Home(PathBuf::from("/Users/a b"))).unwrap();
        std::fs::remove_dir(&gone).unwrap();
        assert!(install_login_item(&spec, &target).is_err());
        assert!(!gone.exists());
    }

    #[test]
    fn the_scratch_form_refuses_the_account_launch_agents_directory_and_anything_inside_it() {
        let home = tempfile::tempdir().unwrap();
        let account = Home(home.path().to_path_buf());
        let agents = LoginItemDir::for_account(&account).unwrap();
        std::fs::create_dir_all(agents.path().join("check")).unwrap();
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
            let scratch = ScratchDir::against(&inside, &user).unwrap();
            assert_eq!(
                LoginItemDir::scratch_form(&scratch, &account),
                Err(PathError::InsideLaunchAgents),
                "{}",
                inside.display()
            );
        }
        std::fs::create_dir(home.path().join("scratch")).unwrap();
        let outside = ScratchDir::against(home.path().join("scratch"), &user).unwrap();
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
        for label in [
            "com.halvo.capture",
            "com.halvo.capture.test",
            "COM.HALVO.CAPTURE",
            "com.halvo.Capture",
            "Com.Halvo.Capture.Test",
        ] {
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
            r"C:\",
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
            // SAFETY: `key` is a NUL-terminated wide string that lives across the call.
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

    #[test]
    fn a_start_value_of_another_type_is_replaced_by_the_string() {
        let scratch = ScratchKey(format!(r"Software\Halvo-Test-Type-{}", std::process::id()));
        let spec =
            StartValueSpec::with_key(&scratch.0, RUN_VALUE_NAME, Path::new(r"C:\Halvo\halvo.exe"))
                .unwrap();
        for (kind, data) in [
            ("REG_DWORD", "1"),
            ("REG_BINARY", "00"),
            ("REG_MULTI_SZ", "x"),
        ] {
            let status = std::process::Command::new("reg")
                .args(["add", &format!(r"HKCU\{}", scratch.0), "/v", RUN_VALUE_NAME])
                .args(["/t", kind, "/d", data, "/f"])
                .status()
                .unwrap();
            assert!(status.success(), "reg add {kind}");
            let written = write_start_value(&spec);
            println!("a stored {kind}: write={written:?}");
            assert_eq!(written.unwrap(), InstallOutcome::Written, "{kind}");
            assert_eq!(
                read_start_value(&spec).unwrap(),
                Some(spec.data()),
                "{kind}"
            );
        }
    }
}
