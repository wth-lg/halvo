use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};

use halvo_learn_store::{Clock, LearnStep};
use halvo_schema::learning::names::Mark;
use halvo_schema::learning::record::LearnRoutine;
use halvo_schema::{RowRead, Summary, SummaryRead, WriterStep};
use serde_json::{json, Value};

use crate::case::{Case, Form, Kind, Occurrence, Step, Verb};
use crate::cli::{Cause, Flags, Refusal, Report, StoreName};
use crate::page::{Control, Role};
use crate::store::{stays, Family, MemoryStore};
use crate::stub::{id, workflow_record, Arm, Play};

/// The sample of the routine trial under the repository root, where the commands are run (contracts.md L7.2).
pub const SAMPLE_PATH: &str = "fixtures/r5/trains/sample.jsonl";

/// The number of trains that the sample holds (L7.2).
pub const TRAINS: usize = 135;

/// The last day of a train; the play runs the clock from day 1 to it (L7.2, L10.4).
pub const LAST_DAY: u32 = 730;

/// The first day whose occurrence the second play carries as a verified replay run (L10.4).
pub const REPLAY_FROM: u32 = 120;

/// The mark and the period of a routine on one day.
pub type Figures = (Mark, Option<u32>);

/// One day of a train's `events`: the day on which the mark or the period of the trial's rule changed, and both
/// as they stand from that day on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub day: u32,
    pub mark: Mark,
    pub period: Option<u32>,
}

/// One train of the sample: its occurrence days, ascending and each once, and its events, ascending by day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Train {
    pub days: Vec<u32>,
    pub events: Vec<Event>,
}

impl Train {
    /// The mark and the period that the trial's rule gives after day `day`: those of the last event at or before
    /// it, and the mark `none` with no period before the first event.
    pub fn figures_on(&self, day: u32) -> Figures {
        self.events
            .iter()
            .rev()
            .find(|e| e.day <= day)
            .map_or((Mark::None, None), |e| (e.mark, e.period))
    }
}

/// The stream that carries a train's occurrences in one play (L10.4): every occurrence as the user's summary of its
/// day, or those from day `REPLAY_FROM` on as replay runs of the stub runner and the rest as summaries. The command
/// requires that each run completed; that it is verified is the step's check, through the handlers' verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Summaries,
    Replay,
}

impl Stream {
    pub const BOTH: [Stream; 2] = [Stream::Summaries, Stream::Replay];
}

/// What one play of a train did: the first day whose newest routine row differs from the trial's figures, or none
/// when every day from 1 to `LAST_DAY` gives them, the occurrences that it carried by each stream, and the
/// summaries and `learn_run` rows that the opens removed by their bound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Played {
    pub differs_on: Option<u32>,
    pub summaries: u32,
    pub runs: u32,
    pub expired_summaries: u32,
    pub expired_runs: u32,
}

fn day_of(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|d| u32::try_from(d).ok())
        .filter(|d| (1..=LAST_DAY).contains(d))
}

fn keys_are(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
}

fn read_event(value: &Value) -> Option<Event> {
    if !keys_are(value, &["day", "mark", "period"]) {
        return None;
    }
    let mark = match value["mark"].as_str()? {
        "none" => Mark::None,
        "stale" => Mark::Stale,
        _ => return None,
    };
    let period = match &value["period"] {
        Value::Null => None,
        p => Some(
            p.as_u64()
                .and_then(|p| u32::try_from(p).ok())
                .filter(|p| *p > 0)?,
        ),
    };
    Some(Event {
        day: day_of(&value["day"])?,
        mark,
        period,
    })
}

fn ascending(days: impl Iterator<Item = u32>) -> bool {
    let days: Vec<u32> = days.collect();
    days.windows(2).all(|w| w[0] < w[1])
}

/// The grid of L7.2: a cell is `<cadence>-<jitter>-<miss_pct>` with each part one of these.
const CADENCES: [&str; 3] = ["7", "14", "30"];
const JITTERS: [&str; 3] = ["0", "1", "3"];
const MISS_PCTS: [&str; 3] = ["0", "10", "20"];

