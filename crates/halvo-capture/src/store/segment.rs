//! Per-segment XChaCha20-Poly1305 file writer and reader.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use aead::array::typenum::Unsigned;
use aead::{AeadCore, Payload};
use aead_stream::{DecryptorBE32, EncryptorBE32, NonceSize, StreamBE32};
use chacha20poly1305::aead::array::Array;
use chacha20poly1305::XChaCha20Poly1305;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::custody::dev_fallback::{aead_open, aead_seal};
use crate::custody::wiped_on_drop;
use crate::event::SegmentKeyId;

use super::durable::{self, DurableFs};
use super::{StoreError, StoreResult};

pub(crate) const CHUNK: usize = 64 * 1024;
const TAG: usize = <XChaCha20Poly1305 as AeadCore>::TagSize::USIZE;
const CIPHER_CHUNK: usize = CHUNK + TAG;
const STREAM_NONCE: usize = NonceSize::<XChaCha20Poly1305, StreamBE32<XChaCha20Poly1305>>::USIZE;
const MAGIC: [u8; 7] = *b"HLVOSEG";
const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = MAGIC.len() + size_of::<u8>() + size_of::<Uuid>() + size_of::<i64>();
const SEGMENT_KEY_AAD: &[u8] = b"halvo-segment-key";
const SEGMENT_EXT: &str = "seg";
const PARTIAL_EXT: &str = "seg.tmp";

#[derive(Zeroize, ZeroizeOnDrop)]
pub(crate) struct SegmentKey(Box<[u8; 32]>);

const _: () = wiped_on_drop::<SegmentKey>();

impl SegmentKey {
    pub(crate) fn random() -> StoreResult<Self> {
        let mut key = Self(Box::new([0u8; 32]));
        getrandom::getrandom(key.0.as_mut_slice()).map_err(rng_error)?;
        Ok(key)
    }
    pub(crate) fn expose(&self) -> &[u8; 32] {
        &self.0
    }
    fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Box::new(bytes))
    }
}

pub(crate) fn wrap_segment_key(store_key: &[u8; 32], seg_key: &SegmentKey) -> StoreResult<Vec<u8>> {
    aead_seal(store_key, seg_key.expose(), SEGMENT_KEY_AAD).map_err(|_| StoreError::Seal)
}

pub(crate) fn unwrap_segment_key(store_key: &[u8; 32], wrapped: &[u8]) -> StoreResult<SegmentKey> {
    let plain = aead_open(store_key, wrapped, SEGMENT_KEY_AAD)
        .map_err(|_| StoreError::Corrupt("the segment key does not unwrap".to_string()))?;
    if plain.len() != 32 {
        return Err(StoreError::Corrupt(
            "segment key is not 32 bytes".to_string(),
        ));
    }
    let mut key = SegmentKey::from_bytes([0u8; 32]);
    key.0.copy_from_slice(&plain);
    Ok(key)
}

pub(crate) fn segments_root(store_dir: &Path) -> PathBuf {
    store_dir.join("segments")
}

pub(crate) fn segment_path(store_dir: &Path, segment_id: Uuid) -> PathBuf {
    let id = segment_id.simple().to_string();
    let shard = &id[..2];
    segments_root(store_dir)
        .join(shard)
        .join(format!("{id}.{SEGMENT_EXT}"))
}

pub(crate) enum SegmentFile {
    Segment { id: Uuid, path: PathBuf },
    Other(PathBuf),
}

impl SegmentFile {
    pub(crate) fn path(&self) -> &Path {
        match self {
            SegmentFile::Segment { path, .. } | SegmentFile::Other(path) => path,
        }
    }

    fn classify(path: PathBuf) -> Self {
        let id = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(&format!(".{SEGMENT_EXT}")))
            .and_then(|stem| Uuid::parse_str(stem).ok());
        match id {
            Some(id) => SegmentFile::Segment { id, path },
            None => SegmentFile::Other(path),
        }
    }
}

pub(crate) fn segment_files(store_dir: &Path) -> StoreResult<Vec<SegmentFile>> {
    let mut files = Vec::new();
    let mut stack = vec![segments_root(store_dir)];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io_error(e)),
        };
        for entry in entries {
            let path = entry.map_err(io_error)?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(SegmentFile::classify(path));
            }
        }
    }
    Ok(files)
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
        return Err(StoreError::Corrupt("bad segment magic".to_string()));
    }
    if rest[0] != FORMAT_VERSION {
        return Err(StoreError::Corrupt(format!(
            "unsupported segment format version {}",
            rest[0]
        )));
    }
    Ok(())
}

fn io_error(e: io::Error) -> StoreError {
    StoreError::Io(e.kind())
}

