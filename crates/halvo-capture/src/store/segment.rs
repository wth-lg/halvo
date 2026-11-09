//! Per-segment XChaCha20-Poly1305 file writer and reader.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use aead::array::typenum::Unsigned;
use aead::{AeadCore, Payload};
use aead_stream::{DecryptorBE32, EncryptorBE32, NonceSize, StreamBE32};
use chacha20poly1305::aead::array::Array;
use chacha20poly1305::XChaCha20Poly1305;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::custody::dev_fallback::{aead_open, aead_seal, random_key};
use crate::event::SegmentKeyId;

use super::{StoreError, StoreResult};

pub(crate) const CHUNK: usize = 64 * 1024;
const TAG: usize = <XChaCha20Poly1305 as AeadCore>::TagSize::USIZE;
const CIPHER_CHUNK: usize = CHUNK + TAG;
const STREAM_NONCE: usize = NonceSize::<XChaCha20Poly1305, StreamBE32<XChaCha20Poly1305>>::USIZE;
const MAGIC: [u8; 7] = *b"HLVOSEG";
const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = MAGIC.len() + size_of::<u8>() + size_of::<Uuid>() + size_of::<i64>();
const SEGMENT_KEY_AAD: &[u8] = b"halvo-segment-key";

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

pub(crate) fn wrap_segment_key(store_key: &[u8; 32], seg_key: &SegmentKey) -> StoreResult<Vec<u8>> {
    aead_seal(store_key, seg_key.expose(), SEGMENT_KEY_AAD)
        .map_err(|_| StoreError::Io("sealing the segment key failed".to_string()))
}

pub(crate) fn unwrap_segment_key(store_key: &[u8; 32], wrapped: &[u8]) -> StoreResult<SegmentKey> {
    let plain = aead_open(store_key, wrapped, SEGMENT_KEY_AAD).map_err(|_| StoreError::Sealed)?;
    let arr: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| StoreError::Io("segment key is not 32 bytes".to_string()))?;
    Ok(SegmentKey::from_bytes(arr))
}

fn segments_root(store_dir: &Path) -> PathBuf {
    store_dir.join("segments")
}

pub(crate) fn segment_path(store_dir: &Path, segment_id: Uuid) -> PathBuf {
    let id = segment_id.simple().to_string();
    let shard = &id[..2];
    segments_root(store_dir)
        .join(shard)
        .join(format!("{id}.seg"))
}

pub(crate) struct SegmentHeader {
    pub segment_id: Uuid,
    pub segment_key_id: SegmentKeyId,
}

impl SegmentHeader {
    fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut bytes = [0u8; HEADER_LEN];
        let (magic, rest) = bytes.split_at_mut(MAGIC.len());
        magic.copy_from_slice(&MAGIC);
        let (version, rest) = rest.split_at_mut(size_of::<u8>());
        version[0] = FORMAT_VERSION;
        let (segment_id, segment_key_id) = rest.split_at_mut(size_of::<Uuid>());
        segment_id.copy_from_slice(self.segment_id.as_bytes());
        segment_key_id.copy_from_slice(&self.segment_key_id.0.to_le_bytes());
        bytes
    }
}

fn check_header(bytes: &[u8; HEADER_LEN]) -> StoreResult<()> {
    let (magic, rest) = bytes.split_at(MAGIC.len());
    if magic != MAGIC {
        return Err(StoreError::Io("bad segment magic".to_string()));
    }
    if rest[0] != FORMAT_VERSION {
        return Err(StoreError::Io(format!(
            "unsupported segment format version {}",
            rest[0]
        )));
    }
    Ok(())
}

fn io_error(e: io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

struct Chunks<'a> {
    reader: &'a mut dyn Read,
    chunk_len: usize,
    lookahead: Option<Zeroizing<Vec<u8>>>,
}

impl<'a> Chunks<'a> {
    fn new(reader: &'a mut dyn Read, chunk_len: usize) -> Self {
        Self {
            reader,
            chunk_len,
            lookahead: None,
        }
    }

    fn next(&mut self) -> StoreResult<(Zeroizing<Vec<u8>>, bool)> {
        let current = match self.lookahead.take() {
            Some(chunk) => chunk,
            None => self.read_chunk()?,
        };
        if current.len() < self.chunk_len {
            return Ok((current, true));
        }
        let next = self.read_chunk()?;
        let is_last = next.is_empty();
        if !is_last {
            self.lookahead = Some(next);
        }
        Ok((current, is_last))
    }

    fn read_chunk(&mut self) -> StoreResult<Zeroizing<Vec<u8>>> {
        let mut buf = Zeroizing::new(vec![0u8; self.chunk_len]);
        let mut filled = 0;
        while filled < self.chunk_len {
            match self.reader.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(io_error(e)),
            }
        }
        buf.truncate(filled);
        Ok(buf)
    }
}

