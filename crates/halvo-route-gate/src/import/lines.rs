use std::num::NonZeroU32;

use halvo_route::tier::{CallKind, CategoryId, Level, Tier};

use crate::table::{Machine, Model, Row, Seed, Source, Task, SYNTHETIC};

/// A line of a block that starts with this word is an attempt line in the driver file and is
/// refused in the cells file.
pub const ATTEMPT: &[u8] = b"attempt";
/// A line of a block that starts with this word is the verdict line of its file.
pub const VERDICT: &[u8] = b"verdict";
/// A line of a block that starts with this word is a bound line in the cells file and is refused
/// in the driver file.
pub const BOUND: &[u8] = b"bound";

/// The known words of the driver block.
pub const DRIVER_WORDS: [&[u8]; 2] = [ATTEMPT, VERDICT];
/// The known words of the cells block.
pub const CELLS_WORDS: [&[u8]; 2] = [BOUND, VERDICT];

/// Whether `line` is a line of one of `words` written another way: after any spaces and tabs at
/// its start, its bytes start with the word compared without ASCII letter case, and the line does
/// not start with the word in its own letters at its first byte. Such a line is the refusal
/// `source` with its line number (T5.4).
pub fn written_another_way(line: &[u8], words: &[&[u8]]) -> bool {
    words
        .iter()
        .any(|word| starts_folded(after_blanks(line), word) && !line.starts_with(word))
}

fn after_blanks(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .take_while(|byte| **byte == b' ' || **byte == b'\t')
        .count();
    &line[start..]
}

fn starts_folded(bytes: &[u8], word: &[u8]) -> bool {
    bytes
        .get(..word.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(word))
}

/// The tasks that a `limits` line names (AS-A13.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    /// The line holds `task=bill` or `task=order`, the tasks of the verdict's driver.
    pub drive: bool,
    /// The line holds `task=reply`, the task of the verdict's writer.
    pub reply: bool,
}

/// A `limits` line with the tasks that it names, read by its words: words are divided by spaces
/// and tabs, the first word is `limits` and a later word names a task as `task=bill`, `task=order` or
/// `task=reply`, each compared without ASCII letter case. In the driver file a `limits` line that
/// names `bill` or `order` while the verdict's driver is `none`, or `reply` while its writer is
/// `none`, is the refusal `source` with its line number (T5.4).
pub fn limits(line: &[u8]) -> Option<Limits> {
    let mut words = line
        .split(|byte| *byte == b' ' || *byte == b'\t')
        .filter(|word| !word.is_empty());
    if !words.next()?.eq_ignore_ascii_case(b"limits") {
        return None;
    }
    let words: Vec<&[u8]> = words.collect();
    let names = |task: &[u8]| words.iter().any(|word| word.eq_ignore_ascii_case(task));
    Some(Limits {
        drive: names(b"task=bill") || names(b"task=order"),
        reply: names(b"task=reply"),
    })
}

/// The models of the routing run's group (T12.1); every other model is of the group `agents`.
pub const ROUTING: [&str; 4] = [
    "s5_nano_4b",
    "gemma_4_e4b",
    "gemma_4_e2b",
    "ternary_bonsai_2_9b",
];

/// The key of a row of the outcome table among the attempt lines: task, kind, model, machine,
/// seed and `i`; the category, tier and level follow from these or are constant.
pub type AttemptKey = (Task, CallKind, Model, Machine, Seed, NonZeroU32);

/// The `path` of a verdict line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path {
    A,
    B,
}

/// A verdict line: the driver and the writer, each a model or none.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub driver: Option<Model>,
    pub path: Path,
    pub writer: Option<Model>,
}

/// The `fits` of a bound line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fits {
    Yes,
    No,
    NotMeasured,
}

impl Fits {
    pub const fn as_str(self) -> &'static str {
        match self {
            Fits::Yes => "yes",
            Fits::No => "no",
            Fits::NotMeasured => "not-measured",
        }
    }
}

