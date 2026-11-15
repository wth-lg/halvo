use std::time::{Duration, Instant};

/// Where a press of a hosted case is timed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum From {
    /// The show's report on screen.
    Report,
    /// The time at which the press before it ran on the dialog's thread.
    Previous,
}

/// When a press timed `at` milliseconds from `from` is due: from the report, or from the time
/// `previous` at which the press before it ran on the dialog's thread, the report when there was
/// none.
pub fn due(report: Instant, previous: Option<Instant>, from: From, at: u64) -> Instant {
    let base = match from {
        From::Report => report,
        From::Previous => previous.unwrap_or(report),
    };
    base + Duration::from_millis(at)
}

/// The failure of a press timed from the press before it that must fall inside that press's
/// hold: the line "the schedule slipped" when the press ran `arm` or more after the press before
/// it, by their own times on the dialog's thread.
pub fn slipped(previous: Instant, ran: Instant, arm: Duration) -> Option<String> {
    let gap = ran.saturating_duration_since(previous);
    (gap >= arm).then(|| {
        format!(
            "the schedule slipped: the press ran {} ms after the press before it, and arm_ms is {}",
            gap.as_millis(),
            arm.as_millis()
        )
    })
}

/// How a target runs the case whose press is timed from the press before it (ruling RD-107,
/// Addition 3): once, as in 12-test and check 46, where a slipped schedule runs the case again, at
/// most [`ATTEMPTS`] times in all; or as one run of a loop, where a slipped schedule is counted
/// apart, neither a pass nor a miss.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Once,
    Loop,
}

/// The runs of a case that a target makes at most, once, when its schedule slips.
pub const ATTEMPTS: usize = 3;

/// The name by which a loop with too few runs that kept the schedule fails, and a case whose
/// schedule slipped in each of its [`ATTEMPTS`] runs.
pub const TOO_FEW: &str = "too few runs kept the schedule";

/// What a target does after a run of a case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// The schedule held: the case's own failures decide.
    Done,
    /// The schedule slipped, and the case runs once more.
    Again,
    /// The schedule slipped in a loop's run: counted apart, the hold's answer not read.
    Slipped,
    /// The schedule slipped in each of the runs: the case fails by [`TOO_FEW`].
    TooFew,
}

/// What follows run number `attempt` (1 for the first) of a case in `mode`, by whether its
/// schedule slipped.
pub fn next(mode: Mode, attempt: usize, slipped: bool) -> Next {
    match (slipped, mode) {
        (false, _) => Next::Done,
        (true, Mode::Loop) => Next::Slipped,
        (true, Mode::Once) if attempt < ATTEMPTS => Next::Again,
        (true, Mode::Once) => Next::TooFew,
    }
}

/// One run of a loop, from the end line of the target's run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    /// The schedule held and the run passed.
    Kept,
    /// The schedule held and the run failed.
    KeptFailed,
    /// The schedule slipped, and no other case failed.
    Slipped,
    /// The schedule slipped and another case failed, or the run printed no end line.
    Missed,
}

