//! The outcome table of contract T5.1: `Row` is one attempt, `Table::parse` its reader and
//! `Table::to_text` its writer. A `Table` holds only rows that the reader accepts, in the byte
//! order of their text.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;

use halvo_route::tier::{CallKind, CategoryId, Level, ParseError, Tier};

/// The header line of an outcome table.
pub const HEADER: &str = "task\tcategory\tkind\ttier\tmodel\tmachine\tseed\tsplit\tattempt\tpass\tsteps\twall_s\ttokens_in\ttokens_out\tlevel\tsource";

/// The model of every generated row.
pub const SYNTHETIC: &str = "synthetic";

const CELLS: usize = 16;

macro_rules! word_list {
    ($(#[$doc:meta])* $name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $word),+
                }
            }

            pub fn parse(text: &str) -> Result<$name, ParseError> {
                $name::ALL.iter().copied().find(|value| value.as_str() == text).ok_or(ParseError)
            }
        }
    };
}

word_list! {
    /// The task of a row: one of the agent-setup run's five in a recorded row, `gen` in a generated one.
    Task { Bill => "bill", Reply => "reply", Order => "order", Cleanup => "cleanup", Fill => "fill", Gen => "gen" }
}

word_list! {
    /// The machine of a row: `mac` or `hosted-win` in a recorded row, `gen_a` or `gen_b` in a generated one.
    Machine { Mac => "mac", HostedWin => "hosted-win", GenA => "gen_a", GenB => "gen_b" }
}

word_list! {
    /// The seed of a row; seeds 1 and 2 are development rows and seed 3 held-out rows.
    Seed { One => "1", Two => "2", Three => "3" }
}

word_list! {
    /// The split of a row, which its seed decides.
    Split { Dev => "dev", Held => "held" }
}

word_list! {
    /// The source of a row: an id of the sources file in a recorded row, `generated` in a generated one.
    Source { S1 => "s1", S2 => "s2", Generated => "generated" }
}

word_list! {
    /// Why a table is refused; the reader names the first fault in line order.
    Fault {
        Text => "text",
        Header => "header",
        Cell => "cell",
        Split => "split",
        Mixed => "mixed",
        Order => "order",
        Key => "key",
        Level => "level",
    }
}

impl Seed {
    pub const fn split(self) -> Split {
        match self {
            Seed::One | Seed::Two => Split::Dev,
            Seed::Three => Split::Held,
        }
    }
}

/// A model token: 1 to 32 bytes of `a` to `z`, `0` to `9` and `_`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Model(String);

impl Model {
    pub fn parse(text: &str) -> Result<Model, ParseError> {
        let token = (1..=32).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if token {
            Ok(Model(text.to_owned()))
        } else {
            Err(ParseError)
        }
    }

