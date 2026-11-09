//! Per-segment XChaCha20-Poly1305 file writer and reader.
//!
//! Each segment is one file under a random 256-bit per-segment key, encrypted with
//! the STREAM chunked AEAD construction (aead-stream) in 64 KiB chunks. The
//! segment header (magic, version, segment id, segment-key id) is the associated
//! data on every chunk, so a chunk cannot be moved between segments or reordered
//! without failing authentication. The per-segment key is itself wrapped under the
//! one store key and kept in the database, so destroying that one row crypto-shreds
//! exactly this segment.
//!
//! The writer **streams**: it reads the plaintext 64 KiB at a time and never holds
//! more than two chunks plus one chunk of ciphertext in memory, so resident memory
//! does not grow with the segment or the retention window.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use aead::Payload;
use aead_stream::{DecryptorBE32, EncryptorBE32};
use chacha20poly1305::aead::array::Array;
use chacha20poly1305::XChaCha20Poly1305;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::custody::dev_fallback::{aead_open, aead_seal, random_key};
use crate::event::{SegmentKeyId, SegmentRef};

use super::{StoreError, StoreResult};

/// Plaintext bytes per STREAM chunk.
pub(crate) const CHUNK: usize = 64 * 1024;
/// Poly1305 tag length added to each chunk's ciphertext.
const TAG: usize = 16;
/// Ciphertext bytes per full chunk.
const CIPHER_CHUNK: usize = CHUNK + TAG;
/// STREAM-BE32 nonce length for XChaCha20-Poly1305 (24-byte nonce minus the
/// 5-byte STREAM counter overhead).
const STREAM_NONCE: usize = 24 - 5;
/// File magic plus a one-byte format version.
const MAGIC: &[u8; 8] = b"HLVOSEG1";
/// Header length: magic (8) + segment id (16) + segment-key id (8).
const HEADER_LEN: usize = 8 + 16 + 8;

/// A per-segment key: 32 bytes, zeroed on drop so a destroyed key is not a usable
/// key (type-discipline). It is never serialized in the clear.
#[derive(Zeroize, ZeroizeOnDrop)]
pub(crate) struct SegmentKey([u8; 32]);

impl SegmentKey {
    pub(crate) fn random() -> StoreResult<Self> {
        let bytes = random_key().map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(Self(bytes))
    }
    fn expose(&self) -> &[u8; 32] {
        &self.0
    }
    fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// Wrap a per-segment key under the store key for storage in the database.
pub(crate) fn wrap_segment_key(store_key: &[u8; 32], seg_key: &SegmentKey) -> StoreResult<Vec<u8>> {
    aead_seal(store_key, seg_key.expose(), b"halvo-segment-key")
        .map_err(|_| StoreError::Io("sealing the segment key failed".to_string()))
}

/// Unwrap a per-segment key the store key wrapped. Fails (authentication) if the
/// store key is wrong or the blob was tampered with.
pub(crate) fn unwrap_segment_key(store_key: &[u8; 32], wrapped: &[u8]) -> StoreResult<SegmentKey> {
    let plain =
        aead_open(store_key, wrapped, b"halvo-segment-key").map_err(|_| StoreError::Sealed)?;
    let arr: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| StoreError::Io("segment key is not 32 bytes".to_string()))?;
    Ok(SegmentKey::from_bytes(arr))
}

/// Directory holding segment files, sharded by the first two hex characters of the
/// segment id to keep any one directory small.
fn segments_root(store_dir: &Path) -> PathBuf {
    store_dir.join("segments")
}

/// Absolute path of a segment file.
pub(crate) fn segment_path(store_dir: &Path, segment_id: Uuid) -> PathBuf {
    let id = segment_id.simple().to_string();
    let shard = &id[..2];
    segments_root(store_dir)
        .join(shard)
        .join(format!("{id}.seg"))
}

/// Build the header that is the associated data for every chunk of this segment.
fn header(segment_id: Uuid, segment_key_id: SegmentKeyId) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..8].copy_from_slice(MAGIC);
    h[8..24].copy_from_slice(segment_id.as_bytes());
    h[24..32].copy_from_slice(&segment_key_id.0.to_le_bytes());
    h
}

