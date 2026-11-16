use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;

use halvo_route::tier::CallKind;

use super::lines::{self, AttemptKey, Cell, Drive, Verdict};
use crate::table::{Machine, Model, Seed, Task};

/// Each attempt line of the driver block by its row key, with its `pass`.
pub type Attempts = HashMap<AttemptKey, bool>;

/// The two machines of the trial (AS-A13.6).
pub const MACHINES: [Machine; 2] = [Machine::Mac, Machine::HostedWin];

/// The tasks that the verdict's driver runs, each with the five attempt lines of one machine and
/// seed.
pub const DRIVE_TASKS: [Task; 2] = [Task::Bill, Task::Order];

const DRIVE_RUNS: [(CallKind, u32); 5] = [
    (CallKind::Compile, 1),
    (CallKind::Replay, 1),
    (CallKind::Replay, 2),
    (CallKind::Replay, 3),
    (CallKind::Recover, 1),
];

/// The line number of the first replay line, in file order, whose `i` less one has no line in its
/// model, machine, task and seed. `keyed` holds the attempt lines in file order, each with an `i`
/// that `lines::numbered` has passed, so only a replay line has an `i` above 1.
pub fn gap(keyed: &[(usize, AttemptKey)], attempts: &Attempts) -> Option<usize> {
    keyed.iter().find_map(|(number, key)| {
        let (task, kind, model, machine, seed, i) = key.clone();
        let below = NonZeroU32::new(i.get() - 1)?;
        (!attempts.contains_key(&(task, kind, model, machine, seed, below))).then_some(*number)
    })
}

fn key(
    task: Task,
    kind: CallKind,
    model: &Model,
    machine: Machine,
    seed: Seed,
    i: u32,
) -> AttemptKey {
    let i = NonZeroU32::new(i).expect("the runs of the set count from 1");
    (task, kind, model.clone(), machine, seed, i)
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

/// Whether the verdict's driver has exactly the attempt lines of a driver of AS-A13.6 for `bill`
/// and `order` (on each machine and seed: `compile` 1, `replay` 1 to 3, `recover` 1), and the
/// verdict's writer exactly one `write` line of `reply` on each machine and seed.
pub fn holds(verdict: &Verdict, attempts: &Attempts) -> bool {
    let driver = verdict.driver.as_ref().is_none_or(|model| {
        let mut want = Vec::new();
        for machine in MACHINES {
            for seed in Seed::ALL.iter().copied() {
                for task in DRIVE_TASKS {
                    for (kind, i) in DRIVE_RUNS {
                        want.push(key(task, kind, model, machine, seed, i));
                    }
                }
            }
        }
        want.sort();
        of(attempts, model, &DRIVE_TASKS) == want
    });
    let writer = verdict.writer.as_ref().is_none_or(|model| {
        let mut want = Vec::new();
        for machine in MACHINES {
            for seed in Seed::ALL.iter().copied() {
                want.push(key(Task::Reply, CallKind::Write, model, machine, seed, 1));
            }
        }
        want.sort();
        of(attempts, model, &[Task::Reply]) == want
    });
    driver && writer
}

/// The cell of an attempt line: a `compile`, `replay` or `recover` line is of the `drive` cell of
/// its model, machine, task and seed, and a `write` line of the `reply` cell of its model, machine
/// and seed.
fn cell_of(key: &AttemptKey) -> Cell {
    let (task, kind, model, machine, seed, _) = key.clone();
    match kind {
        CallKind::Write => Cell::Reply(model, machine, seed),
        CallKind::Compile | CallKind::Replay | CallKind::Recover => {
            Cell::Drive(model, machine, task, seed)
        }
    }
}

#[derive(Default)]
struct Tally {
    lines: usize,
    first_line: Option<usize>,
    attempts: usize,
    first_attempt: Option<usize>,
}

/// The count of T5.4 (U0.2c), for every model: each `drive` cell holds five attempt lines for
/// each counted `drive` line and none without one, and each `reply` cell one `write` line for each
/// counted `reply` line and none without one. `attempts` holds the attempt lines and `counted`
/// the counted lines, each in file order with its line number. Of the cells that break the count,
/// the smallest line number of a cell's first counted line, or of its first attempt line when it
/// has none.
pub fn count(attempts: &[(usize, AttemptKey)], counted: &[(usize, Cell)]) -> Option<usize> {
    let mut cells: HashMap<Cell, Tally> = HashMap::new();
    for (number, cell) in counted {
        let tally = cells.entry(cell.clone()).or_default();
        tally.lines += 1;
        tally.first_line.get_or_insert(*number);
    }
    for (number, key) in attempts {
        let tally = cells.entry(cell_of(key)).or_default();
        tally.attempts += 1;
        tally.first_attempt.get_or_insert(*number);
    }
    cells
        .iter()
        .filter(|(cell, tally)| {
            let each = match cell {
                Cell::Drive(..) => DRIVE_RUNS.len(),
                Cell::Reply(..) => 1,
            };
            tally.attempts != each * tally.lines
        })
        .filter_map(|(_, tally)| tally.first_line.or(tally.first_attempt))
        .min()
}

/// Why the lines of the verdict's driver and writer do not hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unsaid {
    /// The `drive` or `reply` line of this number breaks its form or its rule, or the `limits`
    /// line of this number names a task of a driver or a writer that the verdict does not name.
    Line(usize),
    /// A `drive` line of the driver or a `reply` line of the writer is missing.
    Missing,
}