/// The highest train number of a cell (L7.2: each cell has 200 trains).
const TRAIN_MAX: u64 = 200;

fn in_grid(cell: &str) -> bool {
    let parts: Vec<&str> = cell.split('-').collect();
    matches!(parts[..], [cadence, jitter, miss]
        if CADENCES.contains(&cadence) && JITTERS.contains(&jitter) && MISS_PCTS.contains(&miss))
}

/// The line of L7.2 that holds these values, byte for byte.
fn written(cell: &str, train: u64, kind: &str, days: &[u32], events: &[Event]) -> String {
    let days: Vec<String> = days.iter().map(u32::to_string).collect();
    let events: Vec<String> = events
        .iter()
        .map(|e| {
            let mark = match e.mark {
                Mark::None => "none",
                Mark::Stale => "stale",
            };
            let period = e.period.map_or("null".to_owned(), |p| p.to_string());
            format!(r#"{{"day":{},"mark":"{mark}","period":{period}}}"#, e.day)
        })
        .collect();
    format!(
        r#"{{"cell":"{cell}","train":{train},"kind":"{kind}","days":[{}],"events":[{}]}}"#,
        days.join(","),
        events.join(",")
    )
}

fn read_train(line: &str) -> Option<Train> {
    let value: Value = serde_json::from_str(line).ok()?;
    if !keys_are(&value, &["cell", "train", "kind", "days", "events"]) {
        return None;
    }
    let cell = value["cell"].as_str().filter(|c| in_grid(c))?;
    let train = value["train"]
        .as_u64()
        .filter(|n| (1..=TRAIN_MAX).contains(n))?;
    let kind = value["kind"]
        .as_str()
        .filter(|k| matches!(*k, "same" | "stop" | "shift"))?;
    let days: Vec<u32> = value["days"]
        .as_array()?
        .iter()
        .map(day_of)
        .collect::<Option<_>>()?;
    let events: Vec<Event> = value["events"]
        .as_array()?
        .iter()
        .map(read_event)
        .collect::<Option<_>>()?;
    if !ascending(days.iter().copied())
        || !ascending(events.iter().map(|e| e.day))
        || written(cell, train, kind, &days, &events) != line
    {
        return None;
    }
    Some(Train { days, events })
}

/// The trains of a sample file: one line per train, each ending in LF alone and each the line of L7.2 byte for byte:
/// the keys in L7.2's order and each once, no white space, a cell of L7.2's grid and a train number from 1 to 200.
pub fn read_sample(bytes: &[u8]) -> Result<Vec<Train>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the sample is not UTF-8".to_owned())?;
    let body = text
        .strip_suffix('\n')
        .ok_or_else(|| "the sample does not end in one LF".to_owned())?;
    body.split('\n')
        .enumerate()
        .map(|(at, line)| {
            read_train(line).ok_or_else(|| format!("line {} of the sample is no train", at + 1))
        })
        .collect()
}

struct Today(u32);

impl Clock for Today {
    fn today(&self) -> u32 {
        self.0
    }
}

/// The summaries of the committed batches, each with its record as stored, which the bound of its kind reads.
struct Batches(Vec<(String, Summary)>);

impl Batches {
    fn open(&mut self, today: u32) -> u32 {
        let before = self.0.len();
        self.0.retain(|(json, _)| stays("summary", json, today));
        u32::try_from(before - self.0.len()).unwrap_or(u32::MAX)
    }
}

impl SummaryRead for Batches {
    fn for_each(&self, visit: &mut dyn FnMut(&Summary)) {
        for (_, summary) in &self.0 {
            visit(summary);
        }
    }
}

fn page() -> Vec<Control> {
    vec![Control {
        id: "go".to_owned(),
        role: Role::Button,
        name: "Pay now".to_owned(),
        test_id: "pay".to_owned(),
        passes: Vec::new(),
    }]
}

fn occurrence(day: u32) -> Occurrence {
    Occurrence {
        day,
        page: page(),
        repair: vec![vec!["go".to_owned()]],
        flaky: Vec::new(),
        restart: false,
    }
}

