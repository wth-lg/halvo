use std::path::Path;

use halvo_derived::DbKey;

use crate::case::{read_dir, Case, Form, Kind};
use crate::cli::{Cause, Flags, Refusal, Report, StoreName};
use crate::derived::{process_key, DerivedHost, Scratch};
use crate::link;
use crate::stub::{play_all, play_case, Arm, Play, Tally};
use crate::targets::{self, Targets};

/// Where a command plays each case in each arm: on a memory store of its own, or on a derived store of its own in a
/// directory under the command's temporary directory, opened with the command's key (contracts.md L10.1).
pub enum Plays {
    Memory,
    Derived { scratch: Scratch, key: DbKey },
}

impl Plays {
    /// The plays on the store that `--store` names; the refusal `store` when the command's temporary directory cannot
    /// be made.
    pub fn on(store: StoreName) -> Result<Plays, Refusal> {
        match store {
            StoreName::Memory => Ok(Plays::Memory),
            StoreName::Derived => Ok(Plays::Derived {
                scratch: Scratch::make().map_err(|detail| Refusal::new(Cause::Store, detail))?,
                key: process_key(),
            }),
        }
    }

    pub fn store(&self) -> StoreName {
        match self {
            Plays::Memory => StoreName::Memory,
            Plays::Derived { .. } => StoreName::Derived,
        }
    }

    /// Plays every occurrence of the case in the arm on a store of its own (L10.2, L10.4).
    pub fn play(&self, case: &Case, arm: Arm, restart_all: bool) -> Result<Tally, String> {
        match self {
            Plays::Memory => play_case(case, arm, restart_all),
            Plays::Derived { scratch, key } => {
                let host =
                    DerivedHost::new(scratch.play_dir()?, key.clone(), case.occurrences[0].day);
                let mut play = Play::with_host(case, arm, 1, host);
                play_all(&mut play, restart_all)
            }
        }
    }
}

/// The set of cases that `--gate` names, with the keys of the targets file that L8.2 reads for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Dev,
    Heldout,
}

impl Gate {
    fn key(self, key: &str) -> String {
        match self {
            Gate::Dev => format!("dev_{key}"),
            Gate::Heldout => format!("heldout_{key}"),
        }
    }
}

/// One case line of `drift` (L10.4).
pub fn case_line(case: &Case, arm: Arm, store: StoreName, tally: &Tally) -> String {
    let c = &tally.counts;
    format!(
        "case name={} kind={} form={} arm={} store={} occurrences={} drifts={} assisted={} failed={} harm={} reverted_good={} edit_rows={} expired_runs={}",
        case.name,
        case.kind.as_str(),
        case.form.as_str(),
        arm.as_str(),
        store.as_str(),
        c.occurrences,
        c.drifts,
        c.assisted,
        c.failed,
        c.harm,
        tally.reverted_good,
        tally.edit_rows,
        tally.expired_runs
    )
}

/// One play of the command, the one numbered `n` + 1, which `n` then counts; on the derived store the play's own
/// store is read back before the command's scratch goes (`read_back`).
pub fn played(
    plays: &Plays,
    case: &Case,
    arm: Arm,
    restart_all: bool,
    n: &mut u32,
) -> Result<Tally, Refusal> {
    let tally = plays.play(case, arm, restart_all).map_err(|detail| {
        Refusal::new(
            Cause::Case,
            format!(
                "{}.jsonl: an occurrence did not complete: {detail}",
                case.name
            ),
        )
    })?;
    *n += 1;
    if let Plays::Derived { scratch, key } = plays {
        let last = case.occurrences.last().map_or(0, |o| o.day);
        read_back(
            &scratch.path().join(format!("play-{n}")),
            key,
            last,
            tally.edit_rows,
        )?;
    }
    Ok(tally)
}

/// The read-back of one play on the derived store (contracts.md L10.1): the store under `dir`, opened again with
/// `key` on `day`, holds `edit_rows` rows of the kind `learn_edit`. The refusal `store`, whose line holds no value,
/// when no store file stands under `dir`, when the store cannot be read, or when it holds another number of rows.
pub fn read_back(dir: &Path, key: &DbKey, day: u32, edit_rows: u32) -> Result<(), Refusal> {
    let held = if dir.join("derived").join("derived.db").is_file() {
        let mut host = crate::derived::DerivedHost::new(dir.to_path_buf(), key.clone(), day);
        crate::host::PlayHost::learn_rows(&mut host, "learn_edit")
            .ok()
            .map(|rows| rows.len())
    } else {
        None
    };
    if held.is_some_and(|rows| u32::try_from(rows) == Ok(edit_rows)) {
        Ok(())
    } else {
        Err(Refusal::new(
            Cause::Store,
            "a play's own derived store does not hold the play's learned edits",
        ))
    }
}

/// The six `spread` lines of L7.4, one per form in the order of L9.3.
pub fn spread(cases: &[Case]) -> Vec<String> {
    Form::ALL
        .iter()
        .map(|form| {
            let of: Vec<&Case> = cases.iter().filter(|c| c.form == *form).collect();
            let shows = |s| of.iter().filter(|c| c.shows == s).count();
            format!(
                "spread form={} cases={} shows1={} shows2={}",
                form.as_str(),
                of.len(),
                shows(1),
                shows(2)
            )
        })
        .collect()
}

