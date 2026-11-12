use std::io::{self, Cursor, Read};

use halvo_seam::frame::{read_frame, write_frame, FrameError, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = (payload.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(payload);
    out
}

fn length_only(n: u32) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}

struct OneByte(Cursor<Vec<u8>>);

impl Read for OneByte {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.0.read(&mut buf[..1])
    }
}

struct Interrupting {
    inner: Cursor<Vec<u8>>,
    next: bool,
}

impl Read for Interrupting {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.next = !self.next;
        if self.next {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "again"));
        }
        let n = buf.len().min(3);
        self.inner.read(&mut buf[..n])
    }
}

struct Broken;

impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone"))
    }
}

#[test]
fn the_example_of_c5_2_travels_after_its_four_length_bytes() {
    let mut out = Vec::new();
    write_frame(&mut out, br#"{"msg":"read_batch"}"#, CLIENT_FRAME_MAX).unwrap();
    assert_eq!(&out[..4], &[0, 0, 0, 0x14]);
    assert_eq!(&out[4..], br#"{"msg":"read_batch"}"#);
    let got = read_frame(&mut Cursor::new(out), CLIENT_FRAME_MAX).unwrap();
    assert_eq!(got.as_deref(), Some(&br#"{"msg":"read_batch"}"#[..]));
}

#[test]
fn a_length_of_zero_is_refused_and_no_payload_is_read() {
    let mut input = Cursor::new([length_only(0), b"{}".to_vec()].concat());
    assert!(matches!(
        read_frame(&mut input, CLIENT_FRAME_MAX),
        Err(FrameError::Length(0))
    ));
    assert_eq!(input.position(), 4);
}

#[test]
fn a_frame_of_the_bound_is_taken_and_one_byte_over_it_is_refused_unread() {
    for limit in [CLIENT_FRAME_MAX, SERVER_FRAME_MAX] {
        let at_bound = vec![b' '; limit];
        let got = read_frame(&mut Cursor::new(frame(&at_bound)), limit).unwrap();
        assert_eq!(got.map(|p| p.len()), Some(limit));
        let over = vec![b' '; limit + 1];
        let mut input = Cursor::new(frame(&over));
        match read_frame(&mut input, limit) {
            Err(FrameError::Length(n)) => assert_eq!(n as usize, limit + 1),
            other => panic!("took a frame over the bound: {other:?}"),
        }
        assert_eq!(input.position(), 4);
        let mut huge = Cursor::new([length_only(u32::MAX), vec![b' '; 8]].concat());
        assert!(matches!(
            read_frame(&mut huge, limit),
            Err(FrameError::Length(u32::MAX))
        ));
        assert_eq!(huge.position(), 4);
    }
    let mut sink = Vec::new();
    assert!(write_frame(
        &mut sink,
        &vec![b' '; CLIENT_FRAME_MAX + 1],
        CLIENT_FRAME_MAX
    )
    .is_err());
    assert!(write_frame(&mut sink, b"", CLIENT_FRAME_MAX).is_err());
    assert!(sink.is_empty());
    write_frame(&mut sink, &vec![b' '; SERVER_FRAME_MAX], SERVER_FRAME_MAX).unwrap();
    assert_eq!(sink.len(), SERVER_FRAME_MAX + 4);
}

#[test]
fn a_frame_one_byte_short_ends_inside_the_frame() {
    let payload = br#"{"msg":"read_batch"}"#;
    let mut bytes = frame(payload);
    bytes.pop();
    assert!(matches!(
        read_frame(&mut Cursor::new(bytes), CLIENT_FRAME_MAX),
        Err(FrameError::End)
    ));
}

#[test]
fn a_frame_one_byte_long_leaves_its_last_byte_as_the_start_of_the_next_length() {
    let payload = br#"{"msg":"read_batch"}"#;
    let mut bytes = length_only(payload.len() as u32 - 1);
    bytes.extend_from_slice(payload);
    let mut input = Cursor::new(bytes);
    let first = read_frame(&mut input, CLIENT_FRAME_MAX).unwrap().unwrap();
    assert_eq!(first, &payload[..payload.len() - 1]);
    assert!(matches!(
        read_frame(&mut input, CLIENT_FRAME_MAX),
        Err(FrameError::End)
    ));
}

#[test]
fn the_end_of_the_stream_before_a_frame_is_no_error_and_inside_the_length_is_one() {
    assert!(read_frame(&mut Cursor::new(Vec::new()), CLIENT_FRAME_MAX)
        .unwrap()
        .is_none());
    for n in 1..4 {
        let bytes = length_only(20)[..n].to_vec();
        assert!(matches!(
            read_frame(&mut Cursor::new(bytes), CLIENT_FRAME_MAX),
            Err(FrameError::End)
        ));
    }
}

#[test]
fn a_frame_split_at_every_byte_and_two_frames_in_one_read_are_read_whole() {
    let a = br#"{"msg":"read_batch"}"#;
    let b = br#"{"msg":"close"}"#;
    let bytes = [frame(a), frame(b)].concat();
    let mut split = OneByte(Cursor::new(bytes.clone()));
    assert_eq!(
        read_frame(&mut split, CLIENT_FRAME_MAX).unwrap().unwrap(),
        a
    );
    assert_eq!(
        read_frame(&mut split, CLIENT_FRAME_MAX).unwrap().unwrap(),
        b
    );
    assert!(read_frame(&mut split, CLIENT_FRAME_MAX).unwrap().is_none());
    let mut together = Cursor::new(bytes);
    assert_eq!(
        read_frame(&mut together, CLIENT_FRAME_MAX)
            .unwrap()
            .unwrap(),
        a
    );
    assert_eq!(
        read_frame(&mut together, CLIENT_FRAME_MAX)
            .unwrap()
            .unwrap(),
        b
    );
    assert!(read_frame(&mut together, CLIENT_FRAME_MAX)
        .unwrap()
        .is_none());
    let mut interrupted = Interrupting {
        inner: Cursor::new([frame(a), frame(b)].concat()),
        next: false,
    };
    assert_eq!(
        read_frame(&mut interrupted, CLIENT_FRAME_MAX)
            .unwrap()
            .unwrap(),
        a
    );
    assert_eq!(
        read_frame(&mut interrupted, CLIENT_FRAME_MAX)
            .unwrap()
            .unwrap(),
        b
    );
}

#[test]
fn a_failed_read_is_an_io_error() {
    assert!(matches!(
        read_frame(&mut Broken, CLIENT_FRAME_MAX),
        Err(FrameError::Io(_))
    ));
}