/// The case whose compile on day 1 makes the workflow, its agent and so its first `learn_routine` row, and whose
/// one step every replay run plays on an unchanged page.
fn routine_case() -> Case {
    Case {
        name: "routine".to_owned(),
        kind: Kind::Benign,
        form: Form::Renamed,
        shows: 0,
        steps: vec![Step {
            n: 1,
            verb: Verb::Click,
            control: "go".to_owned(),
        }],
        occurrences: vec![occurrence(1)],
    }
}

/// The user's summary of the task on `day`: the category, the site and the app of the workflow that the gate host
/// makes, so that the step reads it as an occurrence (L6.1).
fn summary(workflow: &str, day: u32) -> Result<(String, Summary), String> {
    let record = serde_json::to_value(workflow_record(workflow))
        .map_err(|_| "a workflow with no JSON".to_owned())?;
    let json = json!({
        "v": 1, "id": id('6', day), "category": record["category"], "site": record["site"],
        "app": record["app"], "day": day, "part": "evening", "minutes": 5, "keys": null, "pointer": null,
    })
    .to_string();
    let summary = serde_json::from_str(&json).map_err(|e| format!("a summary is refused: {e}"))?;
    Ok((json, summary))
}

/// The mark and the period of the newest `learn_routine` row of the rows, or an error when a row of that kind
/// cannot be read or none is there.
pub fn newest(rows: &dyn RowRead) -> Result<Figures, String> {
    let mut found: Option<LearnRoutine> = None;
    let mut unread = false;
    rows.for_each(
        "learn_routine",
        &mut |json| match LearnRoutine::parse(json.as_bytes()) {
            Ok(row) if found.as_ref().is_none_or(|f| row.rev > f.rev) => found = Some(row),
            Ok(_) => {}
            Err(_) => unread = true,
        },
    );
    if unread {
        return Err("a learn_routine row that its parser refuses".to_owned());
    }
    let row = found.ok_or_else(|| "no learn_routine row".to_owned())?;
    Ok((row.mark, row.period.map(std::num::NonZeroU32::get)))
}

/// Plays one train in one stream on a memory store of its own (L10.2, L10.4): the compile on day 1, then each day
/// from 1 to `LAST_DAY` on the clock, until the first day whose figures differ from the train's. On a day with an
/// occurrence the store and the command's list of summaries are opened on the day, and the stub runner plays the
/// occurrence where the stream carries it as a run; else the day's summary goes onto that list. No batch is
/// committed: every day `LearnStep` runs over that list and the rows of the family `learn`, its rows are committed,
/// and the mark and the period of the newest routine row, read again after a write or on an occurrence day, are
/// compared with the train's. The line reaches neither the step's 30-day rewrite nor the bound of a routine row.
pub fn play(train: &Train, stream: Stream) -> Result<Played, String> {
    let case = routine_case();
    let mut play = Play::new(&case, Arm::Admit, 1, MemoryStore::new());
    play.host.day = 1;
    play.compile()?;
    let workflow = play.workflow().to_owned();
    let occurs: BTreeSet<u32> = train.days.iter().copied().collect();
    let mut batches = Batches(Vec::new());
    let mut played = Played::default();
    let mut figures = newest(play.host.store.rows(Family::Learn))?;
    for day in 1..=LAST_DAY {
        play.host.day = day;
        let occurs_today = occurs.contains(&day);
        if occurs_today {
            let expired = play.host.store.open(day).of("learn_run");
            played.expired_runs += u32::try_from(expired).unwrap_or(u32::MAX);
            played.expired_summaries += batches.open(day);
            if stream == Stream::Replay && day >= REPLAY_FROM {
                let (_, _, completed, _) = play.occurrence(&occurrence(day))?;
                if !completed {
                    return Err(format!("the replay run of day {day} did not complete"));
                }
                played.runs += 1;
            } else {
                batches.0.push(summary(&workflow, day)?);
                played.summaries += 1;
            }
        }
        let step = LearnStep::new(Today(day));
        let records = step.after_commit(&batches, play.host.store.rows(Family::Learn));
        let wrote = !records.is_empty();
        play.host
            .store
            .commit(Family::Learn, records)
            .map_err(|e| format!("the step's rows of day {day} are refused: {e:?}"))?;
        if wrote || occurs_today {
            figures = newest(play.host.store.rows(Family::Learn))?;
        }
        if figures != train.figures_on(day) {
            played.differs_on = Some(day);
            return Ok(played);
        }
    }
    Ok(played)
}

