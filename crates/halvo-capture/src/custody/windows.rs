//! Windows DPAPI key custody.

use std::path::PathBuf;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use zeroize::Zeroizing;

use super::file_root::{FileRoots, Protector};
use super::{CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};

const USER_SCOPE_NO_UI: u32 = CRYPTPROTECT_UI_FORBIDDEN;
const DATA_DOES_NOT_DECODE: i32 = 0x8007_000D_u32 as i32;

fn classify_dpapi_failure(call: &str, code: i32) -> CustodyError {
    if code == DATA_DOES_NOT_DECODE {
        CustodyError::Crypto(format!("{call}: the data does not decode"))
    } else {
        CustodyError::RootLocked(code)
    }
}

struct DpapiBlob(CRYPT_INTEGER_BLOB);

impl DpapiBlob {
    fn as_slice(&self) -> &[u8] {
        if self.0.pbData.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.0.pbData, self.0.cbData as usize) }
    }
}

impl Drop for DpapiBlob {
    fn drop(&mut self) {
        if self.0.pbData.is_null() {
            return;
        }
        unsafe {
            std::ptr::write_bytes(self.0.pbData, 0, self.0.cbData as usize);
            let _ = LocalFree(Some(HLOCAL(self.0.pbData as *mut core::ffi::c_void)));
        }
    }
}

fn borrowed_blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    }
}

pub(crate) fn dpapi_protect(data: &[u8], entropy: &[u8]) -> Result<Vec<u8>, CustodyError> {
    let in_blob = borrowed_blob(data);
    let entropy_blob = borrowed_blob(entropy);
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &in_blob as *const _,
            None,
            Some(&entropy_blob as *const _),
            None,
            None,
            USER_SCOPE_NO_UI,
            &mut out as *mut _,
        )
    }
    .map_err(|e| classify_dpapi_failure("CryptProtectData", e.code().0))?;
    Ok(DpapiBlob(out).as_slice().to_vec())
}

pub(crate) fn dpapi_unprotect(
    blob: &[u8],
    entropy: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CustodyError> {
    let in_blob = borrowed_blob(blob);
    let entropy_blob = borrowed_blob(entropy);
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &in_blob as *const _,
            None,
            Some(&entropy_blob as *const _),
            None,
            None,
            USER_SCOPE_NO_UI,
            &mut out as *mut _,
        )
    }
    .map_err(|e| classify_dpapi_failure("CryptUnprotectData", e.code().0))?;
    Ok(Zeroizing::new(DpapiBlob(out).as_slice().to_vec()))
}

pub(crate) struct Dpapi;

impl Protector for Dpapi {
    fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError> {
        dpapi_protect(plain, label)
    }

    fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError> {
        dpapi_unprotect(blob, label)
    }
}

pub(crate) fn user_roots() -> Result<FileRoots<Dpapi>, CustodyError> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or(CustodyError::RootDenied)?;
    Ok(FileRoots::new(
        PathBuf::from(local).join("Halvo").join("roots"),
        Dpapi,
    ))
}

/// The release Windows custody: the store key wrapped under a per-store root
/// that DPAPI protects for this user.
pub struct DpapiCustody {
    root_id: RootId,
}

impl DpapiCustody {
    /// Build a custody bound to a per-store root locator.
    pub fn new(root_id: RootId) -> Self {
        Self { root_id }
    }
}

impl KeyCustody for DpapiCustody {
    fn kind(&self) -> CustodyKind {
        CustodyKind::Dpapi
    }

    fn root_id(&self) -> RootId {
        self.root_id
    }

    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        user_roots()?.wrap(self.root_id, key)
    }

    fn wrap_store_key_under_new_root(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        user_roots()?.wrap_under_new_root(self.root_id, key)
    }

    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        user_roots()?.unwrap(self.root_id, wrapped)
    }

    fn destroy_root(&self) -> Result<(), CustodyError> {
        user_roots()?.destroy_root(self.root_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dpapi_code_that_can_clear_is_root_locked_and_data_that_does_not_decode_is_crypto() {
        const NTE_BAD_KEY_STATE: i32 = 0x8009_000B_u32 as i32;
        assert!(matches!(
            classify_dpapi_failure("CryptUnprotectData", NTE_BAD_KEY_STATE),
            CustodyError::RootLocked(code) if code == NTE_BAD_KEY_STATE
        ));
        assert!(matches!(
            classify_dpapi_failure("CryptUnprotectData", DATA_DOES_NOT_DECODE),
            CustodyError::Crypto(_)
        ));
    }

    #[test]
    fn a_blob_that_dpapi_cannot_decode_is_crypto_and_no_state_that_can_clear() {
        let blob = dpapi_protect(b"thirty-two bytes of test payload", b"label").unwrap();
        let cases = [
            ("another label", dpapi_unprotect(&blob, b"another label")),
            (
                "bytes that are no blob",
                dpapi_unprotect(&[0x5a; 64], b"label"),
            ),
            (
                "half of the blob",
                dpapi_unprotect(&blob[..blob.len() / 2], b"label"),
            ),
        ];
        for (case, answer) in cases {
            assert!(
                matches!(answer, Err(CustodyError::Crypto(_))),
                "{case}: {:?}",
                answer.err()
            );
        }
    }
}
