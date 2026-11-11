use std::io::Read;
use std::path::Path;

use crate::case::{named_and_sized, read_case, Case, Form, Kind, BYTES_MAX};
use crate::model::{self, Arm, Tally};
use crate::targets::Targets;
use crate::Refusal;

pub fn line(case: &Case, arm: Arm, tally: &Tally) -> String {
    format!(
        "case name={} kind={} form={} arm={} sort={} occurrences={} drifts={} assisted={} failed={} harm={} reverted_good={}\n",
        case.name,
        case.kind.as_str(),
        case.form.as_str(),
        arm.name(),
        arm.sort(),
        tally.occurrences,
        tally.drifts,
        tally.assisted,
        tally.failed,
        tally.harm,
        tally.reverted_good
    )
}

pub fn completed(file: &str, arm: Arm, tally: Tally) -> Result<Tally, Refusal> {
    if tally.failed > 0 {
        return Err(Refusal::Case(format!(
            "{file}: an occurrence did not complete in the arm {} with sort={}",
            arm.name(),
            arm.sort()
        )));
    }
    Ok(tally)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Admit,
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub plain: Tally,
    pub admit: Tally,
}

pub fn rule(rows: &[Row], targets: &Targets) -> Rule {
    let faults = rows.iter().filter(|row| row.kind == Kind::Fault);
    let wins = faults
        .clone()
        .filter(|row| row.admit.harm < row.plain.harm)
        .count();
    let losses = faults.filter(|row| row.admit.harm > row.plain.harm).count();
    let benign = rows.iter().filter(|row| row.kind == Kind::Benign);
    let admit: u64 = benign
        .clone()
        .map(|row| u64::from(row.admit.assisted))
        .sum();
    let plain: u64 = benign.map(|row| u64::from(row.plain.assisted)).sum();
    let wins = u64::try_from(wins).unwrap_or(u64::MAX);
    if wins >= targets.dev_critical
        && losses == 0
        && admit <= plain.saturating_add(targets.dev_cost_budget)
    {
        Rule::Admit
    } else {
        Rule::Plain
    }
}

fn read_one(name: &str, path: &Path, calendar_days_min: u64) -> Result<Case, Refusal> {
    let refuse = |rule: String| Refusal::Case(format!("{name}: {rule}"));
    if !path.is_file() {
        return Err(refuse("not a case file".to_owned()));
    }
    let unreadable = || refuse("the file cannot be read".to_owned());
    let size = std::fs::metadata(path).map_err(|_| unreadable())?.len();
    named_and_sized(name, size).map_err(refuse)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(BYTES_MAX + 1).read_to_end(&mut bytes))
        .map_err(|_| unreadable())?;
    read_case(name, &bytes, calendar_days_min).map_err(refuse)
}

pub fn read_dir(dir: &Path, calendar_days_min: u64) -> Result<Vec<(String, Case)>, Refusal> {
    let unreadable = || Refusal::Case("the case directory cannot be read".to_owned());
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|_| unreadable())? {
        let entry = entry.map_err(|_| unreadable())?;
        let name = entry.file_name().into_string().map_err(|name| {
            Refusal::Case(format!("{}: not a case file", name.to_string_lossy()))
        })?;
        entries.push((name, entry.path()));
    }
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    entries
        .into_iter()
        .map(|(name, path)| read_one(&name, &path, calendar_days_min).map(|case| (name, case)))
        .collect()
}

pub fn trial(dir: &Path, targets: &Targets) -> Result<String, Refusal> {
    block(&read_dir(dir, targets.calendar_days_min)?, targets)
}

pub fn block(cases: &[(String, Case)], targets: &Targets) -> Result<String, Refusal> {
    let of = |kind: Kind| {
        u64::try_from(cases.iter().filter(|(_, case)| case.kind == kind).count())
            .unwrap_or(u64::MAX)
    };
    let (benign, fault) = (of(Kind::Benign), of(Kind::Fault));
    if benign != targets.dev_benign || fault != targets.dev_fault {
        return Err(Refusal::Case(format!(
            "the directory holds {benign} benign and {fault} fault cases, not {} and {}",
            targets.dev_benign, targets.dev_fault
        )));
    }
    let mut text = String::new();
    let mut rows = Vec::new();
    for (name, case) in cases {
        let mut tallies = Vec::new();
        for arm in Arm::TRIAL {
            let tally = completed(name, arm, model::run(case, arm))?;
            text.push_str(&line(case, arm, &tally));
            tallies.push(tally);
        }
        rows.push(Row {
            kind: case.kind,
            plain: tallies[1],
            admit: tallies[2],
        });
    }
    text.push_str(match rule(&rows, targets) {
        Rule::Admit => "verdict rule=admit\n",
        Rule::Plain => "verdict rule=plain\n",
    });
    Ok(text)
}

pub fn count(dir: &Path, targets: &Targets) -> Result<String, Refusal> {
    let cases = read_dir(dir, targets.calendar_days_min)?;
    Ok(Form::ALL
        .iter()
        .map(|form| {
            let of_form = || cases.iter().filter(|(_, case)| case.form == *form);
            format!(
                "spread form={} cases={} shows1={} shows2={}\n",
                form.as_str(),
                of_form().count(),
                of_form().filter(|(_, case)| case.shows == 1).count(),
                of_form().filter(|(_, case)| case.shows == 2).count()
            )
        })
        .collect())
}

pub fn one(file: &Path, arm: Arm, targets: &Targets) -> Result<String, Refusal> {
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    case_line(
        &name,
        &read_one(&name, file, targets.calendar_days_min)?,
        arm,
    )
}

pub fn case_line(name: &str, case: &Case, arm: Arm) -> Result<String, Refusal> {
    let tally = completed(name, arm, model::run(case, arm))?;
    Ok(line(case, arm, &tally))
}
