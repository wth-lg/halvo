use std::io::{self, Read, Write};

/// The most payload bytes of a frame from the client (C5.2), the limit of an `analysis_write` included.
pub const CLIENT_FRAME_MAX: usize = 65_536;

/// The most payload bytes of a frame from the server (C5.2).
pub const SERVER_FRAME_MAX: usize = 262_144;

/// Why a frame was not read or not written.
#[derive(Debug)]
pub enum FrameError {
    /// The stream ended inside a frame: inside its 4 length bytes or inside its payload.
    End,
    /// The length is 0 or above the caller's limit; the reader read no payload byte.
    Length(u32),
    /// A read or a write of the stream failed.
    Io(io::Error),
}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> FrameError {
        FrameError::Io(e)
    }
}

fn fill<R: Read + ?Sized>(reader: &mut R, buf: &mut [u8]) -> Result<usize, FrameError> {
    let mut got = 0;
    while got < buf.len() {
        match reader.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(FrameError::Io(e)),
        }
    }
    Ok(got)
}

/// Reads one frame of C5.2: 4 bytes of a big-endian length, then that many payload bytes. Returns none when
/// the stream ends before the first length byte. Refuses a length of 0 or above `limit` before it reads any
/// payload byte, and reads no byte beyond the frame.
pub fn read_frame<R: Read + ?Sized>(
    reader: &mut R,
    limit: usize,
) -> Result<Option<Vec<u8>>, FrameError> {
    let mut length = [0u8; 4];
    match fill(reader, &mut length)? {
        0 => return Ok(None),
        4 => {}
        _ => return Err(FrameError::End),
    }
    let n = u32::from_be_bytes(length);
    let size = usize::try_from(n).map_err(|_| FrameError::Length(n))?;
    if size == 0 || size > limit {
        return Err(FrameError::Length(n));
    }
    let mut payload = vec![0u8; size];
    if fill(reader, &mut payload)? != size {
        return Err(FrameError::End);
    }
    Ok(Some(payload))
}

/// Writes one frame of C5.2 and flushes the stream. Refuses, and writes nothing, an empty payload or one above
/// `limit`.
pub fn write_frame<W: Write + ?Sized>(
    writer: &mut W,
    payload: &[u8],
    limit: usize,
) -> Result<(), FrameError> {
    let n = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    if payload.is_empty() || payload.len() > limit {
        return Err(FrameError::Length(n));
    }
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&n.to_be_bytes());
    frame.extend_from_slice(payload);
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}
