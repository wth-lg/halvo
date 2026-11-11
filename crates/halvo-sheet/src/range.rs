use crate::error::Error;

/// A rectangle of cells, from its first cell to its last, as a range of the A1 form names it:
/// `B2:D6`, or one cell `B2`. Columns are 1 to 3 letters of either case up to `XFD`, rows are
/// 1 to 1,048,576 written with no leading zero, and the first cell's row and column are at
/// most the last cell's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub(crate) first: (u32, u32),
    pub(crate) last: (u32, u32),
}

impl Range {
    /// Parses a range of the A1 form; [`Error::Range`] for any other text.
    pub fn parse(text: &str) -> Result<Range, Error> {
        let (first, last) = text.split_once(':').unwrap_or((text, text));
        let first = cell(first)?;
        let last = cell(last)?;
        if first.0 > last.0 || first.1 > last.1 {
            return Err(Error::Range);
        }
        Ok(Range { first, last })
    }

    /// The number of rows.
    pub fn rows(&self) -> u32 {
        self.last.0 - self.first.0 + 1
    }

    /// The number of columns.
    pub fn cols(&self) -> u32 {
        self.last.1 - self.first.1 + 1
    }

    pub(crate) fn cells(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        (self.first.0..=self.last.0)
            .flat_map(move |row| (self.first.1..=self.last.1).map(move |col| (row, col)))
    }

    pub(crate) fn holds(&self, (row, col): (u32, u32)) -> bool {
        (self.first.0..=self.last.0).contains(&row) && (self.first.1..=self.last.1).contains(&col)
    }
}

fn cell(text: &str) -> Result<(u32, u32), Error> {
    let letters = text.bytes().take_while(u8::is_ascii_alphabetic).count();
    let (col_text, row_text) = text.split_at(letters);
    let digits = !row_text.is_empty() && row_text.bytes().all(|b| b.is_ascii_digit());
    if !(1..=3).contains(&letters) || !digits || row_text.starts_with('0') {
        return Err(Error::Range);
    }
    let col = col_text.bytes().fold(0u32, |acc, b| {
        acc * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1)
    });
    let row: u32 = row_text.parse().map_err(|_| Error::Range)?;
    if col > 16_384 || row > 1_048_576 {
        return Err(Error::Range);
    }
    Ok((row - 1, col - 1))
}