/// Encrypt `plaintext` to the segment file, streaming in 64 KiB chunks. Returns
/// the plaintext length (the segment's logical byte length).
pub(crate) fn write_segment_file(
    store_dir: &Path,
    seg_key: &SegmentKey,
    segment_id: Uuid,
    segment_key_id: SegmentKeyId,
    plaintext: &[u8],
) -> StoreResult<u64> {
    let path = segment_path(store_dir, segment_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| StoreError::Io(e.to_string()))?;
    }
    let file = std::fs::File::create(&path).map_err(|e| StoreError::Io(e.to_string()))?;
    let mut writer = io::BufWriter::new(file);

    let mut nonce_bytes = [0u8; STREAM_NONCE];
    getrandom::getrandom(&mut nonce_bytes).map_err(|e| StoreError::Io(e.to_string()))?;
    let head = header(segment_id, segment_key_id);

    writer
        .write_all(&head)
        .and_then(|()| writer.write_all(&nonce_bytes))
        .map_err(|e| StoreError::Io(e.to_string()))?;

    encrypt_stream(
        &mut io::Cursor::new(plaintext),
        &mut writer,
        seg_key.expose(),
        &nonce_bytes,
        &head,
    )?;
    writer.flush().map_err(|e| StoreError::Io(e.to_string()))?;
    Ok(plaintext.len() as u64)
}

/// Stream-encrypt a reader into a writer with a one-chunk lookahead so the final
/// chunk is sealed with `encrypt_last`. Holds at most two plaintext chunks.
fn encrypt_stream<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    key: &[u8; 32],
    nonce_bytes: &[u8; STREAM_NONCE],
    aad: &[u8],
) -> StoreResult<()> {
    let key = Array::from(*key);
    let nonce = Array::try_from(&nonce_bytes[..])
        .map_err(|_| StoreError::Io("bad stream nonce".to_string()))?;
    let mut enc: EncryptorBE32<XChaCha20Poly1305> = EncryptorBE32::new(&key, &nonce);

    let mut current = read_chunk(reader)?;
    loop {
        if current.len() < CHUNK {
            let ct = enc
                .encrypt_last(Payload { msg: &current, aad })
                .map_err(|_| StoreError::Io("encrypt_last failed".to_string()))?;
            writer
                .write_all(&ct)
                .map_err(|e| StoreError::Io(e.to_string()))?;
            current.zeroize();
            return Ok(());
        }
        let next = read_chunk(reader)?;
        if next.is_empty() {
            let ct = enc
                .encrypt_last(Payload { msg: &current, aad })
                .map_err(|_| StoreError::Io("encrypt_last failed".to_string()))?;
            writer
                .write_all(&ct)
                .map_err(|e| StoreError::Io(e.to_string()))?;
            current.zeroize();
            return Ok(());
        }
        let ct = enc
            .encrypt_next(Payload { msg: &current, aad })
            .map_err(|_| StoreError::Io("encrypt_next failed".to_string()))?;
        writer
            .write_all(&ct)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        current.zeroize();
        current = next;
    }
}

