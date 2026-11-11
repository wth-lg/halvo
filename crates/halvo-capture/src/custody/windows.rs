//! Windows DPAPI key custody.

use std::path::PathBuf;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use zeroize::Zeroizing;

use super::file_root::{FileRoots, Protector};
use super::{CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};

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
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out as *mut _,
        )
    }
    .map_err(|e| CustodyError::Crypto(format!("CryptProtectData: {e}")))?;
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
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out as *mut _,
        )
    }
    .map_err(|e| CustodyError::Crypto(format!("CryptUnprotectData: {e}")))?;
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

    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        user_roots()?.unwrap(self.root_id, wrapped)
    }

    fn destroy_root(&self) -> Result<(), CustodyError> {
        user_roots()?.destroy_root(self.root_id)
    }
}