/// The number after `key` and a space in `line`.
fn count(line: &str, key: &str) -> Option<u64> {
    let rest = &line[line.find(key)? + key.len()..];
    let digits: String = rest
        .strip_prefix(' ')?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The number right after `key` in `line`.
fn digits_after(line: &str, key: &str) -> Option<u64> {
    let rest = &line[line.find(key)? + key.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// One run of a loop from its target's lines: the kind of run by the end line
/// (`guard_hosted end: cases ran A of B, failed F, schedule slipped S, helped H, moments at a
/// press M, retries R`), and the gap of each case line that says its schedule slipped.
pub fn run_of(text: &str) -> (Run, Vec<u64>) {
    let gaps = text
        .lines()
        .filter(|line| line.starts_with("guard_hosted case ") && line.contains(" SLIPPED: "))
        .filter_map(|line| count(line, "the press ran"))
        .collect();
    let Some(end) = text
        .lines()
        .find(|line| line.starts_with("guard_hosted end: "))
    else {
        return (Run::Missed, gaps);
    };
    let ran = count(end, "cases ran");
    let listed = count(end, " of");
    let fields = (
        count(end, "failed"),
        count(end, "schedule slipped"),
        count(end, "helped"),
        count(end, "moments at a press"),
    );
    let (Some(failed), Some(slipped), Some(helped), Some(moments)) = fields else {
        return (Run::Missed, gaps);
    };
    let passed = ran.is_some()
        && ran == listed
        && failed == 0
        && helped == 0
        && moments == 0
        && !end.contains(", FAIL");
    let kind = match (slipped, passed) {
        (0, true) => Run::Kept,
        (0, false) => Run::KeptFailed,
        (_, true) => Run::Slipped,
        (_, false) => Run::Missed,
    };
    (kind, gaps)
}

/// The schedule case's first two presses in one run, from the first schedule line that it
/// printed (`guard_hosted schedule <case>: the press before ran at +A ms, this one at +B ms, gap G
/// ms`): A, B and G.
pub fn times_of(text: &str) -> Option<(u64, u64, u64)> {
    let line = text
        .lines()
        .find(|line| line.starts_with("guard_hosted schedule "))?;
    let first = digits_after(line, "the press before ran at +")?;
    let second = digits_after(line, "this one at +")?;
    let gap = count(line, "gap")?;
    Some((first, second, gap))
}

/// The retries of an answering press that one run's end line counts, 0 with no end line.
pub fn retries_of(text: &str) -> u64 {
    text.lines()
        .find(|line| line.starts_with("guard_hosted end: "))
        .and_then(|end| count(end, "retries"))
        .unwrap_or(0)
}

/// The runs of a loop, numbered from 1, by their kind, with each run's press times and the runs
/// that retried their answering press.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub runs: usize,
    pub kept: usize,
    pub kept_failed: Vec<usize>,
    pub slipped: Vec<(usize, Vec<u64>)>,
    pub missed: Vec<usize>,
    pub retried: Vec<usize>,
    pub lines: Vec<String>,
}

/// The tally of a loop's runs, each the text of one run of the target.
pub fn tally(texts: &[String]) -> Tally {
    let mut tally = Tally {
        runs: texts.len(),
        ..Tally::default()
    };
    for (n, text) in texts.iter().enumerate() {
        let (kind, gaps) = run_of(text);
        if retries_of(text) > 0 {
            tally.retried.push(n + 1);
        }
        let word = match kind {
            Run::Kept => "kept the schedule and passed",
            Run::KeptFailed => "kept the schedule and failed",
            Run::Slipped => "schedule slipped",
            Run::Missed => "missed",
        };
        tally.lines.push(match times_of(text) {
            Some((first, second, gap)) => format!(
                "guard_hosted loop run {}: the first press at +{first} ms, the second at +{second} ms, gap {gap} ms; {word}",
                n + 1
            ),
            None => format!("guard_hosted loop run {}: no schedule line; {word}", n + 1),
        });
        match kind {
            Run::Kept => tally.kept += 1,
            Run::KeptFailed => tally.kept_failed.push(n + 1),
            Run::Slipped => tally.slipped.push((n + 1, gaps)),
            Run::Missed => tally.missed.push(n + 1),
        }
    }
    tally
}

/// The floor of a loop: the runs that it makes and the runs that must keep the schedule, 45 of 60
/// on macOS and 18 of 20 on Windows.
pub const FLOOR_MACOS: (usize, usize) = (60, 45);
pub const FLOOR_WINDOWS: (usize, usize) = (20, 18);

fn numbers(runs: &[usize]) -> String {
    runs.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The loop's failures, none when it passed: it ran `runs` runs; at least `floor` of them kept
/// the schedule; every run that kept it passed; and no run missed.
pub fn verdict(tally: &Tally, (runs, floor): (usize, usize)) -> Vec<String> {
    let mut failures = Vec::new();
    if tally.runs != runs {
        failures.push(format!("the loop ran {} runs, not {runs}", tally.runs));
    }
    let kept = tally.kept + tally.kept_failed.len();
    if kept < floor {
        failures.push(format!(
            "{TOO_FEW}: {kept} of {} kept it, and the floor is {floor}",
            tally.runs
        ));
    }
    if !tally.kept_failed.is_empty() {
        failures.push(format!(
            "a run that kept the schedule failed: run {}",
            numbers(&tally.kept_failed)
        ));
    }
    if !tally.missed.is_empty() {
        failures.push(format!(
            "a run whose schedule slipped failed in another case, or a run printed no end line: run {}",
            numbers(&tally.missed)
        ));
    }
    failures
}

/// The loop's line: the runs, how many kept the schedule, and how many slipped with their gaps.
pub fn line(tally: &Tally, (runs, floor): (usize, usize)) -> String {
    let gaps: Vec<String> = tally
        .slipped
        .iter()
        .map(|(n, gaps)| {
            let gaps: Vec<String> = gaps.iter().map(|g| format!("{g} ms")).collect();
            format!("run {n} gap {}", gaps.join(" and "))
        })
        .collect();
    format!(
        "guard_hosted loop: runs {} of {runs}; kept the schedule {} (the floor is {floor}): passed {}, failed {}; schedule slipped {}{}; missed {}; retried an answering press {}{}",
        tally.runs,
        tally.kept + tally.kept_failed.len(),
        tally.kept,
        tally.kept_failed.len(),
        tally.slipped.len(),
        if gaps.is_empty() {
            String::new()
        } else {
            format!(" ({})", gaps.join("; "))
        },
        tally.missed.len(),
        tally.retried.len(),
        if tally.retried.is_empty() {
            String::new()
        } else {
            format!(" (run {})", numbers(&tally.retried))
        }
    )
}
