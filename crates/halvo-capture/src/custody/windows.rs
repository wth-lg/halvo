//! Windows DPAPI key custody.

use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use super::{CustodyError, CustodyKind, KeyCustody, StoreKey, WrappedKey};

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