pub(crate) fn encrypt_segment(
    plaintext: &mut dyn Read,
    out: &mut dyn Write,
    key: &SegmentKey,
    header: &SegmentHeader,
) -> StoreResult<u64> {
    let head = header.to_bytes();
    let mut nonce_bytes = [0u8; STREAM_NONCE];
    getrandom::getrandom(&mut nonce_bytes).map_err(|e| StoreError::Io(e.to_string()))?;
    out.write_all(&head)
        .and_then(|()| out.write_all(&nonce_bytes))
        .map_err(io_error)?;

    let nonce = Array::try_from(&nonce_bytes[..])
        .map_err(|_| StoreError::Io("bad stream nonce".to_string()))?;
    let mut encryptor: EncryptorBE32<XChaCha20Poly1305> =
        EncryptorBE32::new(&Array::from(*key.expose()), &nonce);
    let mut chunks = Chunks::new(plaintext, CHUNK);
    let mut total = 0u64;
    loop {
        let (chunk, is_last) = chunks.next()?;
        total += chunk.len() as u64;
        let payload = Payload {
            msg: &chunk,
            aad: &head,
        };
        if is_last {
            emit_ciphertext(out, encryptor.encrypt_last(payload))?;
            return Ok(total);
        }
        emit_ciphertext(out, encryptor.encrypt_next(payload))?;
    }
}

fn emit_ciphertext(out: &mut dyn Write, sealed: Result<Vec<u8>, aead::Error>) -> StoreResult<()> {
    let ciphertext = sealed.map_err(|_| StoreError::Io("segment encryption failed".to_string()))?;
    out.write_all(&ciphertext).map_err(io_error)
}

pub(crate) fn decrypt_segment(
    file: &mut dyn Read,
    out: &mut dyn Write,
    key: &SegmentKey,
) -> StoreResult<u64> {
    let mut head = [0u8; HEADER_LEN];
    let mut nonce_bytes = [0u8; STREAM_NONCE];
    file.read_exact(&mut head)
        .and_then(|()| file.read_exact(&mut nonce_bytes))
        .map_err(|_| StoreError::Io("segment file too short".to_string()))?;
    check_header(&head)?;

    let nonce = Array::try_from(&nonce_bytes[..])
        .map_err(|_| StoreError::Io("bad stream nonce".to_string()))?;
    let mut decryptor: DecryptorBE32<XChaCha20Poly1305> =
        DecryptorBE32::new(&Array::from(*key.expose()), &nonce);
    let mut chunks = Chunks::new(file, CIPHER_CHUNK);
    let mut total = 0u64;
    loop {
        let (chunk, is_last) = chunks.next()?;
        let payload = Payload {
            msg: &chunk,
            aad: &head,
        };
        if is_last {
            total += emit_plaintext(out, decryptor.decrypt_last(payload))?;
            return Ok(total);
        }
        total += emit_plaintext(out, decryptor.decrypt_next(payload))?;
    }
}

fn emit_plaintext(out: &mut dyn Write, opened: Result<Vec<u8>, aead::Error>) -> StoreResult<u64> {
    let plaintext = Zeroizing::new(opened.map_err(|_| StoreError::Sealed)?);
    out.write_all(&plaintext).map_err(io_error)?;
    Ok(plaintext.len() as u64)
}

