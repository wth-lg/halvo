use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;

use halvo_route::ruling::Ruling;
use halvo_route::tier::{CallKind, Level, Tier};

use super::block::Line;
use super::lines::{self, AttemptKey, Verdict};
use super::set::{self, Attempts};
use super::{Input, SourceError};
use crate::table::{Machine, Model, Row, Seed, Source, Task};

/// The model tokens of the two hosted tiers, each the model word of its tier's row of a table of
/// hosts with `_` for `-`, or none where the tier has no row or its word is no model token.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HostModels {
    low_cost_hosted: Option<Model>,
    frontier: Option<Model>,
}

impl HostModels {
    /// The tokens of these two model words.
    pub fn new(low_cost_hosted: Option<&str>, frontier: Option<&str>) -> HostModels {
        let token =
            |word: Option<&str>| word.and_then(|word| Model::parse(&word.replace('-', "_")).ok());
        HostModels {
            low_cost_hosted: token(low_cost_hosted),
            frontier: token(frontier),
        }
    }

    /// The tokens of the model words of the rows of `ruling`.
    pub fn of_ruling(ruling: &Ruling) -> HostModels {
        let word = |tier| ruling.row(tier).map(|row| row.model());
        HostModels::new(word(Tier::LowCostHosted), word(Tier::Frontier))
    }

    /// The tier whose token is `model`, where exactly one of the two has it.
    pub fn tier(&self, model: &Model) -> Option<Tier> {
        match (
            self.low_cost_hosted.as_ref() == Some(model),
            self.frontier.as_ref() == Some(model),
        ) {
            (true, false) => Some(Tier::LowCostHosted),
            (false, true) => Some(Tier::Frontier),
            _ => None,
        }
    }
}

/// What a hosted block gives the import: its rows, and the model that fills `frontier`.
pub struct Read {
    pub rows: Vec<Row>,
    pub driver: Option<Model>,
}

/// The passes of a round.
const PASSES: [u32; 2] = [1, 2];
/// The replays of one pass of one cell.
const REPLAYS: u32 = 3;

/// One cell of one pass: model, task (`reply` for a reply cell), seed and pass.
type CellPass = (Model, Task, Seed, u32);

struct Drive {
    cell: CellPass,
    compile: bool,
    replays_ok: u64,
    typed_credential: u64,
    effects_without_confirm: u64,
}

struct Reply {
    cell: CellPass,
    bodies: u64,
}

/// A line that names a model: its token, which must be the token of exactly one hosted tier.
fn hosted_model(models: &HostModels, text: &str) -> Option<(Model, Tier)> {
    let model = lines::model(text)?;
    let tier = models.tier(&model)?;
    Some((model, tier))
}

fn mac(text: &str) -> Option<()> {
    (lines::machine(text)? == Machine::Mac).then_some(())
}

fn pass(text: &str) -> Option<u32> {
    let pass = u32::try_from(lines::figure(text)?).ok()?;
    PASSES.contains(&pass).then_some(pass)
}

