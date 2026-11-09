//! Windows DPAPI key custody.
//!
//! `CryptProtectData` in user scope wraps the store key: only a process with the
//! same logon on the same computer can unwrap it (researcher-2 findings 26, 27,
//! 31). `CRYPTPROTECT_UI_FORBIDDEN` makes any call that would show UI fail instead
//! of blocking, so the logon-trigger daemon never prompts. An
//! application-specific entropy (the per-store root id) scopes the blob to this
//! store. The DPAPI master key is derived from the user's logon password, so a
//! second local account and an administrator who does not know the password
//! recover only ciphertext.
//!
//! This path reports [`CustodyKind::Dpapi`], which the release privacy check
//! accepts. The dev fallback on Windows ([`super::dev_fallback`]) shares the same
//! `CryptProtectData` mechanism but reports [`CustodyKind::DevFallback`]; the
//! difference is the keyed kind, not the primitive.
//!
//! Compiled on Windows; its functional test is deferred to a Windows machine (no
//! interactive Windows desktop in this run).

use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use super::{CustodyError, CustodyKind, KeyCustody, StoreKey, WrappedKey};

/// Protect `data` with DPAPI user scope, UI forbidden, scoped by `entropy`.
pub(crate) fn dpapi_protect(data: &[u8], entropy: &[u8]) -> Result<Vec<u8>, CustodyError> {
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let entropy_blob = CRYPT_INTEGER_BLOB {
            cbData: entropy.len() as u32,
            pbData: entropy.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(
            &in_blob as *const _,
            None,
            Some(&entropy_blob as *const _),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out as *mut _,
        )
        .map_err(|e| CustodyError::Crypto(format!("CryptProtectData: {e}")))?;
        Ok(copy_and_free(&out))
    }
}

/// Unprotect a blob produced by [`dpapi_protect`] with the same `entropy`.
pub(crate) fn dpapi_unprotect(blob: &[u8], entropy: &[u8]) -> Result<Vec<u8>, CustodyError> {
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr() as *mut u8,
        };
        let entropy_blob = CRYPT_INTEGER_BLOB {
            cbData: entropy.len() as u32,
            pbData: entropy.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(
            &in_blob as *const _,
            None,
            Some(&entropy_blob as *const _),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out as *mut _,
        )
        .map_err(|e| CustodyError::Crypto(format!("CryptUnprotectData: {e}")))?;
        Ok(copy_and_free(&out))
    }
}

/// Copy a DPAPI output blob into an owned `Vec` and free the OS allocation.
unsafe fn copy_and_free(out: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let slice = std::slice::from_raw_parts(out.pbData, out.cbData as usize);
    let owned = slice.to_vec();
    let _ = LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
    owned
}

/// The release Windows custody: DPAPI user scope wrapping the store key directly.
pub struct DpapiCustody {
    entropy: Vec<u8>,
}

impl DpapiCustody {
    /// Build a custody scoped by a per-store entropy label (a non-secret id).
    pub fn new(entropy: impl Into<Vec<u8>>) -> Self {
        Self {
            entropy: entropy.into(),
        }
    }
}

impl KeyCustody for DpapiCustody {
    fn kind(&self) -> CustodyKind {
        CustodyKind::Dpapi
    }

    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        let blob = dpapi_protect(key.expose(), &self.entropy)?;
        Ok(WrappedKey(blob))
    }

    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        let plain = dpapi_unprotect(&wrapped.0, &self.entropy)?;
        let arr: [u8; 32] = plain
            .as_slice()
            .try_into()
            .map_err(|_| CustodyError::Crypto("store key is not 32 bytes".to_string()))?;
        Ok(StoreKey::from_bytes(arr))
    }
}
