use std::collections::HashMap;
use std::num::NonZeroU32;

use halvo_route::tier::CallKind;

use super::lines::{AttemptKey, Verdict};
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
/// model, machine, task and seed. `replays` holds the replay lines in file order.
pub fn gap(replays: &[(usize, AttemptKey)], attempts: &Attempts) -> Option<usize> {
    replays.iter().find_map(|(number, key)| {
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