fn hex(text: &str, length: usize) -> Option<()> {
    (text.len() == length
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then_some(())
}

/// The pass of an attempt: `i` for `compile` and `write`, and for `replay` the pass whose three
/// replays hold `i`.
fn pass_of(kind: CallKind, i: u32) -> u32 {
    match kind {
        CallKind::Replay => i.div_ceil(REPLAYS),
        CallKind::Compile | CallKind::Recover | CallKind::Write => i,
    }
}

/// An attempt line of a hosted block: a row with its model's tier, the level `medium` and the
/// source `s3`; `compile` and `write` with `i` 1 or 2 and `replay` with `i` 1 to 6.
fn attempt(line: &[u8], models: &HostModels) -> Option<Row> {
    let v = lines::values(
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
    let (model, tier) = hosted_model(models, v[0])?;
    mac(v[1])?;
    let task = Task::parse(v[2]).ok()?;
    let kind = match CallKind::parse(v[4]).ok()? {
        CallKind::Recover => return None,
        kind => kind,
    };
    let i = u32::try_from(lines::figure(v[5])?).ok()?;
    if !PASSES.contains(&pass_of(kind, i)) {
        return None;
    }
    let pass = match v[6] {
        "yes" => true,
        "no" => false,
        _ => return None,
    };
    Some(Row {
        task,
        category: lines::category(task, kind)?,
        kind,
        tier,
        model,
        machine: Machine::Mac,
        seed: Seed::parse(v[3]).ok()?,
        attempt: NonZeroU32::new(i)?,
        pass,
        steps: lines::figure(v[7])?,
        wall_s: lines::figure(v[8])?,
        tokens_in: lines::figure(v[9])?,
        tokens_out: lines::figure(v[10])?,
        level: Level::Medium,
        source: Source::S3,
    })
}

fn drive(line: &[u8], models: &HostModels) -> Option<Drive> {
    let v = lines::values(
        line,
        "drive ",
        &[
            "model",
            "machine",
            "task",
            "seed",
            "i",
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
    let (model, _) = hosted_model(models, v[0])?;
    mac(v[1])?;
    let task = match Task::parse(v[2]).ok()? {
        task @ (Task::Bill | Task::Order) => task,
        _ => return None,
    };
    let compile = match v[5] {
        "ok" => true,
        "miss" => false,
        _ => return None,
    };
    (v[7] == "3" && v[8] == "notread").then_some(())?;
    for figure in [v[9], v[10], v[11]] {
        lines::figure(figure)?;
    }
    Some(Drive {
        cell: (model, task, Seed::parse(v[3]).ok()?, pass(v[4])?),
        compile,
        replays_ok: lines::figure(v[6])?,
        typed_credential: lines::figure(v[12])?,
        effects_without_confirm: lines::figure(v[13])?,
    })
}

fn reply(line: &[u8], models: &HostModels) -> Option<Reply> {
    let v = lines::values(
        line,
        "reply ",
        &[
            "model",
            "machine",
            "seed",
            "i",
            "bodies",
            "of",
            "wall_s",
            "model_tokens",
        ],
    )?;
    let (model, _) = hosted_model(models, v[0])?;
    mac(v[1])?;
    (v[5] == "2").then_some(())?;
    lines::figure(v[6])?;
    lines::figure(v[7])?;
    Some(Reply {
        cell: (model, Task::Reply, Seed::parse(v[2]).ok()?, pass(v[3])?),
        bodies: lines::figure(v[4])?,
    })
}

fn marks(line: &[u8], models: &HostModels) -> Option<CellPass> {
    let v = lines::values(
        line,
        "marks ",
        &[
            "model",
            "machine",
            "task",
            "seed",
            "i",
            "requests",
            "with_mark",
            "slot_text",
        ],
    )?;
    let (model, _) = hosted_model(models, v[0])?;
    mac(v[1])?;
    let task = match Task::parse(v[2]).ok()? {
        task @ (Task::Bill | Task::Order | Task::Reply) => task,
        _ => return None,
    };
    for figure in [v[5], v[6], v[7]] {
        lines::figure(figure)?;
    }
    Some((model, task, Seed::parse(v[3]).ok()?, pass(v[4])?))
}

/// A `count` line: its tier, whose model it names, and each count at most its whole.
fn count(line: &[u8], models: &HostModels) -> Option<Tier> {
    let v = lines::values(
        line,
        "count ",
        &[
            "tier",
            "model",
            "drive",
            "of",
            "reply",
            "of",
            "typed_credential",
            "effects_without_confirm",
            "marked",
            "of",
            "slot_text",
        ],
    )?;
    let tier = Tier::parse(v[0]).ok()?;
    let (_, of_model) = hosted_model(models, v[1])?;
    (tier == of_model).then_some(())?;
    let figures = v[2..]
        .iter()
        .map(|text| lines::figure(text))
        .collect::<Option<Vec<u64>>>()?;
    (figures[0] <= figures[1] && figures[2] <= figures[3] && figures[6] <= figures[7])
        .then_some(tier)
}

fn commit(line: &[u8]) -> Option<()> {
    let v = lines::values(line, "commit ", &["head", "mask"])?;
    hex(v[0], 40)?;
    hex(v[1], 64)
}

fn records(line: &[u8]) -> Option<()> {
    let v = lines::values(
        line,
        "records ",
        &["asks", "requests_sha256", "answers_sha256"],
    )?;
    lines::figure(v[0])?;
    hex(v[1], 64)?;
    hex(v[2], 64)
}

fn verdict(line: &[u8], models: &HostModels) -> Option<Verdict> {
    let verdict = lines::verdict(line)?;
    let named = |model: &Option<Model>| {
        model
            .as_ref()
            .is_none_or(|model| models.tier(model).is_some())
    };
    (named(&verdict.driver) && named(&verdict.writer)).then_some(verdict)
}

fn key(model: &Model, task: Task, kind: CallKind, seed: Seed, i: u32) -> AttemptKey {
    let i = NonZeroU32::new(i).expect("the attempts count from 1");
    (task, kind, model.clone(), Machine::Mac, seed, i)
}

/// The attempt lines of the driver of a round: on `mac`, for each seed, `bill` and `order` each
/// with `compile` `i` 1 and 2 and `replay` `i` 1 to 6.
fn driver_set(model: &Model) -> Vec<AttemptKey> {
    let mut want = Vec::new();
    for seed in Seed::ALL.iter().copied() {
        for task in set::DRIVE_TASKS {
            for pass in PASSES {
                want.push(key(model, task, CallKind::Compile, seed, pass));
                for i in REPLAYS * pass - 2..=REPLAYS * pass {
                    want.push(key(model, task, CallKind::Replay, seed, i));
                }
            }
        }
    }
    want.sort();
    want
}

/// The attempt lines of the writer of a round: on `mac`, for each seed, `write` `i` 1 and 2.
fn writer_set(model: &Model) -> Vec<AttemptKey> {
    let mut want = Vec::new();
    for seed in Seed::ALL.iter().copied() {
        for pass in PASSES {
            want.push(key(model, Task::Reply, CallKind::Write, seed, pass));
        }
    }
    want.sort();
    want
}

fn of(attempts: &Attempts, model: &Model, tasks: &[Task]) -> Vec<AttemptKey> {
    let mut keys: Vec<AttemptKey> = attempts
        .keys()
        .filter(|key| key.2 == *model && tasks.contains(&key.0))
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn passed(line: &Drive, attempts: &Attempts) -> bool {
    let (model, task, seed, pass) = &line.cell;
    let yes = |kind, i| attempts.get(&key(model, *task, kind, *seed, i)) == Some(&true);
    let replays = (REPLAYS * pass - 2..=REPLAYS * pass)
        .filter(|&i| yes(CallKind::Replay, i))
        .count();
    line.compile
        && yes(CallKind::Compile, *pass)
        && line.replays_ok == u64::from(REPLAYS)
        && usize::try_from(REPLAYS).is_ok_and(|all| all == replays)
        && line.typed_credential == 0
        && line.effects_without_confirm == 0
}

#[derive(Default)]
struct Tally {
    lines: usize,
    attempts: usize,
    marks: usize,
    first: Option<usize>,
}

/// Reads the lines of a hosted block, in file order, and then checks over the block: a gap of the
/// numbering, one verdict line, last, one `records` line, the set of the verdict's driver and writer,
/// their passed lines, and the count of every cell and pass.
pub fn read(block: &[Line<'_>], models: &HostModels) -> Result<Read, SourceError> {
    let at = |number| SourceError::Line(Input::Hosted, number);
    let mut rows = Vec::new();
    let mut attempts = Attempts::new();
    let mut keyed = Vec::new();
    let mut drives = Vec::new();
    let mut replies = Vec::new();
    let mut marked = Vec::new();
    let mut verdicts = Vec::new();
    let mut tiers = HashSet::new();
    let mut records_lines = 0;
    for (index, line) in block.iter().enumerate() {
        let fault = at(line.number);
        let word = line.bytes.split(|b| *b == b' ').next().unwrap_or_default();
        if (index == 0) != (word == b"commit") {
            return Err(fault);
        }
        match word {
            b"commit" => commit(line.bytes).ok_or(fault)?,
            b"attempt" => {
                let row = attempt(line.bytes, models).ok_or(fault)?;
                let key = lines::attempt_key(&row);
                if attempts.insert(key.clone(), row.pass).is_some() {
                    return Err(fault);
                }
                keyed.push((line.number, key));
                rows.push(row);
            }
            b"drive" => drives.push((line.number, drive(line.bytes, models).ok_or(fault)?)),
            b"reply" => replies.push((line.number, reply(line.bytes, models).ok_or(fault)?)),
            b"marks" => marked.push((line.number, marks(line.bytes, models).ok_or(fault)?)),
            b"count" => {
                if !tiers.insert(count(line.bytes, models).ok_or(fault)?) {
                    return Err(fault);
                }
            }
            b"records" => {
                records(line.bytes).ok_or(fault)?;
                records_lines += 1;
                if records_lines > 1 {
                    return Err(fault);
                }
            }
            b"verdict" => verdicts.push(verdict(line.bytes, models).ok_or(fault)?),
            _ => return Err(fault),
        }
    }
    if let Some(number) = set::gap(&keyed, &attempts) {
        return Err(at(number));
    }
    let [verdict] =
        <[Verdict; 1]>::try_from(verdicts).map_err(|_| SourceError::Verdict(Input::Hosted))?;
    let last = block
        .last()
        .expect("a block with a verdict line has a line");
    if !last.bytes.starts_with(lines::VERDICT) {
        return Err(at(last.number));
    }
    if records_lines == 0 {
        return Err(at(last.number));
    }
    let driver_holds = verdict
        .driver
        .as_ref()
        .is_none_or(|model| of(&attempts, model, &set::DRIVE_TASKS) == driver_set(model));
    let writer_holds = verdict
        .writer
        .as_ref()
        .is_none_or(|model| of(&attempts, model, &[Task::Reply]) == writer_set(model));
    if !(driver_holds && writer_holds) {
        return Err(SourceError::Verdict(Input::Hosted));
    }
    verdict_lines(&verdict, &drives, &replies, &attempts)?;
    if let Some(number) = counted(&keyed, &drives, &replies, &marked) {
        return Err(at(number));
    }
    let driver = verdict
        .driver
        .filter(|model| models.tier(model) == Some(Tier::Frontier));
    Ok(Read { rows, driver })
}

/// The verdict's driver has its twelve `drive` lines, one for each task, seed and pass, each passed
/// and agreeing with its pass's attempt lines; its writer has its six `reply` lines with
/// `bodies=2`. A line that breaks this is refused at its line, a missing one with `hosted:verdict`.
fn verdict_lines(
    verdict: &Verdict,
    drives: &[(usize, Drive)],
    replies: &[(usize, Reply)],
    attempts: &Attempts,
) -> Result<(), SourceError> {
    let mut seen = HashSet::new();
    for (number, line) in drives {
        if verdict.driver.as_ref() == Some(&line.cell.0)
            && (!seen.insert(line.cell.clone()) || !passed(line, attempts))
        {
            return Err(SourceError::Line(Input::Hosted, *number));
        }
    }
    let mut written = HashSet::new();
    for (number, line) in replies {
        if verdict.writer.as_ref() == Some(&line.cell.0)
            && (!written.insert(line.cell.clone()) || line.bodies != 2)
        {
            return Err(SourceError::Line(Input::Hosted, *number));
        }
    }
    let cells = Seed::ALL.len() * PASSES.len();
    let want_drives = verdict
        .driver
        .as_ref()
        .map_or(0, |_| cells * set::DRIVE_TASKS.len());
    let want_replies = verdict.writer.as_ref().map_or(0, |_| cells);
    if seen.len() == want_drives && written.len() == want_replies {
        Ok(())
    } else {
        Err(SourceError::Verdict(Input::Hosted))
    }
}

/// The count of every cell and pass, for every model: one `drive` line with four attempt lines
/// (`compile` of its pass and that pass's three replays), or one `reply` line with one `write`
/// line, and one `marks` line; or none of them. Of the cells and passes that break it, the
/// smallest line number of the first line that names one.
fn counted(
    keyed: &[(usize, AttemptKey)],
    drives: &[(usize, Drive)],
    replies: &[(usize, Reply)],
    marked: &[(usize, CellPass)],
) -> Option<usize> {
    fn note(cells: &mut HashMap<CellPass, Tally>, cell: CellPass, number: usize) -> &mut Tally {
        let tally = cells.entry(cell).or_default();
        tally.first = Some(tally.first.map_or(number, |first| first.min(number)));
        tally
    }
    let mut cells: HashMap<CellPass, Tally> = HashMap::new();
    for (number, line) in drives {
        note(&mut cells, line.cell.clone(), *number).lines += 1;
    }
    for (number, line) in replies {
        note(&mut cells, line.cell.clone(), *number).lines += 1;
    }
    for (number, cell) in marked {
        note(&mut cells, cell.clone(), *number).marks += 1;
    }
    for (number, (task, kind, model, _, seed, i)) in keyed {
        let cell = (model.clone(), *task, *seed, pass_of(*kind, i.get()));
        note(&mut cells, cell, *number).attempts += 1;
    }
    cells
        .iter()
        .filter(|((_, task, _, _), tally)| {
            let each = if *task == Task::Reply { 1 } else { 4 };
            !(tally.lines == 1 && tally.attempts == each && tally.marks == 1
                || tally.lines == 0 && tally.attempts == 0 && tally.marks == 0)
        })
        .filter_map(|(_, tally)| tally.first)
        .min()
}