/// A bound line: one row of models.tsv.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Bound {
    pub model: Model,
    pub machine: Machine,
    pub bound_bytes: u64,
    pub fits: Fits,
}

impl Bound {
    /// The row of models.tsv, without its LF.
    pub fn to_line(&self) -> String {
        let group = if ROUTING.contains(&self.model.as_str()) {
            "routing"
        } else {
            "agents"
        };
        format!(
            "{}\t{}\t{}\t{}\t{group}\ts2",
            self.model.as_str(),
            self.machine.as_str(),
            self.bound_bytes,
            self.fits.as_str()
        )
    }
}

fn values<'a>(line: &'a [u8], head: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let rest = std::str::from_utf8(line).ok()?.strip_prefix(head)?;
    let words: Vec<&str> = rest.split(' ').collect();
    if words.len() != keys.len() {
        return None;
    }
    words
        .iter()
        .zip(keys)
        .map(|(word, key)| word.strip_prefix(key)?.strip_prefix('='))
        .collect()
}

fn model(text: &str) -> Option<Model> {
    if text == SYNTHETIC || text == "none" {
        return None;
    }
    Model::parse(text).ok()
}

fn model_or_none(text: &str) -> Option<Option<Model>> {
    if text == "none" {
        Some(None)
    } else {
        model(text).map(Some)
    }
}

fn machine(text: &str) -> Option<Machine> {
    match Machine::parse(text).ok()? {
        machine @ (Machine::Mac | Machine::HostedWin) => Some(machine),
        Machine::GenA | Machine::GenB => None,
    }
}

fn figure(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if canonical {
        text.parse().ok()
    } else {
        None
    }
}

fn category(task: Task, kind: CallKind) -> Option<CategoryId> {
    match (task, kind) {
        (Task::Bill, CallKind::Compile | CallKind::Replay | CallKind::Recover) => {
            Some(CategoryId::UtilityBillPayment)
        }
        (Task::Order, CallKind::Compile | CallKind::Replay | CallKind::Recover) => {
            Some(CategoryId::ShopOrder)
        }
        (Task::Reply, CallKind::Write) => Some(CategoryId::Email),
        _ => None,
    }
}

/// `attempt model=<token> machine=<m> task=<t> seed=<n> kind=<k> i=<n> pass=<yes|no> steps=<n>
/// wall_s=<n> tokens_in=<n> tokens_out=<n>`: one recorded row with the tier `on_device`, the level
/// `medium` and the source `s1`. The task and the kind are `bill` or `order` with `compile`,
/// `replay` or `recover`, or `reply` with `write`, the cells that the trial runs (AS-A13.6, T4.4).
pub fn attempt(line: &[u8]) -> Option<Row> {
    let v = values(
        line,
        "attempt ",
        &[
            "model",
            "machine",
            "task",
            "seed",
            "kind",
            "i",
            "pass",
            "steps",
            "wall_s",
            "tokens_in",
            "tokens_out",
        ],
    )?;
    let task = Task::parse(v[2]).ok()?;
    let kind = CallKind::parse(v[4]).ok()?;
    let pass = match v[6] {
        "yes" => true,
        "no" => false,
        _ => return None,
    };
    Some(Row {
        task,
        category: category(task, kind)?,
        kind,
        tier: Tier::OnDevice,
        model: model(v[0])?,
        machine: machine(v[1])?,
        seed: Seed::parse(v[3]).ok()?,
        attempt: NonZeroU32::new(u32::try_from(figure(v[5])?).ok()?)?,
        pass,
        steps: figure(v[7])?,
        wall_s: figure(v[8])?,
        tokens_in: figure(v[9])?,
        tokens_out: figure(v[10])?,
        level: Level::Medium,
        source: Source::S1,
    })
}

