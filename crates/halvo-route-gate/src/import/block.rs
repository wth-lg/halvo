/// One line of a file: its number, counted from 1, and its bytes without the LF.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Line<'a> {
    pub number: usize,
    pub bytes: &'a [u8],
}

/// Why a file has no block that can be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockFault {
    /// The line of this number holds a carriage return.
    CarriageReturn(usize),
    /// The file does not hold exactly two fence lines, an opening and then a closing.
    Block,
}

/// The lines between the two fence lines of `bytes`. A fence line is a line whose first
/// characters, after any spaces and tabs, are three backticks. The file holds exactly two: the
/// opening, three backticks at the start of the line, alone or followed by one word of ASCII
/// letters, and then the closing, exactly three backticks. A carriage return anywhere in the file
/// is refused first, with the number of its line.
pub fn block(bytes: &[u8]) -> Result<Vec<Line<'_>>, BlockFault> {
    let mut pieces: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        pieces.pop();
    }
    let lines: Vec<Line<'_>> = pieces
        .into_iter()
        .enumerate()
        .map(|(i, bytes)| Line {
            number: i + 1,
            bytes,
        })
        .collect();
    if let Some(line) = lines.iter().find(|line| line.bytes.contains(&b'\r')) {
        return Err(BlockFault::CarriageReturn(line.number));
    }
    let fences: Vec<usize> = (0..lines.len())
        .filter(|&i| fence(lines[i].bytes))
        .collect();
    match fences[..] {
        [open, close] if opens(lines[open].bytes) && lines[close].bytes == b"```" => {
            Ok(lines[open + 1..close].to_vec())
        }
        _ => Err(BlockFault::Block),
    }
}

fn fence(line: &[u8]) -> bool {
    line.iter()
        .position(|byte| *byte != b' ' && *byte != b'\t')
        .is_some_and(|start| line[start..].starts_with(b"```"))
}

fn opens(line: &[u8]) -> bool {
    line.strip_prefix(b"```")
        .is_some_and(|word| word.iter().all(u8::is_ascii_alphabetic))
}
