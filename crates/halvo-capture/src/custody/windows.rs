//! Windows DPAPI key custody.

use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use zeroize::Zeroizing;

use super::dev_fallback::store_key_from;
use super::{CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};

/// A buffer DPAPI allocated: zeroed, then freed, on drop.
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

pub(crate) fn destroy_root_is_deleting_the_wrapped_blob() -> Result<(), CustodyError> {
    Ok(())
}

/// The release Windows custody: DPAPI user scope wrapping the store key directly.
pub struct DpapiCustody {
    root_id: RootId,
}

impl DpapiCustody {
    /// Build a custody scoped by a per-store entropy label (a non-secret id).
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
        Ok(WrappedKey(dpapi_protect(
            key.expose(),
            self.root_id.as_bytes(),
        )?))
    }

    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        store_key_from(dpapi_unprotect(&wrapped.0, self.root_id.as_bytes())?)
    }

    fn destroy_root(&self) -> Result<(), CustodyError> {
        destroy_root_is_deleting_the_wrapped_blob()
    }
}
