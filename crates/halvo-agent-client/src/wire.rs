//! The frame of the link between a runner and its supervisor: a 4-byte big-endian length and that many bytes of
//! JSON, at most 65,536.

use std::io::{self, Read, Write};

/// The most bytes of the JSON of one frame.
pub const FRAME_MAX: usize = 65_536;

/// Reads one frame. `Ok(None)` is an end of file before the first byte of a frame; an end of file inside a frame
/// is `UnexpectedEof`, and a length above `FRAME_MAX` is `InvalidData`, read before any byte of the JSON.
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; 4];
    let mut got = 0;
    while got < header.len() {
        match reader.read(&mut header[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = usize::try_from(u32::from_be_bytes(header)).unwrap_or(usize::MAX);
    if len > FRAME_MAX {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame_length"));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Writes one frame and flushes it; JSON above `FRAME_MAX` bytes is `InvalidInput` and writes nothing.
pub fn write_frame(writer: &mut impl Write, json: &str) -> io::Result<()> {
    let len = u32::try_from(json.len())
        .ok()
        .filter(|n| usize::try_from(*n).is_ok_and(|n| n <= FRAME_MAX))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "frame_length"))?;
    let mut frame = Vec::with_capacity(4 + json.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(json.as_bytes());
    writer.write_all(&frame)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_a_big_endian_length_and_its_json() {
        let mut out = Vec::new();
        write_frame(&mut out, "{\"t\":\"cancel\"}").unwrap();
        assert_eq!(&out[..4], &[0, 0, 0, 14]);
        assert_eq!(&out[4..], b"{\"t\":\"cancel\"}");
        let mut input = out.as_slice();
        assert_eq!(
            read_frame(&mut input).unwrap().unwrap(),
            b"{\"t\":\"cancel\"}"
        );
        assert!(read_frame(&mut input).unwrap().is_none());
    }

    #[test]
    fn a_frame_of_65536_bytes_passes_and_one_more_is_refused_both_ways() {
        let at = "x".repeat(FRAME_MAX);
        let mut out = Vec::new();
        write_frame(&mut out, &at).unwrap();
        assert_eq!(&out[..4], &[0, 1, 0, 0]);
        assert_eq!(
            read_frame(&mut out.as_slice()).unwrap().unwrap().len(),
            FRAME_MAX
        );
        let over = "x".repeat(FRAME_MAX + 1);
        let mut none = Vec::new();
        let e = write_frame(&mut none, &over).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        assert!(none.is_empty());
        let header = [0u8, 1, 0, 1];
        let e = read_frame(&mut header.as_slice()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn an_end_inside_a_frame_is_an_error_and_not_an_end_of_the_link() {
        let cut = [0u8, 0, 0, 5, b'{', b'}'];
        let e = read_frame(&mut cut.as_slice()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
        let half_header = [0u8, 0];
        let e = read_frame(&mut half_header.as_slice()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
    }
}
