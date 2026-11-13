use std::io::{ErrorKind, Read, Write};

pub const REQUEST_MAX_BYTES: usize = 4_096;

pub const REPLY_MAX_BYTES: usize = 262_144;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("the frame announces {len} bytes, above the limit of {max}")]
    TooLong { len: u64, max: usize },
    #[error("the frame announces no bytes")]
    Empty,
    #[error("the stream ended inside a frame")]
    Truncated,
    #[error("bytes follow the frame")]
    Trailing,
    #[error("the transport failed: {0}")]
    Io(std::io::Error),
}

/// Writes one frame: the length of `body` as a big-endian u32, then `body`. Refuses an
/// empty body and one longer than `max` before it writes a byte.
pub fn write_frame<W: Write>(writer: &mut W, body: &[u8], max: usize) -> Result<(), FrameError> {
    if body.is_empty() {
        return Err(FrameError::Empty);
    }
    if body.len() > max {
        return Err(FrameError::TooLong {
            len: body.len() as u64,
            max,
        });
    }
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLong {
        len: body.len() as u64,
        max,
    })?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(body);
    writer.write_all(&frame).map_err(FrameError::Io)?;
    writer.flush().map_err(FrameError::Io)
}

/// Reads one frame and checks its length against `max` before it allocates for the body.
pub fn read_frame<R: Read>(reader: &mut R, max: usize) -> Result<Vec<u8>, FrameError> {
    let mut prefix = [0u8; 4];
    read_exactly(reader, &mut prefix)?;
    let len = u32::from_be_bytes(prefix);
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if u64::from(len) > max as u64 {
        return Err(FrameError::TooLong {
            len: u64::from(len),
            max,
        });
    }
    let mut body = vec![0u8; len as usize];
    read_exactly(reader, &mut body)?;
    Ok(body)
}

/// Reads the one frame of a connection and requires the stream to end after it.
pub fn read_one_frame<R: Read>(reader: &mut R, max: usize) -> Result<Vec<u8>, FrameError> {
    let body = read_frame(reader, max)?;
    let mut probe = [0u8; 1];
    loop {
        match reader.read(&mut probe) {
            Ok(0) => return Ok(body),
            Ok(_) => return Err(FrameError::Trailing),
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(FrameError::Io(e)),
        }
    }
}

