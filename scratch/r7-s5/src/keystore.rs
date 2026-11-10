//! One named item in the platform key store. Every call runs with user interaction
//! disabled: it succeeds silently or it fails, and it never opens a dialog.

use std::fmt;

#[derive(Debug)]
pub struct KeyStoreError {
    pub op: &'static str,
    /// OSStatus on macOS, a Win32 error code on Windows.
    pub code: i64,
    pub message: String,
    pub not_found: bool,
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} failed: code {}: {}",
            self.op, self.code, self.message
        )
    }
}

impl std::error::Error for KeyStoreError {}

pub use imp::{delete, read, store, BACKEND};

#[cfg(target_os = "macos")]
mod imp {
    use super::KeyStoreError;
    use security_framework::base::Error;
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    pub const BACKEND: &str = "macOS file-based keychain through security-framework (SecItem), \
         SecKeychainSetUserInteractionAllowed(false)";

    const SERVICE: &str = "dev.halvo.spike.r7-s5";
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    fn error(op: &'static str, source: Error) -> KeyStoreError {
        KeyStoreError {
            op,
            code: i64::from(source.code()),
            message: source.message().unwrap_or_else(|| "no message".to_string()),
            not_found: source.code() == ERR_SEC_ITEM_NOT_FOUND,
        }
    }

    fn without_interaction<T>(
        op: &'static str,
        call: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, KeyStoreError> {
        let _interaction_off = SecKeychain::disable_user_interaction()
            .map_err(|source| error("disable user interaction", source))?;
        match SecKeychain::user_interaction_allowed() {
            Ok(false) => {}
            Ok(true) => {
                return Err(KeyStoreError {
                    op: "disable user interaction",
                    code: 0,
                    message: "user interaction is still allowed".to_string(),
                    not_found: false,
                })
            }
            Err(source) => return Err(error("check user interaction", source)),
        }
        call().map_err(|source| error(op, source))
    }

    fn delete_if_present(name: &str) -> Result<(), Error> {
        match delete_generic_password(SERVICE, name) {
            Err(source) if source.code() != ERR_SEC_ITEM_NOT_FOUND => Err(source),
            _ => Ok(()),
        }
    }

    /// Replaces the item, so its access list always names the build that stored it.
    pub fn store(name: &str, value: &[u8]) -> Result<(), KeyStoreError> {
        without_interaction("store", || {
            delete_if_present(name)?;
            set_generic_password(SERVICE, name, value)
        })
    }

    pub fn read(name: &str) -> Result<Vec<u8>, KeyStoreError> {
        without_interaction("read", || get_generic_password(SERVICE, name))
    }

    pub fn delete(name: &str) -> Result<(), KeyStoreError> {
        without_interaction("delete", || delete_if_present(name))
    }
}

#[cfg(windows)]
mod imp {
    use super::KeyStoreError;
    use std::path::PathBuf;
    use std::{fs, io, ptr};
    use windows_sys::Win32::Foundation::{GetLastError, LocalFree};
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    pub const BACKEND: &str =
        "Windows DPAPI in user scope (CryptProtectData and CryptUnprotectData \
         with CRYPTPROTECT_UI_FORBIDDEN), blob in a file under %LOCALAPPDATA%";

    fn blob_path(name: &str) -> Result<PathBuf, KeyStoreError> {
        let base = std::env::var_os("LOCALAPPDATA").ok_or_else(|| KeyStoreError {
            op: "locate the blob",
            code: 0,
            message: "LOCALAPPDATA is not set".to_string(),
            not_found: false,
        })?;
        Ok(PathBuf::from(base)
            .join("halvo-spike-r7-s5")
            .join(format!("{name}.dpapi")))
    }

    fn io_error(op: &'static str, source: io::Error) -> KeyStoreError {
        KeyStoreError {
            op,
            code: i64::from(source.raw_os_error().unwrap_or(0)),
            message: source.to_string(),
            not_found: source.kind() == io::ErrorKind::NotFound,
        }
    }

    fn dpapi(op: &'static str, protect: bool, input: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
        let data_in = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_ptr().cast_mut(),
        };
        let mut data_out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };
        // SAFETY: `data_in` points at `input` for the length given and is only read.
        // `data_out` receives a buffer that DPAPI allocates and that is freed below.
        // The description, entropy, reserved and prompt arguments are optional.
        let ok = unsafe {
            if protect {
                CryptProtectData(
                    &data_in,
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut data_out,
                )
            } else {
                CryptUnprotectData(
                    &data_in,
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut data_out,
                )
            }
        };
        if ok == 0 {
            // SAFETY: reads the calling thread's last error, set by the failed call above.
            let code = unsafe { GetLastError() };
            return Err(KeyStoreError {
                op,
                code: i64::from(code),
                message: io::Error::from_raw_os_error(code as i32).to_string(),
                not_found: false,
            });
        }
        // SAFETY: on success DPAPI returns `cbData` readable bytes at `pbData`.
        let output =
            unsafe { std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize) }
                .to_vec();
        // SAFETY: the buffer came from DPAPI, which documents LocalFree as its release.
        unsafe { LocalFree(data_out.pbData.cast()) };
        Ok(output)
    }

    pub fn store(name: &str, value: &[u8]) -> Result<(), KeyStoreError> {
        let blob = dpapi("store", true, value)?;
        let path = blob_path(name)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|source| io_error("store", source))?;
        }
        fs::write(&path, blob).map_err(|source| io_error("store", source))
    }

    pub fn read(name: &str) -> Result<Vec<u8>, KeyStoreError> {
        let blob = fs::read(blob_path(name)?).map_err(|source| io_error("read", source))?;
        dpapi("read", false, &blob)
    }

    pub fn delete(name: &str) -> Result<(), KeyStoreError> {
        match fs::remove_file(blob_path(name)?) {
            Err(source) if source.kind() != io::ErrorKind::NotFound => {
                Err(io_error("delete", source))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
compile_error!("the spike measures the macOS keychain and Windows DPAPI only");