/// Whether the `i` of a row keeps the numbering by itself: `compile`, `recover` and `write` have
/// no `i` but 1, and `replay` has `i` from 1 to 3.
pub fn numbered(row: &Row) -> bool {
    match row.kind {
        CallKind::Replay => row.attempt.get() <= 3,
        CallKind::Compile | CallKind::Recover | CallKind::Write => row.attempt.get() == 1,
    }
}

/// The key of a row that `attempt` made.
pub fn attempt_key(row: &Row) -> AttemptKey {
    (
        row.task,
        row.kind,
        row.model.clone(),
        row.machine,
        row.seed,
        row.attempt,
    )
}

/// `verdict driver=<token|none> path=<A|B> writer=<token|none>`. The path is `A` exactly when a
/// driver is named, and under `A` the writer is the driver or none (AS-A13.6).
pub fn verdict(line: &[u8]) -> Option<Verdict> {
    let v = values(line, "verdict ", &["driver", "path", "writer"])?;
    let driver = model_or_none(v[0])?;
    let path = match v[1] {
        "A" => Path::A,
        "B" => Path::B,
        _ => return None,
    };
    let writer = model_or_none(v[2])?;
    let fits_path = match path {
        Path::A => driver.is_some() && (writer.is_none() || writer == driver),
        Path::B => driver.is_none(),
    };
    fits_path.then_some(Verdict {
        driver,
        path,
        writer,
    })
}

/// The start of the verdict line of the cells file, `verdict candidates=<list>` (AS-A13.5), whose
/// list the importer does not read.
pub const CANDIDATES: &[u8] = b"verdict candidates=";

/// `bound model=<token> machine=<m> bound_bytes=<n> fits=<yes|no|not-measured>`.
pub fn bound(line: &[u8]) -> Option<Bound> {
    let v = values(line, "bound ", &["model", "machine", "bound_bytes", "fits"])?;
    let fits = match v[3] {
        "yes" => Fits::Yes,
        "no" => Fits::No,
        "not-measured" => Fits::NotMeasured,
        _ => return None,
    };
    Some(Bound {
        model: model(v[0])?,
        machine: machine(v[1])?,
        bound_bytes: figure(v[2])?,
        fits,
    })
}

/// The start of a `drive` line of AS-A13.6.
pub const DRIVE: &[u8] = b"drive model=";
/// The start of a `reply` line of AS-A13.6.
pub const REPLY: &[u8] = b"reply model=";

/// The model that a line which starts with `head` names, the bytes after `head` up to the next
/// space or the end of the line.
pub fn named<'a>(line: &'a [u8], head: &[u8]) -> Option<&'a [u8]> {
    let rest = line.strip_prefix(head)?;
    Some(rest.split(|byte| *byte == b' ').next().unwrap_or(rest))
}

/// A cell of the trial: the `drive` line of one model, machine, task and seed with its five
/// attempt lines, or the `reply` line of one model, machine and seed with its one `write` line.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Cell {
    Drive(Model, Machine, Task, Seed),
    Reply(Model, Machine, Seed),
}

/// The cell of a counted `drive` or `reply` line (T5.4, U0.2c): a line whose first bytes are
/// `drive model=<token> machine=<m> task=<bill|order> seed=<n> ` or `reply model=<token>
/// machine=<m> seed=<n> `, with values of T5.1 and T5.2. The rest of the line is not read.
pub fn counted(line: &[u8]) -> Option<Cell> {
    let heads = |head: &str, keys: &[&str]| -> Option<Vec<String>> {
        let rest = std::str::from_utf8(line).ok()?.strip_prefix(head)?;
        let mut words = rest.splitn(keys.len() + 1, ' ');
        let values = keys
            .iter()
            .map(|key| {
                let value = words.next()?.strip_prefix(key)?.strip_prefix('=')?;
                Some(value.to_owned())
            })
            .collect::<Option<Vec<String>>>()?;
        words.next().map(|_| values)
    };
    if let Some(v) = heads("drive ", &["model", "machine", "task", "seed"]) {
        let task = match Task::parse(&v[2]).ok()? {
            task @ (Task::Bill | Task::Order) => task,
            _ => return None,
        };
        return Some(Cell::Drive(
            model(&v[0])?,
            machine(&v[1])?,
            task,
            Seed::parse(&v[3]).ok()?,
        ));
    }
    let v = heads("reply ", &["model", "machine", "seed"])?;
    Some(Cell::Reply(
        model(&v[0])?,
        machine(&v[1])?,
        Seed::parse(&v[2]).ok()?,
    ))
}

