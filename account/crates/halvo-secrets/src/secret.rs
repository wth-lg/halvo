use std::sync::atomic::{compiler_fence, Ordering};

/// The largest secret that a store keeps, in bytes.
pub const MAX_SECRET_LEN: usize = 16 * 1024;

/// The bytes of one secret, from 1 to `MAX_SECRET_LEN` of them. It has no `Debug`, no
/// `Display` and no `Clone`, so no format prints its bytes and no copy of them is made
/// through it, and when it is dropped every byte of its buffer, the spare capacity
/// included, is overwritten with zeros before the buffer is freed.
pub struct Secret {
    bytes: Vec<u8>,
}

impl Secret {
    /// Takes `bytes` as a secret. With no bytes or more than `MAX_SECRET_LEN` it answers
    /// `None`, and the buffer of `bytes` is overwritten with zeros before it is freed.
    pub fn new(bytes: Vec<u8>) -> Option<Secret> {
        let secret = Secret { bytes };
        if secret.bytes.is_empty() || secret.bytes.len() > MAX_SECRET_LEN {
            return None;
        }
        Some(secret)
    }

    /// The bytes of the secret.
    pub fn expose(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(&mut self.bytes);
    }
}

pub(crate) struct Buffer {
    bytes: Vec<u8>,
}

impl Buffer {
    pub(crate) fn zeroed(len: usize) -> Buffer {
        Buffer {
            bytes: vec![0; len],
        }
    }

    pub(crate) fn capacity(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn space(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    pub(crate) fn into_vec(mut self, len: usize) -> Vec<u8> {
        let mut bytes = std::mem::take(&mut self.bytes);
        bytes.truncate(len);
        bytes
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        wipe(&mut self.bytes);
    }
}

fn wipe(bytes: &mut Vec<u8>) {
    let start = bytes.as_mut_ptr();
    for offset in 0..bytes.capacity() {
        unsafe { std::ptr::write_volatile(start.add(offset), 0) };
    }
    compiler_fence(Ordering::SeqCst);
    bytes.clear();
}