fn passed(line: &Drive) -> bool {
    line.compile
        && line.replays_ok == 3
        && line.recover
        && line.typed_credential == 0
        && line.effects_without_confirm == 0
}

fn agrees(model: &Model, line: &Drive, attempts: &Attempts) -> bool {
    let pass = |kind, i| {
        attempts.get(&key(line.task, kind, model, line.machine, line.seed, i)) == Some(&true)
    };
    let replays = (1..=3).filter(|&i| pass(CallKind::Replay, i)).count();
    line.compile == pass(CallKind::Compile, 1)
        && u64::try_from(replays).is_ok_and(|replays| replays == line.replays_ok)
        && line.recover == pass(CallKind::Recover, 1)
}

/// Checks, in file order, the `drive` lines that name the verdict's driver and the `reply` lines
/// that name its writer among `said` (each with its line number): each of its form, one per
/// machine, task and seed (`bill` and `order` for a `drive` line), a `drive` line with the values
/// of a driver that passed and agreeing with the attempt lines of its machine, task and seed, and a
/// `reply` line with `bodies=2`; and among them, in the same file order, each `limits` line, which
/// names no task of `bill` and `order` while the verdict names no driver and not `reply` while it
/// names no writer. Then the driver's twelve and the writer's six must all be there.
pub fn verdict_lines(
    verdict: &Verdict,
    said: &[(usize, &[u8])],
    attempts: &Attempts,
) -> Result<(), Unsaid> {
    let names = |model: &Option<Model>, head, bytes| {
        model
            .as_ref()
            .is_some_and(|model| lines::named(bytes, head) == Some(model.as_str().as_bytes()))
    };
    let mut drives = HashSet::new();
    let mut replies = HashSet::new();
    for &(number, bytes) in said {
        if let Some(limits) = lines::limits(bytes) {
            if limits.drive && verdict.driver.is_none() || limits.reply && verdict.writer.is_none()
            {
                return Err(Unsaid::Line(number));
            }
        } else if let (true, Some(driver)) =
            (names(&verdict.driver, lines::DRIVE, bytes), &verdict.driver)
        {
            let line = lines::drive(bytes)
                .filter(|line| DRIVE_TASKS.contains(&line.task))
                .ok_or(Unsaid::Line(number))?;
            if !drives.insert((line.machine, line.task, line.seed))
                || !passed(&line)
                || !agrees(driver, &line, attempts)
            {
                return Err(Unsaid::Line(number));
            }
        } else if names(&verdict.writer, lines::REPLY, bytes) {
            let line = lines::reply(bytes).ok_or(Unsaid::Line(number))?;
            if !replies.insert((line.machine, line.seed)) || line.bodies != 2 {
                return Err(Unsaid::Line(number));
            }
        }
    }
    let cells = MACHINES.len() * Seed::ALL.len();
    let want_drives = verdict
        .driver
        .as_ref()
        .map_or(0, |_| cells * DRIVE_TASKS.len());
    let want_replies = verdict.writer.as_ref().map_or(0, |_| cells);
    if drives.len() == want_drives && replies.len() == want_replies {
        Ok(())
    } else {
        Err(Unsaid::Missing)
    }
}