    pub fn synthetic() -> Model {
        Model(SYNTHETIC.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One attempt of one task in one call kind, by one model on one machine with one seed.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Row {
    pub task: Task,
    pub category: CategoryId,
    pub kind: CallKind,
    pub tier: Tier,
    pub model: Model,
    pub machine: Machine,
    pub seed: Seed,
    pub attempt: NonZeroU32,
    pub pass: bool,
    pub steps: u64,
    pub wall_s: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub level: Level,
    pub source: Source,
}

type Key = (
    Task,
    CategoryId,
    CallKind,
    Tier,
    String,
    Machine,
    Seed,
    NonZeroU32,
);

impl Row {
    pub const fn is_generated(&self) -> bool {
        matches!(self.source, Source::Generated)
    }

    pub const fn split(&self) -> Split {
        self.seed.split()
    }

    fn key(&self) -> Key {
        (
            self.task,
            self.category,
            self.kind,
            self.tier,
            self.model.0.clone(),
            self.machine,
            self.seed,
            self.attempt,
        )
    }

    /// The line of the row in the table, without its LF.
    pub fn to_line(&self) -> String {
        let attempt = self.attempt.to_string();
        let pass = if self.pass { "yes" } else { "no" };
        let figures =
            [self.steps, self.wall_s, self.tokens_in, self.tokens_out].map(|n| n.to_string());
        [
            self.task.as_str(),
            self.category.as_str(),
            self.kind.as_str(),
            self.tier.as_str(),
            self.model.as_str(),
            self.machine.as_str(),
            self.seed.as_str(),
            self.split().as_str(),
            &attempt,
            pass,
            &figures[0],
            &figures[1],
            &figures[2],
            &figures[3],
            self.level.as_str(),
            self.source.as_str(),
        ]
        .join("\t")
    }
}

/// A refusal of the reader: the fault and the line of the text, counted from 1, where it stands.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TableError {
    pub fault: Fault,
    pub line: usize,
}

impl TableError {
    /// `<fault>:<line>`, the detail of `REFUSED table`.
    pub fn detail(&self) -> String {
        format!("{}:{}", self.fault.as_str(), self.line)
    }
}

/// An outcome table that the reader accepts.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Table {
    rows: Vec<Row>,
}

impl Table {
    /// Reads a table and refuses: a carriage return, a text with no last LF or a blank line
    /// (`text`); another header (`header`); a cell that is not of its form (`cell`); a `split` that
    /// does not fit its `seed` (`split`); generated and recorded rows in one table (`mixed`); rows out
    /// of byte order (`order`); two rows of one key (`key`); and two levels for one tier and model
    /// of a model cell (`level`).
    pub fn parse(text: &str) -> Result<Table, TableError> {
        let fail = |fault, line| Err(TableError { fault, line });
        if let Some(at) = text.find('\r') {
            return fail(Fault::Text, text[..at].matches('\n').count() + 1);
        }
        if !text.ends_with('\n') {
            return fail(Fault::Text, text.matches('\n').count() + 1);
        }
        let mut lines = text.split_terminator('\n');
        if lines.next() != Some(HEADER) {
            return fail(Fault::Header, 1);
        }
        let mut rows: Vec<Row> = Vec::new();
        let mut previous: Option<&str> = None;
        let mut keys = HashSet::new();
        let mut levels: HashMap<(CategoryId, CallKind, Tier, String), Level> = HashMap::new();
        for (i, line) in lines.enumerate() {
            let number = i + 2;
            if line.is_empty() {
                return fail(Fault::Text, number);
            }
            let (row, split) = read_row(line).ok_or(TableError {
                fault: Fault::Cell,
                line: number,
            })?;
            if split != row.split() {
                return fail(Fault::Split, number);
            }
            if rows
                .first()
                .is_some_and(|first| first.is_generated() != row.is_generated())
            {
                return fail(Fault::Mixed, number);
            }
            match previous {
                Some(before) if line < before => return fail(Fault::Order, number),
                _ => {}
            }
            previous = Some(line);
            if !keys.insert(row.key()) {
                return fail(Fault::Key, number);
            }
            if row.category.is_seed() && row.kind != CallKind::Replay {
                let level = *levels
                    .entry((
                        row.category,
                        row.kind,
                        row.tier,
                        row.model.as_str().to_owned(),
                    ))
                    .or_insert(row.level);
                if level != row.level {
                    return fail(Fault::Level, number);
                }
            }
            rows.push(row);
        }
        Ok(Table { rows })
    }