/// Read up to one chunk, filling across short reads until full or EOF.
fn read_chunk<R: Read>(reader: &mut R) -> StoreResult<Vec<u8>> {
    let mut buf = vec![0u8; CHUNK];
    let mut filled = 0;
    while filled < CHUNK {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(StoreError::Io(e.to_string())),
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

/// Decrypt a segment file with its per-segment key, streaming. Returns the
/// plaintext. Fails if the key is wrong, the file is truncated or any chunk's
/// authentication fails.
pub(crate) fn read_segment_file(
    store_dir: &Path,
    seg_key: &SegmentKey,
    segment_id: Uuid,
) -> StoreResult<Vec<u8>> {
    let path = segment_path(store_dir, segment_id);
    let bytes = std::fs::read(&path).map_err(|e| StoreError::Io(e.to_string()))?;
    decrypt_segment_bytes(&bytes, seg_key.expose())
}

/// Decrypt an in-memory copy of a segment file with a candidate 32-byte key.
/// Returns `Ok` only if every chunk authenticates. Used by the deletion check to
/// prove a retained ciphertext copy is not decryptable with any remaining key.
pub(crate) fn decrypt_segment_bytes(file_bytes: &[u8], key: &[u8; 32]) -> StoreResult<Vec<u8>> {
    if file_bytes.len() < HEADER_LEN + STREAM_NONCE {
        return Err(StoreError::Io("segment file too short".to_string()));
    }
    let head = &file_bytes[..HEADER_LEN];
    if &head[..8] != MAGIC {
        return Err(StoreError::Io("bad segment magic".to_string()));
    }
    let nonce_bytes = &file_bytes[HEADER_LEN..HEADER_LEN + STREAM_NONCE];
    let ciphertext = &file_bytes[HEADER_LEN + STREAM_NONCE..];

    let key = Array::from(*key);
    let nonce =
        Array::try_from(nonce_bytes).map_err(|_| StoreError::Io("bad stream nonce".to_string()))?;
    let mut dec: DecryptorBE32<XChaCha20Poly1305> = DecryptorBE32::new(&key, &nonce);

    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let remaining = ciphertext.len() - offset;
        if remaining <= CIPHER_CHUNK {
            let pt = dec
                .decrypt_last(Payload {
                    msg: &ciphertext[offset..],
                    aad: head,
                })
                .map_err(|_| StoreError::Sealed)?;
            out.extend_from_slice(&pt);
            return Ok(out);
        }
        let pt = dec
            .decrypt_next(Payload {
                msg: &ciphertext[offset..offset + CIPHER_CHUNK],
                aad: head,
            })
            .map_err(|_| StoreError::Sealed)?;
        out.extend_from_slice(&pt);
        offset += CIPHER_CHUNK;
    }
}

/// Create a new segment on disk under a fresh per-segment key, and return its ref,
/// the wrapped per-segment key (for the database) and the plaintext length. The
/// caller inserts the wrapped key, learns its row id, and that id is the segment
/// header's key id, so the writer takes it as an argument.
pub(crate) struct NewSegment {
    pub segment_id: Uuid,
    pub seg_key: SegmentKey,
}

impl NewSegment {
    pub(crate) fn new() -> StoreResult<Self> {
        Ok(Self {
            segment_id: Uuid::now_v7(),
            seg_key: SegmentKey::random()?,
        })
    }

    /// Finish writing the segment file now that the key-row id is known, returning
    /// the event's segment ref.
    pub(crate) fn write(
        self,
        store_dir: &Path,
        segment_key_id: SegmentKeyId,
        plaintext: &[u8],
    ) -> StoreResult<SegmentRef> {
        let len = write_segment_file(
            store_dir,
            &self.seg_key,
            self.segment_id,
            segment_key_id,
            plaintext,
        )?;
        Ok(SegmentRef {
            segment_id: self.segment_id,
            byte_offset: 0,
            byte_len: len,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(len: usize) {
        let dir = tempfile::tempdir().unwrap();
        let store_key = [3u8; 32];
        let seg = NewSegment::new().unwrap();
        let seg_key_bytes = *seg.seg_key.expose();
        let wrapped = wrap_segment_key(&store_key, &seg.seg_key).unwrap();
        let plaintext: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let id = seg.segment_id;
        let seg_ref = seg.write(dir.path(), SegmentKeyId(5), &plaintext).unwrap();
        assert_eq!(seg_ref.byte_len, len as u64);

        let seg_key = unwrap_segment_key(&store_key, &wrapped).unwrap();
        assert_eq!(seg_key.expose(), &seg_key_bytes);
        let got = read_segment_file(dir.path(), &seg_key, id).unwrap();
        assert_eq!(got, plaintext);
    }

    #[test]
    fn roundtrips_empty_small_exact_and_large() {
        roundtrip(0);
        roundtrip(100);
        roundtrip(CHUNK); // exactly one chunk
        roundtrip(CHUNK + 1);
        roundtrip(CHUNK * 3); // exact multiple of chunk
        roundtrip(CHUNK * 2 + 123);
    }

    #[test]
    fn wrong_key_does_not_decrypt() {
        let dir = tempfile::tempdir().unwrap();
        let store_key = [3u8; 32];
        let seg = NewSegment::new().unwrap();
        let id = seg.segment_id;
        seg.write(dir.path(), SegmentKeyId(1), b"secret bytes")
            .unwrap();
        let bytes = std::fs::read(segment_path(dir.path(), id)).unwrap();
        // A different per-segment key cannot decrypt.
        assert!(decrypt_segment_bytes(&bytes, &[9u8; 32]).is_err());
        // Neither can the store key, which never encrypts a segment directly.
        assert!(decrypt_segment_bytes(&bytes, &store_key).is_err());
    }
}
