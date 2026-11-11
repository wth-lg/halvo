/// One line of a file: its number, counted from 1, and its bytes without the LF.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Line<'a> {
    pub number: usize,
    pub bytes: &'a [u8],
}

/// Why a file has no block that can be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockFault {
    /// The line of this number is the first that breaks the text of T0.4: it holds a carriage
    /// return or a byte sequence that is no UTF-8, it is the last line and the file has no LF at
    /// its end, or it is the first line and the file starts with a byte-order mark.
    Text(usize),
    /// The file does not hold exactly two fence lines, an opening and then a closing.
    Block,
}

/// The lines between the two fence lines of `bytes`. The text of T0.4 is checked first, over the
/// whole file, and names its first line that breaks it; an empty file has no line. A fence line is
/// a line whose first characters, after any spaces and tabs, are three backticks. The file holds
/// exactly two: the opening, three backticks at the start of the line, alone or followed by one
/// word of ASCII letters, and then the closing, exactly three backticks.
pub fn block(bytes: &[u8]) -> Result<Vec<Line<'_>>, BlockFault> {
    let ends = bytes.is_empty() || bytes.ends_with(b"\n");
    let mut pieces: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    if ends {
        pieces.pop();
    }
    let count = pieces.len();
    let mark = bytes.starts_with(b"\xEF\xBB\xBF");
    let lines: Vec<Line<'_>> = pieces
        .into_iter()
        .enumerate()
        .map(|(i, bytes)| Line {
            number: i + 1,
            bytes,
        })
        .collect();
    let breaks = |line: &&Line<'_>| {
        line.bytes.contains(&b'\r')
            || std::str::from_utf8(line.bytes).is_err()
            || (!ends && line.number == count)
            || (mark && line.number == 1)
    };
    if let Some(line) = lines.iter().find(breaks) {
        return Err(BlockFault::Text(line.number));
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
