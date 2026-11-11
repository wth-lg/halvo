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
    /// The file has no block, two blocks, or a block that is not closed.
    Block,
}

/// The lines between the fence lines of the one block of `bytes`. A carriage return anywhere in
/// the file is refused first, with the number of its line.
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
    let mut found: Option<Vec<Line<'_>>> = None;
    let mut open: Option<Vec<Line<'_>>> = None;
    for line in lines {
        match open.as_mut() {
            Some(body) if line.bytes == b"```" => {
                found = Some(std::mem::take(body));
                open = None;
            }
            Some(body) => body.push(line),
            None if opens(line.bytes) => {
                if found.is_some() {
                    return Err(BlockFault::Block);
                }
                open = Some(Vec::new());
            }
            None => {}
        }
    }
    if open.is_some() {
        return Err(BlockFault::Block);
    }
    found.ok_or(BlockFault::Block)
}

fn opens(line: &[u8]) -> bool {
    line.strip_prefix(b"```")
        .is_some_and(|word| word.iter().all(u8::is_ascii_alphabetic))
}