fn rng_error(e: getrandom::Error) -> StoreError {
    StoreError::Io(e.raw_os_error().map_or(io::ErrorKind::Other, |code| {
        io::Error::from_raw_os_error(code).kind()
    }))
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
    getrandom::getrandom(&mut nonce_bytes).map_err(rng_error)?;
    out.write_all(&head)
        .and_then(|()| out.write_all(&nonce_bytes))
        .map_err(io_error)?;

    let nonce = Array::try_from(&nonce_bytes[..]).map_err(|_| StoreError::Seal)?;
    let mut encryptor: EncryptorBE32<XChaCha20Poly1305> =
        EncryptorBE32::new(key.expose().into(), &nonce);
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
    let ciphertext = sealed.map_err(|_| StoreError::Seal)?;
    out.write_all(&ciphertext).map_err(io_error)
}

fn decrypt_segment(file: &mut dyn Read, out: &mut dyn Write, key: &SegmentKey) -> StoreResult<u64> {
    let mut head = [0u8; HEADER_LEN];
    let mut nonce_bytes = [0u8; STREAM_NONCE];
    file.read_exact(&mut head)
        .and_then(|()| file.read_exact(&mut nonce_bytes))
        .map_err(|e| match e.kind() {
            io::ErrorKind::UnexpectedEof => {
                StoreError::Corrupt("segment file too short".to_string())
            }
            _ => io_error(e),
        })?;
    check_header(&head)?;

    let nonce = Array::try_from(&nonce_bytes[..])
        .map_err(|_| StoreError::Corrupt("bad stream nonce".to_string()))?;
    let mut decryptor: DecryptorBE32<XChaCha20Poly1305> =
        DecryptorBE32::new(key.expose().into(), &nonce);
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
    let plaintext = Zeroizing::new(
        opened.map_err(|_| StoreError::Corrupt("segment did not authenticate".to_string()))?,
    );
    out.write_all(&plaintext).map_err(io_error)?;
    Ok(plaintext.len() as u64)
}

pub(crate) fn write_segment_file(
    store_dir: &Path,
    fs: &dyn DurableFs,
    seg_key: &SegmentKey,
    header: &SegmentHeader,
    plaintext: &mut dyn Read,
) -> StoreResult<u64> {
    let path = segment_path(store_dir, header.segment_id);
    let partial = path.with_extension(PARTIAL_EXT);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    durable::write_durably(
        fs,
        &partial,
        &path,
        io_error,
        |partial| {
            let _ = std::fs::remove_file(partial);
            Ok(())
        },
        |out| encrypt_segment(plaintext, out, seg_key, header),
    )
}

pub(crate) fn read_segment_file(
    store_dir: &Path,
    seg_key: &SegmentKey,
    segment_id: Uuid,
    out: &mut dyn Write,
) -> StoreResult<u64> {
    let file =
        std::fs::File::open(segment_path(store_dir, segment_id)).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => StoreError::SegmentMissing(segment_id),
            _ => io_error(e),
        })?;
    let mut reader = io::BufReader::new(file);
    decrypt_segment(&mut reader, &mut io::sink(), seg_key)?;
    reader.seek(SeekFrom::Start(0)).map_err(io_error)?;
    decrypt_segment(&mut reader, out, seg_key)
}

pub(crate) fn decrypt_segment_bytes(file_bytes: &[u8], key: &SegmentKey) -> StoreResult<Vec<u8>> {
    let mut plaintext = Vec::new();
    decrypt_segment(&mut io::Cursor::new(file_bytes), &mut plaintext, key)?;
    Ok(plaintext)
}

