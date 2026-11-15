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

/// Reads the one frame of a connection and requires the stream to end after it: a byte after
/// the frame is [`FrameError::Trailing`] and a timeout of that wait is an error. The server reads
/// a request with it; the client reads a reply with its own reader in client.rs, which waits a
/// bounded time after the frame and takes the frame when the stream stays open.
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

    /// An envelope of protocol version 1 whose body the decoder of one side takes, or refuses only
    /// after it has read the body, with a field `x` last in the body: a name, the text before the
    /// value of `x` and the text after it.
    struct Envelope {
        name: &'static str,
        head: String,
        tail: &'static str,
    }

    fn envelopes(side: &str) -> Vec<Envelope> {
        let at = |name, head: &str, tail| Envelope {
            name,
            head: head.to_string(),
            tail,
        };
        if side == "request" {
            return vec![
                at(
                    "a pause with the field x too many",
                    r#"{"v":1,"body":{"op":"pause","x":"#,
                    "}}",
                ),
                at(
                    "an op that the table does not know, with a field x",
                    r#"{"v":1,"body":{"op":"zzz","x":"#,
                    "}}",
                ),
            ];
        }
        let status = String::from_utf8(crate::proto::encode_reply(&crate::proto::Reply::Status(
            crate::testing::sample_status(crate::proto::Platform::Macos),
        )))
        .unwrap();
        let broken = status.replace(r#""capture":"running""#, r#""capture":"paused""#);
        assert_ne!(broken, status, "the sample status reads capture=running");
        let open = broken.strip_suffix("}}").unwrap();
        let fields = open
            .strip_prefix(r#"{"v":1,"body":{"kind":"status","#)
            .unwrap();
        vec![
            at(
                "an error reply with a field x that the build does not know",
                r#"{"v":1,"body":{"kind":"error","code":"busy","message":"m","x":"#,
                "}}",
            ),
            at(
                "a status that reads capture=paused beside desired=running, with a field x",
                &format!(r#"{open},"x":"#),
                "}}",
            ),
            at(
                "a cap_set whose status reads capture=paused beside desired=running, with a field x",
                &format!(r#"{{"v":1,"body":{{"kind":"cap_set","evicting":false,"status":{{{fields},"x":"#),
                "}}}",
            ),
        ]
    }

    /// The envelope's head, `inner`, spaces and its tail, `size` bytes in all; `None` when `inner`
    /// does not fit.
    fn in_body(envelope: &Envelope, inner: &str, size: usize) -> Option<Vec<u8>> {
        let used = envelope.head.len() + inner.len() + envelope.tail.len();
        if used > size {
            return None;
        }
        let mut frame = Vec::with_capacity(size);
        frame.extend_from_slice(envelope.head.as_bytes());
        frame.extend_from_slice(inner.as_bytes());
        frame.resize(size - envelope.tail.len(), b' ');
        frame.extend_from_slice(envelope.tail.as_bytes());
        Some(frame)
    }

    /// The bytes that the envelope leaves for the value of `x` in a frame of `size` bytes.
    fn room(envelope: &Envelope, size: usize) -> usize {
        size - envelope.head.len() - envelope.tail.len()
    }

    /// The frame of `size` bytes whose field x holds an array of `width` trees side by side, each
    /// an object of the empty key nested `depth` levels around 0; `None` when they do not fit.
    fn tree_frame(envelope: &Envelope, depth: usize, width: usize, size: usize) -> Option<Vec<u8>> {
        let mut tree = String::with_capacity(5 * depth + 1);
        for _ in 0..depth {
            tree.push_str(r#"{"":"#);
        }
        tree.push('0');
        for _ in 0..depth {
            tree.push('}');
        }
        let mut inner = String::with_capacity(width * (tree.len() + 1) + 1);
        inner.push('[');
        for i in 0..width {
            if i > 0 {
                inner.push(',');
            }
            inner.push_str(&tree);
        }
        inner.push(']');
        in_body(envelope, &inner, size)
    }

    /// The most trees nested `depth` levels that fit side by side in the envelope's room.
    fn widest(envelope: &Envelope, depth: usize, size: usize) -> usize {
        (room(envelope, size) - 1) / (5 * depth + 2)
    }

    /// The deepest nesting of objects of the empty key in the field x whose frame the decoder
    /// still reads to its body.
    fn deepest(side: &str, envelope: &Envelope, size: usize) -> usize {
        (1..1_000)
            .take_while(|d| decode_on(side, &tree_frame(envelope, *d, 1, size).unwrap()))
            .last()
            .unwrap()
    }

    /// `[` and copies of `item` separated by commas and `]`, as many as fit in `room` bytes.
    fn array_of(item: &str, room: usize) -> String {
        let mut items = std::iter::repeat(item);
        array_drawn(room, || items.next().unwrap())
    }

    /// `[`, the items that `next` gives while they fit in `room` bytes, separated by commas, and
    /// `]`.
    fn array_drawn<'a>(room: usize, mut next: impl FnMut() -> &'a str) -> String {
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
        inner
    }

    /// The values of the worst shapes for the field x, each as long as fits in `room` bytes, with
    /// their names; `deep` is the most levels that a shape nests inside its array.
    fn worst_shapes(room: usize, deep: usize) -> Vec<(String, String)> {
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
                array_of("0", room),
            ),
            ("an array of empty arrays".to_string(), array_of("[]", room)),
            (
                "an array of arrays of one value".to_string(),
                array_of("[0]", room),
            ),
            (
                "an array of objects of one one-byte key".to_string(),
                array_of(r#"{"a":0}"#, room),
            ),
            (
                format!("arrays nested {deep} deep at the decoder's depth limit, side by side"),
                array_of(&nest("[", "", "]", deep), room),
            ),
            (
                format!("objects of the empty key nested {deep} deep, side by side"),
                array_of(&nest(r#"{"":"#, "0", "}", deep), room),
            ),
        ];
        let object = format!("{{{keys}}}");
        if object.len() <= room {
            shapes.push((
                "an object of the 128 one-byte keys, each once".to_string(),
                object,
            ));
        }
        shapes
    }

    /// Values for the field x of at most `room` bytes whose array holds items drawn from a few
    /// shapes.
    fn drawn_values(room: usize, count: usize) -> Vec<String> {
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
            .map(|_| array_drawn(room, || ITEMS[(draw.next() % ITEMS.len() as u64) as usize]))
            .collect()
    }

    /// Every frame of `size` bytes that the test of the named peaks measures on `side`, with its
    /// name: the worst shapes in the body of each envelope, 20 drawn values in the body of the
    /// first, and a sweep over depth and width of the trees of [`tree_frame`] in each envelope.
    /// The sweep takes every depth from 1 to the deepest that the decoder reads, at the widths
    /// 1, half the widest and the widest, on the request's side; on the reply's side, where one
    /// decode takes longer, the depths of `REPLY_SWEEP_DEPTHS` and the deepest two at the
    /// widest width. The ignored test below prints the whole sweep of both sides.
    fn measured_frames(side: &str, size: usize) -> Vec<(String, Vec<u8>)> {
        let mut frames = Vec::new();
        for (e, envelope) in envelopes(side).iter().enumerate() {
            let deep = deepest(side, envelope, size);
            for (name, inner) in worst_shapes(room(envelope, size), deep) {
                let frame = in_body(envelope, &inner, size).unwrap();
                frames.push((format!("{}: {name}", envelope.name), frame));
            }
            if e == 0 {
                for (i, inner) in drawn_values(room(envelope, size), 20)
                    .into_iter()
                    .enumerate()
                {
                    let frame = in_body(envelope, &inner, size).unwrap();
                    frames.push((format!("{}: drawn value {i}", envelope.name), frame));
                }
            }
            let depths: Vec<usize> = if side == "request" {
                (1..=deep).collect()
            } else {
                REPLY_SWEEP_DEPTHS
                    .iter()
                    .copied()
                    .filter(|d| *d < deep - 1)
                    .chain([deep - 1, deep])
                    .collect()
            };
            for depth in depths {
                let most = widest(envelope, depth, size);
                let widths = if side == "request" {
                    vec![1, most / 2, most]
                } else {
                    vec![most]
                };
                for width in widths.into_iter().filter(|w| *w > 0) {
                    let frame = tree_frame(envelope, depth, width, size).unwrap();
                    frames.push((
                        format!(
                            "{}: objects of the empty key nested {depth} deep, {width} side by side",
                            envelope.name
                        ),
                        frame,
                    ));
                }
            }
        }
        frames
    }

    /// The depths of the reply's sweep in this test: powers of two, and the six depths whose
    /// frames held the most in the printer's whole sweep (100 the most, 33,047,177 bytes, and
    /// 107, 97, 122, 114 and 109 after it; 107 the most, 66,057,051, before the decoder kept
    /// one tree).
    const REPLY_SWEEP_DEPTHS: [usize; 13] = [1, 2, 4, 8, 16, 32, 64, 96, 97, 100, 107, 109, 114];

    /// The answer of the decoder of `side` to `frame` in one line that holds no part of the frame:
    /// the kind and the code of a reply, the request or the version of an unknown one, or the
    /// error.
    fn answer_of(side: &str, frame: &[u8]) -> String {
        if side == "request" {
            match crate::proto::decode_request(frame) {
                Ok(crate::proto::Decoded::Known(request)) => format!("known {request:?}"),
                Ok(crate::proto::Decoded::Unknown(unknown)) => {
                    format!("unknown of version {}", unknown.version())
                }
                Err(e) => format!("{e:?}"),
            }
        } else {
            match crate::proto::decode_reply(frame) {
                Ok(reply) => format!("{} {:?}", reply.kind(), reply.code()),
                Err(e) => format!("{e:?}"),
            }
        }
    }

    #[test]
    #[ignore = "prints the peak live bytes of each frame of the whole sweep"]
    fn print_the_peak_live_bytes_of_one_decode_call() {
        for (side, size) in [("request", REQUEST_MAX_BYTES), ("reply", REPLY_MAX_BYTES)] {
            let mut most = (0, String::new());
            for (name, frame) in measured_frames(side, size) {
                assert_eq!(frame.len(), size);
                let mut read = false;
                let (peak, end) = live_bytes_of(|| read = decode_on(side, &frame));
                println!("peak {side} {peak} end {end} read the body {read}: {name}");
                println!("answer {side} {}: {name}", answer_of(side, &frame));
                if peak > most.0 {
                    most = (peak, name);
                }
            }
            for (e, envelope) in envelopes(side).iter().enumerate() {
                let deep = deepest(side, envelope, size);
                for depth in 1..=deep {
                    let top = widest(envelope, depth, size);
                    for width in [1, top / 4, top / 2, top] {
                        if width == 0 {
                            continue;
                        }
                        let frame = tree_frame(envelope, depth, width, size).unwrap();
                        let mut read = false;
                        let (peak, end) = live_bytes_of(|| read = decode_on(side, &frame));
                        println!(
                            "sweep {side} envelope {e} depth {depth} width {width} peak {peak} end {end} read the body {read}"
                        );
                        if peak > most.0 {
                            most = (peak, format!("envelope {e}, depth {depth}, width {width}"));
                        }
                    }
                }
            }
            println!("most {side} {} {}", most.0, most.1);
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

    /// Decodes `frame` on `side`, drops the answer, and says whether the decoder read the body:
    /// every answer but no envelope.
    fn decode_on(side: &str, frame: &[u8]) -> bool {
        if side == "request" {
            !matches!(
                crate::proto::decode_request(frame),
                Err(crate::proto::RequestError::NotAnEnvelope)
            )
        } else {
            !matches!(
                crate::proto::decode_reply(frame),
                Err(crate::proto::ReplyError::NotAnEnvelope)
            )
        }
    }

    #[test]
    fn one_decode_call_holds_at_most_its_named_peak_of_live_bytes_and_frees_them_on_both_sides() {
        use crate::proto::{REPLY_DECODE_PEAK_BYTES, REQUEST_DECODE_PEAK_BYTES};

        for (side, size, bound) in [
            ("request", REQUEST_MAX_BYTES, REQUEST_DECODE_PEAK_BYTES),
            ("reply", REPLY_MAX_BYTES, REPLY_DECODE_PEAK_BYTES),
        ] {
            for (name, frame) in &measured_frames(side, size) {
                assert_eq!(frame.len(), size, "{side}: {name}");
                let mut read = false;
                let (peak, end) = live_bytes_of(|| read = decode_on(side, frame));
                assert!(read, "{side}: {name}: the decoder did not read the body");
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
        let (_, end) = live_bytes_of(|| {
            decode_on("request", &request);
        });
        assert_eq!(end, 0, "a request that decodes left bytes live");
    }

    #[test]
    fn the_named_peaks_are_128_bytes_for_each_byte_of_a_frame() {
        assert_eq!(crate::proto::REQUEST_DECODE_PEAK_BYTES, 524_288);
        assert_eq!(crate::proto::REPLY_DECODE_PEAK_BYTES, 33_554_432);
    }
}
