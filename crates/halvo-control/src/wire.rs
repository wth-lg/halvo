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

    /// The test binary's allocator. While the flag `TRACKED` of a thread is set, it records the
    /// largest single block that the thread asks for (`LARGEST`) and the thread's live bytes:
    /// the bytes it allocated less the bytes it freed (`LIVE`) and their highest value
    /// (`PEAK`).
    struct Largest;

    thread_local! {
        static TRACKED: Cell<bool> = const { Cell::new(false) };
        static LARGEST: Cell<usize> = const { Cell::new(0) };
        static LIVE: Cell<isize> = const { Cell::new(0) };
        static PEAK: Cell<isize> = const { Cell::new(0) };
    }

    fn tracked() -> bool {
        TRACKED.try_with(Cell::get).unwrap_or(false)
    }

    fn add_live(bytes: isize) {
        let live = LIVE.with(Cell::get) + bytes;
        LIVE.with(|l| l.set(live));
        PEAK.with(|p| p.set(p.get().max(live)));
    }

    // SAFETY: every call is passed on to the system allocator unchanged; the only addition
    // reads and writes thread-local counters and stores a size.
    unsafe impl GlobalAlloc for Largest {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if tracked() {
                LARGEST.with(|l| l.set(l.get().max(layout.size())));
                add_live(layout.size() as isize);
            }
            // SAFETY: the caller's layout goes to the system allocator as it came.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            if tracked() {
                LARGEST.with(|l| l.set(l.get().max(layout.size())));
                add_live(layout.size() as isize);
            }
            // SAFETY: the caller's layout goes to the system allocator as it came.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if tracked() {
                add_live(-(layout.size() as isize));
            }
            // SAFETY: the block is one that this allocator handed out through alloc, alloc_zeroed
            // or realloc, each of which passes to System, and `layout` is the one that the block
            // has now.
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            if tracked() {
                LARGEST.with(|l| l.set(l.get().max(new_size)));
                add_live(new_size as isize - layout.size() as isize);
            }
            // SAFETY: the pointer and layout came from this allocator and go back to it.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    /// Runs `call` with the live bytes of this thread counted from 0, and gives the highest
    /// count and the count at its end.
    fn live_bytes_of(call: impl FnOnce()) -> (isize, isize) {
        LIVE.with(|l| l.set(0));
        PEAK.with(|p| p.set(0));
        TRACKED.with(|t| t.set(true));
        call();
        TRACKED.with(|t| t.set(false));
        (PEAK.with(Cell::get), LIVE.with(Cell::get))
    }

    /// `{"v":1,"a":` and `inner` and `}`, with spaces before the `}` so that the frame is
    /// `size` bytes; `None` when `inner` does not fit.
    fn padded(inner: &str, size: usize) -> Option<Vec<u8>> {
        let head = HEAD.as_bytes();
        let used = head.len() + inner.len() + 1;
        if used > size {
            return None;
        }
        let mut frame = head.to_vec();
        frame.extend_from_slice(inner.as_bytes());
        frame.resize(size - 1, b' ');
        frame.push(b'}');
        Some(frame)
    }

    /// `[` and copies of `item` separated by commas and `]`, as many as fit in a frame of
    /// `size` bytes.
    fn array_of(item: &str, size: usize) -> Vec<u8> {
        let mut items = std::iter::repeat(item);
        array_drawn(size, || items.next().unwrap())
    }

    /// `[`, the items that `next` gives while they fit, separated by commas, and `]`, in a
    /// frame of `size` bytes.
    fn array_drawn<'a>(size: usize, mut next: impl FnMut() -> &'a str) -> Vec<u8> {
        let room = size - HEAD.len() - 1;
        let mut inner = String::with_capacity(room);
        inner.push('[');
        inner.push_str(next());
        loop {
            let item = next();
            if inner.len() + 1 + item.len() + 1 > room {
                break;
            }
            inner.push(',');
            inner.push_str(item);
        }
        inner.push(']');
        padded(&inner, size).unwrap()
    }

    const HEAD: &str = r#"{"v":1,"a":"#;

    /// The deepest nesting of arrays that the parser takes, counted from the top.
    fn depth_limit() -> usize {
        (1..1_000)
            .take_while(|d| {
                let text = format!("{}{}", "[".repeat(*d), "]".repeat(*d));
                serde_json::from_slice::<serde_json::Value>(text.as_bytes()).is_ok()
            })
            .last()
            .unwrap()
    }

    /// The frames of the worst shapes, each exactly `size` bytes, with their names.
    fn worst_shapes(size: usize) -> Vec<(String, Vec<u8>)> {
        let deep = depth_limit() - 2;
        let nest = |open: &str, leaf: &str, close: &str, levels: usize| {
            format!("{}{leaf}{}", open.repeat(levels), close.repeat(levels))
        };
        let keys: String = (0u8..128)
            .map(|b| format!("\"\\u{b:04x}\":0"))
            .collect::<Vec<_>>()
            .join(",");
        let mut shapes = vec![
            (
                "an array of one-byte values".to_string(),
                array_of("0", size),
            ),
            ("an array of empty arrays".to_string(), array_of("[]", size)),
            (
                "an array of arrays of one value".to_string(),
                array_of("[0]", size),
            ),
            (
                "an array of objects of one one-byte key".to_string(),
                array_of(r#"{"a":0}"#, size),
            ),
            (
                format!("arrays nested {deep} deep at the parser's depth limit, side by side"),
                array_of(&nest("[", "", "]", deep - 1), size),
            ),
            (
                format!("objects of the empty key nested {deep} deep, side by side"),
                array_of(&nest(r#"{"":"#, "0", "}", deep - 1), size),
            ),
        ];
        if let Some(frame) = padded(&format!("{{{keys}}}"), size) {
            shapes.push((
                "an object of the 128 one-byte keys, each once".to_string(),
                frame,
            ));
        }
        shapes
    }

    /// Frames of exactly `size` bytes whose array holds items drawn from a few shapes.
    fn drawn_frames(size: usize, count: usize) -> Vec<Vec<u8>> {
        const ITEMS: [&str; 8] = [
            "0",
            "[]",
            "{}",
            "\"\"",
            "\"ab\"",
            "[0]",
            r#"{"a":0}"#,
            "[[[0]]]",
        ];
        let mut draw = Draw(0x2545_f491_4f6c_dd1d);
        (0..count)
            .map(|_| array_drawn(size, || ITEMS[(draw.next() % ITEMS.len() as u64) as usize]))
            .collect()
    }

    #[test]
    #[ignore = "prints the peak live bytes of each shape"]
    fn print_the_peak_live_bytes_of_one_decode_call() {
        for (side, size) in [("request", REQUEST_MAX_BYTES), ("reply", REPLY_MAX_BYTES)] {
            for (name, frame) in worst_shapes(size) {
                assert_eq!(frame.len(), size);
                let (peak, end) = live_bytes_of(|| {
                    if side == "request" {
                        drop(crate::proto::decode_request(&frame));
                    } else {
                        drop(crate::proto::decode_reply(&frame));
                    }
                });
                println!("peak {side} {peak} end {end} {name}");
            }
            let mut most = 0;
            for frame in drawn_frames(size, 50) {
                let (peak, _) = live_bytes_of(|| {
                    if side == "request" {
                        drop(crate::proto::decode_request(&frame));
                    } else {
                        drop(crate::proto::decode_reply(&frame));
                    }
                });
                most = most.max(peak);
            }
            println!("peak {side} {most} drawn");
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
    fn the_frame_reader_never_panics_on_arbitrary_bytes_and_asks_for_no_block_above_the_frame_limit_on_either_side(
    ) {
        for (max, inputs, count) in [
            (REQUEST_MAX_BYTES, 4_000, 1_000),
            (REPLY_MAX_BYTES, 1_000, 200),
        ] {
            let mut draw = Draw(0x9e37_79b9_7f4a_7c15);
            let mut frames: Vec<Vec<u8>> = Vec::new();
            for _ in 0..inputs {
                let len = (draw.next() % 64) as usize;
                frames.push((0..len).map(|_| draw.next() as u8).collect());
            }
            for _ in 0..count {
                let announced = match draw.next() % 4 {
                    0 => (draw.next() % (max as u64 + 1_000)) as u32,
                    1 => max as u32 + (draw.next() % 3) as u32,
                    2 => u32::MAX - (draw.next() % 3) as u32,
                    _ => draw.next() as u32,
                };
                let mut bytes = announced.to_be_bytes().to_vec();
                let body = (draw.next() % (max as u64 + 1_000)) as usize;
                bytes.extend((0..body).map(|_| draw.next() as u8));
                frames.push(bytes);
            }
            LARGEST.with(|l| l.set(0));
            for bytes in &frames {
                TRACKED.with(|t| t.set(true));
                let _ = read_one_frame(&mut &bytes[..], max);
                TRACKED.with(|t| t.set(false));
                let largest = LARGEST.with(Cell::get);
                assert!(
                    largest <= max,
                    "the frame reader asked for a block of {largest} bytes for a frame of at most {max}"
                );
            }
        }
    }

    fn decode_on(side: &str, frame: &[u8]) {
        if side == "request" {
            drop(crate::proto::decode_request(frame));
        } else {
            drop(crate::proto::decode_reply(frame));
        }
    }

    #[test]
    fn one_decode_call_holds_at_most_its_named_peak_of_live_bytes_and_frees_them_on_both_sides() {
        use crate::proto::{REPLY_DECODE_PEAK_BYTES, REQUEST_DECODE_PEAK_BYTES};

        for (side, size, bound) in [
            ("request", REQUEST_MAX_BYTES, REQUEST_DECODE_PEAK_BYTES),
            ("reply", REPLY_MAX_BYTES, REPLY_DECODE_PEAK_BYTES),
        ] {
            let mut frames = worst_shapes(size);
            for (i, frame) in drawn_frames(size, 20).into_iter().enumerate() {
                frames.push((format!("drawn frame {i}"), frame));
            }
            for (name, frame) in &frames {
                assert_eq!(frame.len(), size, "{side}: {name}");
                let (peak, end) = live_bytes_of(|| decode_on(side, frame));
                assert!(
                    peak <= bound as isize,
                    "{side}: {name}: one decode call held {peak} live bytes, above {bound}"
                );
                assert_eq!(
                    end, 0,
                    "{side}: {name}: the decode call left {end} bytes live after it returned"
                );
            }
        }
        let request = crate::proto::encode_request(&crate::proto::Request::Pause);
        let (_, end) = live_bytes_of(|| decode_on("request", &request));
        assert_eq!(end, 0, "a request that decodes left bytes live");
    }

    #[test]
    fn the_named_peaks_are_128_bytes_for_each_byte_of_a_frame() {
        assert_eq!(crate::proto::REQUEST_DECODE_PEAK_BYTES, 524_288);
        assert_eq!(crate::proto::REPLY_DECODE_PEAK_BYTES, 33_554_432);
    }
}