pub(crate) fn remove_segment_file(fs: &dyn DurableFs, path: &Path) -> StoreResult<()> {
    durable::remove_durably(fs, path).map_err(io_error)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const HEADER: usize = HEADER_LEN;
    pub(crate) const BODY: usize = HEADER_LEN + STREAM_NONCE;
    pub(crate) const TAG_LEN: usize = TAG;
    pub(crate) const CIPHER_CHUNK_LEN: usize = CIPHER_CHUNK;

    const GOLDEN: &[u8] = include_bytes!("testdata/segment-v1.golden");
    const GOLDEN_KEY: [u8; 32] = [0x5e; 32];

    fn golden_plaintext() -> Vec<u8> {
        (0..300u32).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn nonce_hex(sealed: &[u8]) -> String {
        sealed[..24].iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn wrap_nonces_for_parent() {
        if std::env::var("HALVO_TEST_WRAP_NONCES").is_err() {
            println!(
                "wrap_nonces_for_parent: HALVO_TEST_WRAP_NONCES is not set, so no parent asked for nonces"
            );
            return;
        }
        let segment_key_wrap =
            wrap_segment_key(&[0x42; 32], &SegmentKey::random().unwrap()).unwrap();
        println!("segment-key wrap nonce {}", nonce_hex(&segment_key_wrap));
        let roots_dir = tempfile::tempdir().unwrap();
        let roots = crate::custody::file_root::FileRoots::new(
            roots_dir.path().to_path_buf(),
            crate::custody::file_root::test_support::TestProtector::new(0x5a).0,
        );
        let root_id = crate::custody::RootId::fresh();
        roots.replace_root(root_id, &[0x24; 32]).unwrap();
        let store_key_wrap = roots
            .wrap(root_id, &crate::custody::StoreKey::from_bytes([0x42; 32]))
            .unwrap();
        println!("store-key wrap nonce {}", nonce_hex(&store_key_wrap.0));
    }

    fn wrap_nonces_of_a_child_process() -> (String, String) {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store::segment::tests::wrap_nonces_for_parent",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HALVO_TEST_WRAP_NONCES", "1")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "the child failed: {text}");
        let nonce = |marker: &str| {
            text.lines()
                .find_map(|line| line.split_once(marker))
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .unwrap_or_else(|| panic!("the child printed no {marker:?}: {text}"))
                .to_string()
        };
        (
            nonce("segment-key wrap nonce "),
            nonce("store-key wrap nonce "),
        )
    }

    #[test]
    fn two_processes_that_wrap_under_one_key_draw_two_nonces() {
        let first = wrap_nonces_of_a_child_process();
        let second = wrap_nonces_of_a_child_process();
        assert_ne!(
            first.0, second.0,
            "two processes drew one nonce for a segment-key wrap under one store key"
        );
        assert_ne!(
            first.1, second.1,
            "two processes drew one nonce for a store-key wrap under one root"
        );
    }

    fn seal_a_segment_key(store_key: &[u8; 32], seg_key: &[u8; 32], label: &[u8]) -> Vec<u8> {
        use aead::{Aead, KeyInit};

        let nonce = [0x19u8; 24];
        let cipher = XChaCha20Poly1305::new(store_key.into());
        let sealed = cipher
            .encrypt(
                &chacha20poly1305::XNonce::from(nonce),
                Payload {
                    msg: seg_key,
                    aad: label,
                },
            )
            .unwrap();
        [nonce.as_slice(), &sealed].concat()
    }

    const A_WRAP_UNDER_THE_LABEL: &str = concat!(
        "191919191919191919191919191919191919191919191919",
        "5d39fb6accf83487449a704bce9b15ec898e476744abeb99687e75ca35a2f38b",
        "6935d051c9dde65d3301ebc7244fd8a3"
    );

    #[test]
    fn a_segment_key_wrap_opens_only_under_its_label() {
        let (store_key, seg_key) = ([0x6eu8; 32], [0x2du8; 32]);
        let with_the_label = seal_a_segment_key(&store_key, &seg_key, b"halvo-segment-key");
        let hex: String = with_the_label.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, A_WRAP_UNDER_THE_LABEL, "the fixed wrap changed");
        let opened = unwrap_segment_key(&store_key, &with_the_label);
        assert!(
            opened.as_ref().is_ok_and(|key| key.expose() == &seg_key),
            "the fixed wrap under the label did not open: {:?}",
            opened.err()
        );
        for (case, label) in [
            ("no label", b"".as_slice()),
            ("another label", b"halvo-segment-key-2".as_slice()),
        ] {
            let other = seal_a_segment_key(&store_key, &seg_key, label);
            let opened = unwrap_segment_key(&store_key, &other);
            assert!(
                matches!(opened, Err(StoreError::Corrupt(_))),
                "the key sealed under the store key with {case} opened as a segment key"
            );
        }
    }

    #[test]
    fn segment_key_wraps_under_one_store_key_use_distinct_nonces_that_vary_in_every_byte() {
        const XNONCE: usize = 24;
        const WRAPS: usize = 64;
        let store_key = [0x42u8; 32];
        let nonces: Vec<Vec<u8>> = (0..WRAPS)
            .map(|_| {
                wrap_segment_key(&store_key, &SegmentKey::random().unwrap()).unwrap()[..XNONCE]
                    .to_vec()
            })
            .collect();
        let distinct: std::collections::HashSet<&Vec<u8>> = nonces.iter().collect();
        assert_eq!(distinct.len(), WRAPS, "segment-key wraps repeated a nonce");
        for position in 0..XNONCE {
            assert!(
                nonces
                    .iter()
                    .any(|nonce| nonce[position] != nonces[0][position]),
                "byte {position} of the nonce took one value in {WRAPS} wraps"
            );
        }
    }

    #[test]
    fn a_seal_that_fails_is_a_seal_error_and_no_io_error() {
        let emitted = emit_ciphertext(&mut Vec::new(), Err(aead::Error));
        assert!(matches!(emitted, Err(StoreError::Seal)), "{emitted:?}");
    }

    #[test]
    fn the_golden_segment_of_format_version_1_decrypts_to_its_plaintext() {
        let mut head = Vec::new();
        head.extend_from_slice(b"HLVOSEG");
        head.push(1);
        head.extend_from_slice(&[0x11; 16]);
        head.extend_from_slice(&7i64.to_le_bytes());
        assert_eq!(&GOLDEN[..HEADER_LEN], head.as_slice());
        assert_eq!(GOLDEN.len(), BODY + 300 + TAG);
        let plain = decrypt_segment_bytes(GOLDEN, &SegmentKey::from_bytes(GOLDEN_KEY)).unwrap();
        assert_eq!(plain, golden_plaintext());
    }

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
            &crate::store::durable::OsFs,
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
            &crate::store::durable::OsFs,
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
            Err(StoreError::Corrupt(_))
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