/// A `drive` line: one cell of the driver trial, the five runs of one task, machine and seed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Drive {
    pub machine: Machine,
    pub task: Task,
    pub seed: Seed,
    pub compile: bool,
    pub replays_ok: u64,
    pub recover: bool,
    pub typed_credential: u64,
    pub effects_without_confirm: u64,
}

fn ok_or_miss(text: &str) -> Option<bool> {
    match text {
        "ok" => Some(true),
        "miss" => Some(false),
        _ => None,
    }
}

/// `drive model=<token> machine=<m> task=<t> seed=<n> compile=<ok|miss> replays_ok=<n> of=3
/// recover=<ok|miss> steps=<n> wall_s=<n> model_tokens=<n> typed_credential=<n>
/// effects_without_confirm=<n>` (AS-A13.6), read key by key as an attempt line is.
pub fn drive(line: &[u8]) -> Option<Drive> {
    let v = values(
        line,
        "drive ",
        &[
            "model",
            "machine",
            "task",
            "seed",
            "compile",
            "replays_ok",
            "of",
            "recover",
            "steps",
            "wall_s",
            "model_tokens",
            "typed_credential",
            "effects_without_confirm",
        ],
    )?;
    model(v[0])?;
    let task = match Task::parse(v[2]).ok()? {
        Task::Gen => return None,
        task => task,
    };
    (v[6] == "3").then_some(())?;
    figure(v[8])?;
    figure(v[9])?;
    figure(v[10])?;
    Some(Drive {
        machine: machine(v[1])?,
        task,
        seed: Seed::parse(v[3]).ok()?,
        compile: ok_or_miss(v[4])?,
        replays_ok: figure(v[5])?,
        recover: ok_or_miss(v[7])?,
        typed_credential: figure(v[11])?,
        effects_without_confirm: figure(v[12])?,
    })
}

/// A `reply` line: the reply run of one machine and seed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reply {
    pub machine: Machine,
    pub seed: Seed,
    pub bodies: u64,
}

/// `reply model=<token> machine=<m> seed=<n> bodies=<n> of=2 wall_s=<n> model_tokens=<n>`
/// (AS-A13.6), read key by key as an attempt line is.
pub fn reply(line: &[u8]) -> Option<Reply> {
    let v = values(
        line,
        "reply ",
        &[
            "model",
            "machine",
            "seed",
            "bodies",
            "of",
            "wall_s",
            "model_tokens",
        ],
    )?;
    model(v[0])?;
    (v[4] == "2").then_some(())?;
    figure(v[5])?;
    figure(v[6])?;
    Some(Reply {
        machine: machine(v[1])?,
        seed: Seed::parse(v[2]).ok()?,
        bodies: figure(v[3])?,
    })
}

#[cfg(test)]
mod tests {
    use super::attempt;

    fn replay(i: &str) -> Vec<u8> {
        format!("attempt model=m_a machine=mac task=bill seed=1 kind=replay i={i} pass=yes steps=1 wall_s=1 tokens_in=1 tokens_out=1").into_bytes()
    }

    #[test]
    fn the_line_parse_takes_i_up_to_4294967295_and_refuses_above_with_no_wrap() {
        let parsed = |i: &str| attempt(&replay(i)).map(|row| row.attempt.get());
        assert_eq!(parsed("3"), Some(3));
        assert_eq!(parsed("4294967295"), Some(4_294_967_295));
        assert_eq!(parsed("4294967296"), None);
        assert_eq!(parsed("4294967297"), None);
    }
}
