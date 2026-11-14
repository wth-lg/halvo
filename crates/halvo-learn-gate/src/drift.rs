use std::path::Path;

use crate::case::{read_dir, Case, Form, Kind};
use crate::cli::{Cause, Flags, Refusal, Report, StoreName};
use crate::link;
use crate::stub::{play_case, Arm, Tally};
use crate::targets::{self, Targets};

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
pub fn case_line(case: &Case, arm: Arm, tally: &Tally) -> String {
    let c = &tally.counts;
    format!(
        "case name={} kind={} form={} arm={} store=memory occurrences={} drifts={} assisted={} failed={} harm={} reverted_good={} edit_rows={} expired_runs={}",
        case.name,
        case.kind.as_str(),
        case.form.as_str(),
        arm.as_str(),
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

fn played(case: &Case, arm: Arm, restart_all: bool) -> Result<Tally, Refusal> {
    play_case(case, arm, restart_all).map_err(|detail| {
        Refusal::new(
            Cause::Case,
            format!(
                "{}.jsonl: an occurrence did not complete: {detail}",
                case.name
            ),
        )
    })
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
    store.unwrap_or(StoreName::Memory).memory_only()?;
    let targets = targets::read()?;
    let cases = read_dir(
        Path::new(cases_dir),
        targets.get("gate", "calendar_days_min"),
    )
    .map_err(|detail| Refusal::new(Cause::Case, detail))?;
    match mode {
        Ok(arm) => {
            let mut lines = Vec::new();
            for case in &cases {
                lines.push(case_line(case, arm, &played(case, arm, restart_all)?));
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
                    played(&case, Arm::None, restart_all)?,
                    played(&case, Arm::Plain, restart_all)?,
                    played(&case, Arm::Admit, restart_all)?,
                ];
                for (arm, tally) in Arm::ALL.iter().zip(&tallies) {
                    lines.push(case_line(&case, *arm, tally));
                }
                played_cases.push((case, tallies));
            }
            let (summary, ok) = verdict(&played_cases, &targets, gate);
            lines.extend(summary);
            Ok(Report { lines, ok })
        }
    }
}