    /// The table of these rows, checked as the reader checks the text that they make.
    pub fn from_rows(rows: &[Row]) -> Result<Table, TableError> {
        Table::parse(&text_of(rows))
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// True when the first row has the source `generated`; a table with no row is recorded.
    pub fn is_generated(&self) -> bool {
        self.rows.first().is_some_and(Row::is_generated)
    }

    pub fn to_text(&self) -> String {
        text_of(&self.rows)
    }
}

fn text_of(rows: &[Row]) -> String {
    let mut text = String::with_capacity(96 * (rows.len() + 1));
    text.push_str(HEADER);
    text.push('\n');
    for row in rows {
        text.push_str(&row.to_line());
        text.push('\n');
    }
    text
}

fn figure(cell: &str) -> Option<u64> {
    let canonical = !cell.is_empty()
        && cell.bytes().all(|b| b.is_ascii_digit())
        && (cell == "0" || !cell.starts_with('0'));
    if canonical {
        cell.parse().ok()
    } else {
        None
    }
}

fn read_row(line: &str) -> Option<(Row, Split)> {
    let cells: Vec<&str> = line.split('\t').collect();
    if cells.len() != CELLS {
        return None;
    }
    let pass = match cells[9] {
        "yes" => true,
        "no" => false,
        _ => return None,
    };
    let row = Row {
        task: Task::parse(cells[0]).ok()?,
        category: CategoryId::parse(cells[1]).ok()?,
        kind: CallKind::parse(cells[2]).ok()?,
        tier: Tier::parse(cells[3]).ok()?,
        model: Model::parse(cells[4]).ok()?,
        machine: Machine::parse(cells[5]).ok()?,
        seed: Seed::parse(cells[6]).ok()?,
        attempt: NonZeroU32::new(u32::try_from(figure(cells[8])?).ok()?)?,
        pass,
        steps: figure(cells[10])?,
        wall_s: figure(cells[11])?,
        tokens_in: figure(cells[12])?,
        tokens_out: figure(cells[13])?,
        level: Level::parse(cells[14]).ok()?,
        source: Source::parse(cells[15]).ok()?,
    };
    let split = Split::parse(cells[7]).ok()?;
    let generated = row.task == Task::Gen;
    let fits = generated == (row.model.as_str() == SYNTHETIC)
        && if generated {
            matches!(row.machine, Machine::GenA | Machine::GenB) && row.source == Source::Generated
        } else {
            matches!(row.machine, Machine::Mac | Machine::HostedWin)
                && row.source != Source::Generated
        };
    fits.then_some((row, split))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed<T: Copy + Eq + std::fmt::Debug>(
        all: &[T],
        word: fn(T) -> &'static str,
        parse: fn(&str) -> Result<T, ParseError>,
    ) {
        let listed = |text: &str| all.iter().copied().find(|value| word(*value) == text);
        let check = |text: &str| assert_eq!(parse(text).ok(), listed(text), "{text:?}");
        for (i, value) in all.iter().enumerate() {
            let text = word(*value);
            assert!(
                all[..i].iter().all(|other| word(*other) != text),
                "{text} twice"
            );
            assert_eq!(parse(text), Ok(*value));
            let bytes = text.as_bytes();
            for at in 0..=bytes.len() {
                for byte in 0..=127u8 {
                    let mut inserted = bytes.to_vec();
                    inserted.insert(at, byte);
                    check(std::str::from_utf8(&inserted).expect("ascii"));
                    if at < bytes.len() {
                        let mut replaced = bytes.to_vec();
                        replaced[at] = byte;
                        check(std::str::from_utf8(&replaced).expect("ascii"));
                    }
                }
                check(&text[..at]);
                check(&text[at..]);
            }
            check(&text.to_uppercase());
            check(&format!("{text}\n"));
        }
        for text in ["", "-", " ", "\u{feff}"] {
            check(text);
        }
    }

    #[test]
    fn word_lists_are_closed() {
        closed(Task::ALL, Task::as_str, Task::parse);
        closed(Machine::ALL, Machine::as_str, Machine::parse);
        closed(Seed::ALL, Seed::as_str, Seed::parse);
        closed(Split::ALL, Split::as_str, Split::parse);
        closed(Source::ALL, Source::as_str, Source::parse);
        assert_eq!(
            Task::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            ["bill", "reply", "order", "cleanup", "fill", "gen"]
        );
        assert_eq!(
            Machine::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            ["mac", "hosted-win", "gen_a", "gen_b"]
        );
        assert_eq!(
            Seed::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            ["1", "2", "3"]
        );
        assert_eq!(
            Split::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            ["dev", "held"]
        );
        assert_eq!(
            Source::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            ["s1", "s2", "generated"]
        );
    }

    #[test]
    fn model_tokens() {
        for token in ["a", "synthetic", "m_small", "qwen3_5_9b", &"z".repeat(32)] {
            assert_eq!(
                Model::parse(token).map(|model| model.as_str().to_owned()),
                Ok(token.to_owned())
            );
        }
        for text in [
            "",
            "M",
            "m-small",
            "m small",
            "m.small",
            "\u{e9}",
            &"z".repeat(33),
        ] {
            assert_eq!(Model::parse(text), Err(ParseError), "{text:?}");
        }
    }
}