pub(crate) fn write_segment_file(
    store_dir: &Path,
    seg_key: &SegmentKey,
    header: &SegmentHeader,
    plaintext: &mut dyn Read,
) -> StoreResult<u64> {
    let path = segment_path(store_dir, header.segment_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    let file = std::fs::File::create(&path).map_err(io_error)?;
    let mut writer = io::BufWriter::new(file);
    let written = encrypt_segment(plaintext, &mut writer, seg_key, header)?;
    writer.flush().map_err(io_error)?;
    Ok(written)
}

pub(crate) fn read_segment_file(
    store_dir: &Path,
    seg_key: &SegmentKey,
    segment_id: Uuid,
    out: &mut dyn Write,
) -> StoreResult<u64> {
    let file = std::fs::File::open(segment_path(store_dir, segment_id)).map_err(io_error)?;
    let mut reader = io::BufReader::new(file);
    decrypt_segment(&mut reader, out, seg_key)
}

#[cfg(test)]
fn decrypt_segment_bytes(file_bytes: &[u8], key: &SegmentKey) -> StoreResult<Vec<u8>> {
    let mut plaintext = Vec::new();
    decrypt_segment(&mut io::Cursor::new(file_bytes), &mut plaintext, key)?;
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(len: usize) {
        let dir = tempfile::tempdir().unwrap();
        let store_key = [3u8; 32];
        let seg_key = SegmentKey::random().unwrap();
        let seg_key_bytes = *seg_key.expose();
        let wrapped = wrap_segment_key(&store_key, &seg_key).unwrap();
        let plaintext: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let header = SegmentHeader {
            segment_id: Uuid::now_v7(),
            segment_key_id: SegmentKeyId(5),
        };
        let written = write_segment_file(
            dir.path(),
            &seg_key,
            &header,
            &mut io::Cursor::new(&plaintext),
        )
        .unwrap();
        assert_eq!(written, len as u64);

        let seg_key = unwrap_segment_key(&store_key, &wrapped).unwrap();
        assert_eq!(seg_key.expose(), &seg_key_bytes);
        let mut got = Vec::new();
        let read = read_segment_file(dir.path(), &seg_key, header.segment_id, &mut got).unwrap();
        assert_eq!(read, len as u64);
        assert_eq!(got, plaintext);
    }

    #[test]
    fn roundtrips_empty_small_exact_and_large() {
        roundtrip(0);
        roundtrip(100);
        roundtrip(CHUNK);
        roundtrip(CHUNK + 1);
        roundtrip(CHUNK * 3);
        roundtrip(CHUNK * 2 + 123);
    }

    #[test]
    fn wrong_key_does_not_decrypt() {
        let dir = tempfile::tempdir().unwrap();
        let seg_key = SegmentKey::random().unwrap();
        let header = SegmentHeader {
            segment_id: Uuid::now_v7(),
            segment_key_id: SegmentKeyId(1),
        };
        write_segment_file(
            dir.path(),
            &seg_key,
            &header,
            &mut io::Cursor::new(b"secret bytes"),
        )
        .unwrap();
        let bytes = std::fs::read(segment_path(dir.path(), header.segment_id)).unwrap();
        assert!(bytes.starts_with(&MAGIC));
        assert_eq!(bytes[MAGIC.len()], FORMAT_VERSION);
        assert!(!bytes.windows(12).any(|w| w == b"secret bytes"));
        assert!(matches!(
            decrypt_segment_bytes(&bytes, &SegmentKey::from_bytes([9u8; 32])),
            Err(StoreError::Sealed)
        ));
        assert_eq!(
            decrypt_segment_bytes(&bytes, &seg_key).unwrap(),
            b"secret bytes"
        );
    }
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Trace {
        bytes_read: usize,
        bytes_written: usize,
        max_outstanding: usize,
    }

    struct TrackingReader {
        inner: io::Cursor<Vec<u8>>,
        trace: Rc<RefCell<Trace>>,
    }

    impl Read for TrackingReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(buf)?;
            let mut t = self.trace.borrow_mut();
            t.bytes_read += n;
            let outstanding = t.bytes_read.saturating_sub(t.bytes_written);
            t.max_outstanding = t.max_outstanding.max(outstanding);
            Ok(n)
        }
    }

    struct TrackingWriter {
        inner: Vec<u8>,
        trace: Rc<RefCell<Trace>>,
    }

    impl Write for TrackingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.inner.extend_from_slice(buf);
            self.trace.borrow_mut().bytes_written += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    const STREAMING_BOUND: usize = 3 * CHUNK;

    fn traced(input: Vec<u8>) -> (TrackingReader, TrackingWriter, Rc<RefCell<Trace>>) {
        let trace = Rc::new(RefCell::new(Trace::default()));
        let reader = TrackingReader {
            inner: io::Cursor::new(input),
            trace: Rc::clone(&trace),
        };
        let writer = TrackingWriter {
            inner: Vec::new(),
            trace: Rc::clone(&trace),
        };
        (reader, writer, trace)
    }

    #[test]
    fn a_segment_larger_than_one_chunk_streams_in_both_directions() {
        let plaintext: Vec<u8> = (0..CHUNK * 8 + 123).map(|i| (i % 251) as u8).collect();
        let key = SegmentKey::from_bytes([5u8; 32]);
        let header = SegmentHeader {
            segment_id: Uuid::now_v7(),
            segment_key_id: SegmentKeyId(9),
        };

        let (mut reader, mut writer, trace) = traced(plaintext.clone());
        let written = encrypt_segment(&mut reader, &mut writer, &key, &header).unwrap();
        assert_eq!(written, plaintext.len() as u64);
        let outstanding = trace.borrow().max_outstanding;
        assert!(
            outstanding <= STREAMING_BOUND,
            "encrypt held {outstanding} unwritten plaintext bytes, bound {STREAMING_BOUND}"
        );
        let ciphertext = writer.inner;

        let (mut reader, mut writer, trace) = traced(ciphertext);
        let read = decrypt_segment(&mut reader, &mut writer, &key).unwrap();
        assert_eq!(read, plaintext.len() as u64);
        assert_eq!(writer.inner, plaintext);
        let outstanding = trace.borrow().max_outstanding;
        assert!(
            outstanding <= STREAMING_BOUND,
            "decrypt held {outstanding} unwritten ciphertext bytes, bound {STREAMING_BOUND}"
        );
    }
}