fn read_exactly<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<(), FrameError> {
    reader.read_exact(buf).map_err(|e| match e.kind() {
        ErrorKind::UnexpectedEof => FrameError::Truncated,
        _ => FrameError::Io(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn a_frame_is_a_big_endian_length_then_the_body() {
        let mut out = Vec::new();
        write_frame(&mut out, b"{}", REQUEST_MAX_BYTES).unwrap();
        assert_eq!(out, [0, 0, 0, 2, b'{', b'}']);
        assert_eq!(
            read_one_frame(&mut &out[..], REQUEST_MAX_BYTES).unwrap(),
            b"{}"
        );
    }

    #[test]
    fn the_limits_are_four_kib_for_a_request_and_256_kib_for_a_reply() {
        assert_eq!(REQUEST_MAX_BYTES, 4_096);
        assert_eq!(REPLY_MAX_BYTES, 262_144);
    }

    #[test]
    fn a_frame_at_the_limit_passes_and_one_byte_over_is_refused_on_both_sides() {
        for max in [REQUEST_MAX_BYTES, REPLY_MAX_BYTES] {
            let body = vec![b'a'; max];
            let mut out = Vec::new();
            write_frame(&mut out, &body, max).unwrap();
            assert_eq!(read_one_frame(&mut &out[..], max).unwrap(), body);

            let over = vec![b'a'; max + 1];
            let mut nothing = Vec::new();
            assert!(matches!(
                write_frame(&mut nothing, &over, max),
                Err(FrameError::TooLong { len, max: m }) if len == max as u64 + 1 && m == max
            ));
            assert!(nothing.is_empty(), "a refused frame wrote bytes");
            assert!(matches!(
                read_one_frame(&mut &framed(&over)[..], max),
                Err(FrameError::TooLong { .. })
            ));
        }
    }

    #[test]
    fn a_zero_length_a_truncated_frame_and_trailing_bytes_are_errors() {
        assert!(matches!(
            read_one_frame(&mut &[0u8, 0, 0, 0][..], REQUEST_MAX_BYTES),
            Err(FrameError::Empty)
        ));
        let mut cut = framed(b"{\"op\":\"pause\"}");
        cut.pop();
        assert!(matches!(
            read_one_frame(&mut &cut[..], REQUEST_MAX_BYTES),
            Err(FrameError::Truncated)
        ));
        assert!(matches!(
            read_one_frame(&mut &[0u8, 0][..], REQUEST_MAX_BYTES),
            Err(FrameError::Truncated)
        ));
        assert!(matches!(
            read_one_frame(&mut &[][..], REQUEST_MAX_BYTES),
            Err(FrameError::Truncated)
        ));
        let mut more = framed(b"{}");
        more.push(b'x');
        assert!(matches!(
            read_one_frame(&mut &more[..], REQUEST_MAX_BYTES),
            Err(FrameError::Trailing)
        ));
        let mut two = framed(b"{}");
        two.extend(framed(b"{}"));
        assert!(matches!(
            read_one_frame(&mut &two[..], REQUEST_MAX_BYTES),
            Err(FrameError::Trailing)
        ));
    }

    #[test]
    fn an_empty_body_is_never_written() {
        let mut out = Vec::new();
        assert!(matches!(
            write_frame(&mut out, b"", REQUEST_MAX_BYTES),
            Err(FrameError::Empty)
        ));
        assert!(out.is_empty());
    }

    struct Countdown<'a> {
        bytes: &'a [u8],
        reads_left: usize,
    }

    impl std::io::Read for Countdown<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            assert!(
                self.reads_left > 0,
                "the decoder read after a refused length"
            );
            self.reads_left -= 1;
            std::io::Read::read(&mut self.bytes, buf)
        }
    }

    #[test]
    fn a_length_over_the_limit_is_refused_before_any_body_is_read() {
        for len in [REQUEST_MAX_BYTES as u32 + 1, u32::MAX] {
            let prefix = len.to_be_bytes();
            let mut reader = Countdown {
                bytes: &prefix,
                reads_left: 4,
            };
            assert!(matches!(
                read_frame(&mut reader, REQUEST_MAX_BYTES),
                Err(FrameError::TooLong { len: l, .. }) if l == u64::from(len)
            ));
        }
    }

    #[test]
    fn a_reader_that_hands_one_byte_at_a_time_gives_the_same_frame() {
        struct OneByte<'a>(&'a [u8]);
        impl std::io::Read for OneByte<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = usize::from(!self.0.is_empty() && !buf.is_empty());
                buf[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let bytes = framed(b"{\"op\":\"stop\"}");
        assert_eq!(
            read_one_frame(&mut OneByte(&bytes), REQUEST_MAX_BYTES).unwrap(),
            b"{\"op\":\"stop\"}"
        );
    }

    #[test]
    fn a_timeout_of_the_transport_is_an_io_error_and_not_a_truncation() {
        struct Silent;
        impl std::io::Read for Silent {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::TimedOut.into())
            }
        }
        match read_one_frame(&mut Silent, REQUEST_MAX_BYTES) {
            Err(FrameError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::TimedOut),
            other => panic!("{other:?}"),
        }
    }

    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Largest;

    static LARGEST: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        static TRACKED: Cell<bool> = const { Cell::new(false) };
    }

    // SAFETY: every call is passed on to the system allocator unchanged; the only addition
    // reads a thread-local flag and stores a size.
    unsafe impl GlobalAlloc for Largest {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if TRACKED.with(Cell::get) {
                LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
            }
            // SAFETY: the caller's layout goes to the system allocator as it came.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            if TRACKED.with(Cell::get) {
                LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
            }
            // SAFETY: the caller's layout goes to the system allocator as it came.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: the block is one that this allocator handed out through alloc, alloc_zeroed
            // or realloc, each of which passes to System, and `layout` is the one that the block
            // has now.
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            if TRACKED.with(Cell::get) {
                LARGEST.fetch_max(new_size, Ordering::Relaxed);
            }
            // SAFETY: the pointer and layout came from this allocator and go back to it.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Largest = Largest;

    struct Draw(u64);

    impl Draw {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic_the_decoder_or_allocate_above_the_limit() {
        let mut draw = Draw(0x9e37_79b9_7f4a_7c15);
        let mut inputs: Vec<Vec<u8>> = Vec::new();
        for _ in 0..4_000 {
            let len = (draw.next() % 64) as usize;
            inputs.push((0..len).map(|_| draw.next() as u8).collect());
        }
        for _ in 0..1_000 {
            let announced = match draw.next() % 4 {
                0 => (draw.next() % 5_000) as u32,
                1 => REQUEST_MAX_BYTES as u32 + (draw.next() % 3) as u32,
                2 => u32::MAX - (draw.next() % 3) as u32,
                _ => draw.next() as u32,
            };
            let mut bytes = announced.to_be_bytes().to_vec();
            let body = (draw.next() % 5_000) as usize;
            bytes.extend((0..body).map(|_| draw.next() as u8));
            inputs.push(bytes);
        }
        for bytes in &inputs {
            TRACKED.with(|t| t.set(true));
            let _ = read_one_frame(&mut &bytes[..], REQUEST_MAX_BYTES);
            TRACKED.with(|t| t.set(false));
            let largest = LARGEST.load(Ordering::Relaxed);
            assert!(
                largest <= REQUEST_MAX_BYTES,
                "the decoder allocated {largest} bytes for a frame of at most {REQUEST_MAX_BYTES}"
            );
        }
    }
}