/// The threads that play the trains: the machine's parallelism, at most four.
pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(4))
}

/// Every play of the trains, each train in both streams in the order of `Stream::BOTH`, in the order of the trains,
/// spread over `threads` threads, each play made by `play_one`. A play that panics gives the error "the play
/// panicked" at its own place, and the thread goes on with the next play, so every place holds its own result; what
/// the panic hook writes to standard error stays beside the command's one line.
pub fn play_all<F>(trains: &[Train], threads: usize, play_one: F) -> Vec<Result<Played, String>>
where
    F: Fn(&Train, Stream) -> Result<Played, String> + Sync,
{
    let plays: Vec<(&Train, Stream)> = trains
        .iter()
        .flat_map(|t| Stream::BOTH.map(|s| (t, s)))
        .collect();
    let next = AtomicUsize::new(0);
    let mut out: Vec<Option<Result<Played, String>>> = vec![None; plays.len()];
    let done: Vec<Vec<(usize, Result<Played, String>)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads.max(1))
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some((train, stream)) = plays.get(at) else {
                            return mine;
                        };
                        let one = || play_one(train, *stream);
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(one)) {
                            Ok(result) => mine.push((at, result)),
                            Err(_) => mine.push((at, Err("the play panicked".to_owned()))),
                        }
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap_or_default())
            .collect()
    });
    for (at, result) in done.into_iter().flatten() {
        out[at] = Some(result);
    }
    out.into_iter()
        .map(|r| r.unwrap_or_else(|| Err("a play that no thread ran".to_owned())))
        .collect()
}

/// Plays every train in both streams and gives the line of L10.4: `same` counts the plays in which every day
/// gives the trial's figures, and the result is ok when it is twice the number of trains, which must be `TRAINS`.
pub fn report(trains: &[Train]) -> Result<Report, Refusal> {
    report_with(trains, threads(), play)
}

/// `report` with the thread count and the function that plays one train in one stream given: the command passes
/// `threads()` and `play`.
pub fn report_with<F>(trains: &[Train], threads: usize, play_one: F) -> Result<Report, Refusal>
where
    F: Fn(&Train, Stream) -> Result<Played, String> + Sync,
{
    if trains.len() != TRAINS {
        return Err(Refusal::new(
            Cause::Case,
            format!("the sample holds {} trains, not {TRAINS}", trains.len()),
        ));
    }
    let played = play_all(trains, threads, play_one)
        .into_iter()
        .enumerate()
        .map(|(at, result)| {
            result.map_err(|detail| {
                Refusal::new(
                    Cause::Store,
                    format!("train {}: {detail}", at / Stream::BOTH.len() + 1),
                )
            })
        })
        .collect::<Result<Vec<Played>, Refusal>>()?;
    Ok(line(trains.len(), &played))
}

/// The line of L10.4 for the plays of `trains` trains.
pub fn line(trains: usize, played: &[Played]) -> Report {
    let same = played.iter().filter(|p| p.differs_on.is_none()).count();
    let plays = trains * Stream::BOTH.len();
    let ok = played.len() == plays && same == plays;
    Report {
        lines: vec![format!(
            "routine trains={trains} plays={plays} same={same} result={}",
            if ok { "ok" } else { "miss" }
        )],
        ok,
    }
}

/// `learn-gate routine [--store memory|derived]` (L10.4).
pub fn run(args: &[String]) -> Result<Report, Refusal> {
    let mut flags = Flags::new(args);
    let store = flags.value("--store")?.map(StoreName::parse).transpose()?;
    flags.done()?;
    store.unwrap_or(StoreName::Memory).memory_only()?;
    let bytes = std::fs::read(SAMPLE_PATH)
        .map_err(|_| Refusal::new(Cause::Case, format!("{SAMPLE_PATH} cannot be read")))?;
    let trains = read_sample(&bytes).map_err(|detail| Refusal::new(Cause::Case, detail))?;
    report(&trains)
}
