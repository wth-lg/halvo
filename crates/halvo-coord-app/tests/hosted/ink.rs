use halvo_coord::items::chars::{below, MOST_BELOW, MOST_MARKS};

/// What an ink case asserts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// The ink stays inside the field's room and reaches no other line's box.
    Room,
    /// The control: the ink reaches another line's box.
    Control,
    /// One mark more than `MOST_MARKS`: printed, never asserted.
    Probe,
    /// The marked line is the last: no control is drawn over.
    Last,
}

/// `count` marks taken in turn from `marks`.
pub fn stack(marks: &[char], count: usize) -> String {
    marks.iter().cycle().take(count).collect()
}

/// The ink cases: a name, the marked line plain and with its marks, and what the case asserts.
/// A mark is stacked as often as a native card allows it, `MOST_BELOW` times for a mark of a
/// below class and `MOST_MARKS` times for any other, so that a copy that allows one more mark
/// reads one more here; each probe stacks one more. Marks above stand on a tall letter (`l`),
/// marks below and enclosing marks on a letter with a descender (`y`), so that each direction
/// measures its deepest reach; U+0353, U+0359 and U+035C are the deepest marks below (ruling
/// RD-137 M3).
pub fn ink_cases() -> Vec<(String, String, String, Ink)> {
    let most = |mark: char| if below(mark) { MOST_BELOW } else { MOST_MARKS };
    let tall = "Subject (From the site): Hl";
    let deep = "Subject (From the site): gy";
    let payee = "Pay to (You gave this): Ally";
    let mut cases = Vec::new();
    for (name, base, mark) in [
        ("above-0301", tall, '\u{301}'),
        ("below-0316", deep, '\u{316}'),
        ("enclosing-0489", deep, '\u{489}'),
        ("deep-0353", deep, '\u{353}'),
        ("deep-0359", deep, '\u{359}'),
        ("deep-035C", deep, '\u{35c}'),
    ] {
        for (count, ink) in [(most(mark), Ink::Room), (most(mark) + 1, Ink::Probe)] {
            cases.push((
                format!("{name}x{count}"),
                base.to_owned(),
                format!("{base}{}", stack(&[mark], count)),
                ink,
            ));
        }
    }
    let deciding: String = std::iter::once('\u{316}')
        .chain(std::iter::repeat_n('\u{301}', MOST_MARKS - MOST_BELOW))
        .collect();
    cases.push((
        format!("deciding-{MOST_MARKS}"),
        payee.to_owned(),
        format!("{payee}{deciding}"),
        Ink::Room,
    ));
    for (name, mark) in [
        ("last-below-0316", '\u{316}'),
        ("last-deep-0353", '\u{353}'),
        ("last-deep-035C", '\u{35c}'),
        ("last-enclosing-0489", '\u{489}'),
    ] {
        cases.push((
            format!("{name}x{}", most(mark)),
            deep.to_owned(),
            format!("{deep}{}", stack(&[mark], most(mark))),
            Ink::Last,
        ));
    }
    cases.push((
        "control-0301x64".to_owned(),
        tall.to_owned(),
        format!("{tall}{}", stack(&['\u{301}'], 64)),
        Ink::Control,
    ));
    cases
}

/// The runs of rows that hold ink, as their first row and the row after their last.
pub fn ink_runs(rows: &[bool]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (y, ink) in rows.iter().enumerate() {
        match (ink, start) {
            (true, None) => start = Some(y),
            (false, Some(a)) => {
                runs.push((a, y));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(a) = start {
        runs.push((a, rows.len()));
    }
    runs
}

/// The reading of an ink case of five lines.
#[derive(Clone, Debug, PartialEq)]
pub struct InkReading {
    /// The pitch, from the first and the last line's ink.
    pub pitch: f64,
    /// The middle field's room: its line box and half of each empty line beside it.
    pub room: (f64, f64),
    /// The first row and the row after the last of the marked ink outside the first and the
    /// last line's own ink.
    pub ink: (usize, usize),
    /// Whether that ink lies inside the room.
    pub in_room: bool,
    /// Whether it reaches a row of the first or the last line's box.
    pub reaches: bool,
}

impl InkReading {
    /// The rows between the ink's end and the room's end, below and above: what is left before
    /// the ink leaves the room (negative once it has).
    pub fn margin(&self) -> (f64, f64) {
        (
            self.ink.0 as f64 - self.room.0,
            self.room.1 - self.ink.1 as f64,
        )
    }
}

/// The middle line's room in `plain`, a card of three text lines with an empty line between two,
/// and the ink of `marked` outside the first and the last line's own ink: whether it stays in the
/// room and whether it reaches a row of the first or the last line's box. A line's box is its ink
/// centre plus and less half the pitch, the pitch a quarter of the rows from the first line's
/// first ink to the last line's.
pub fn ink_reading(plain: &[bool], marked: &[bool]) -> Result<InkReading, String> {
    let runs = ink_runs(plain);
    if runs.len() != 3 {
        return Err(format!(
            "{} runs of ink in the plain card, not 3",
            runs.len()
        ));
    }
    let pitch = (runs[2].0 - runs[0].0) as f64 / 4.0;
    let centre = |run: (usize, usize)| (run.0 + run.1) as f64 / 2.0;
    let boxed = |run: (usize, usize)| (centre(run) - pitch / 2.0, centre(run) + pitch / 2.0);
    let middle = boxed(runs[1]);
    let room = (middle.0 - pitch / 2.0, middle.1 + pitch / 2.0);
    let own = |y: usize| (runs[0].0..runs[0].1).contains(&y) || (runs[2].0..runs[2].1).contains(&y);
    let rows: Vec<usize> = (0..marked.len())
        .filter(|y| marked[*y] && !own(*y))
        .collect();
    let (Some(&first), Some(&last)) = (rows.first(), rows.last()) else {
        return Err("no ink of the middle line".to_owned());
    };
    let within = |y: usize, b: (f64, f64)| y as f64 + 1.0 > b.0 && (y as f64) < b.1;
    let reaches = rows
        .iter()
        .any(|y| within(*y, boxed(runs[0])) || within(*y, boxed(runs[2])));
    let in_room = first as f64 >= room.0 && (last + 1) as f64 <= room.1;
    Ok(InkReading {
        pitch,
        room,
        ink: (first, last + 1),
        in_room,
        reaches,
    })
}