/// The lines of L10.4 after the case lines of a gate set, by L8.2, and whether the set passes.
pub fn verdict(cases: &[(Case, [Tally; 3])], targets: &Targets, gate: Gate) -> (Vec<String>, bool) {
    let key = |k: &str| targets.get("gate", &gate.key(k));
    let benign: Vec<&[Tally; 3]> = cases
        .iter()
        .filter(|(c, _)| c.kind == Kind::Benign)
        .map(|(_, t)| t)
        .collect();
    let fault: Vec<&[Tally; 3]> = cases
        .iter()
        .filter(|(c, _)| c.kind == Kind::Fault)
        .map(|(_, t)| t)
        .collect();
    let sum = |set: &[&[Tally; 3]], arm: usize, f: fn(&Tally) -> u32| -> u64 {
        set.iter().map(|t| u64::from(f(&t[arm]))).sum()
    };
    let assisted = |t: &Tally| t.counts.assisted;
    let (none, plain, admit) = (0, 1, 2);
    let worse = benign
        .iter()
        .filter(|t| {
            let a = &t[admit].counts;
            !(a.assisted < t[none].counts.assisted && a.assisted >= a.drifts && a.failed == 0)
        })
        .count();
    let wins = fault
        .iter()
        .filter(|t| t[admit].counts.harm < t[plain].counts.harm)
        .count();
    let losses = fault
        .iter()
        .filter(|t| t[admit].counts.harm > t[plain].counts.harm)
        .count();
    let cost =
        i128::from(sum(&benign, admit, assisted)) - i128::from(sum(&benign, plain, assisted));
    let critical = key("critical");
    let budget = key("cost_budget");
    let failed = cases
        .iter()
        .flat_map(|(_, t)| t.iter())
        .any(|t| t.counts.failed > 0);
    let ok = worse == 0
        && u64::try_from(wins).is_ok_and(|w| w >= critical)
        && losses == 0
        && cost <= i128::from(budget)
        && !failed;
    let lines = vec![
        format!(
            "improve cases={} admit={} none={} bound={} worse={}",
            benign.len(),
            sum(&benign, admit, assisted),
            sum(&benign, none, assisted),
            sum(&benign, admit, |t| t.counts.drifts),
            worse
        ),
        format!(
            "rollback cases={} wins={} losses={} critical={} cost={} budget={}",
            fault.len(),
            wins,
            losses,
            critical,
            cost,
            budget
        ),
        format!("drift result={}", if ok { "ok" } else { "miss" }),
    ];
    (lines, ok)
}

/// `learn-gate drift --cases DIR --arm none|plain|admit [--store memory|derived] [--restart-all]` and
/// `learn-gate drift --gate dev|heldout --cases DIR [--store memory|derived] [--restart-all]` (L10.4).
pub fn run(args: &[String]) -> Result<Report, Refusal> {
    let mut flags = Flags::new(args);
    if flags.value("--link")?.is_some() {
        return link::run(args);
    }
    let cases_dir = flags
        .value("--cases")?
        .ok_or_else(|| Refusal::usage("drift takes --cases DIR"))?;
    let arm = flags.value("--arm")?;
    let gate = flags.value("--gate")?;
    let store = flags.value("--store")?.map(StoreName::parse).transpose()?;
    let restart_all = flags.switch("--restart-all")?;
    flags.done()?;
    let mode = match (arm, gate) {
        (Some(arm), None) => Ok(
            Arm::parse(arm).ok_or_else(|| Refusal::usage("--arm takes none, plain or admit"))?
        ),
        (None, Some("dev")) => Err(Gate::Dev),
        (None, Some("heldout")) => Err(Gate::Heldout),
        (None, Some(_)) => return Err(Refusal::usage("--gate takes dev or heldout")),
        _ => return Err(Refusal::usage("drift takes one of --arm and --gate")),
    };
    let targets = targets::read()?;
    let cases = read_dir(
        Path::new(cases_dir),
        targets.get("gate", "calendar_days_min"),
    )
    .map_err(|detail| Refusal::new(Cause::Case, detail))?;
    let plays = Plays::on(store.unwrap_or(StoreName::Memory))?;
    let store = plays.store();
    let mut n = 0;
    match mode {
        Ok(arm) => {
            let mut lines = Vec::new();
            for case in &cases {
                let tally = played(&plays, case, arm, restart_all, &mut n)?;
                lines.push(case_line(case, arm, store, &tally));
            }
            Ok(Report { lines, ok: true })
        }
        Err(gate) => {
            let benign = cases.iter().filter(|c| c.kind == Kind::Benign).count() as u64;
            let fault = cases.len() as u64 - benign;
            let want = (
                targets.get("gate", &gate.key("benign")),
                targets.get("gate", &gate.key("fault")),
            );
            if (benign, fault) != want {
                return Err(Refusal::new(
                    Cause::Case,
                    format!(
                        "the set holds {benign} benign and {fault} fault cases, not {} and {}",
                        want.0, want.1
                    ),
                ));
            }
            let mut lines = match gate {
                Gate::Heldout => spread(&cases),
                Gate::Dev => Vec::new(),
            };
            let mut played_cases = Vec::new();
            for case in cases {
                let tallies = [
                    played(&plays, &case, Arm::None, restart_all, &mut n)?,
                    played(&plays, &case, Arm::Plain, restart_all, &mut n)?,
                    played(&plays, &case, Arm::Admit, restart_all, &mut n)?,
                ];
                for (arm, tally) in Arm::ALL.iter().zip(&tallies) {
                    lines.push(case_line(&case, *arm, store, tally));
                }
                played_cases.push((case, tallies));
            }
            let (summary, ok) = verdict(&played_cases, &targets, gate);
            lines.extend(summary);
            Ok(Report { lines, ok })
        }
    }
}
