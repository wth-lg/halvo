use std::path::{Path, PathBuf};
use std::process::ExitCode;

use r3_sheet_trial::book;
use r3_sheet_trial::candidate::Candidate;
use r3_sheet_trial::corpus::{self, Spec};
use r3_sheet_trial::facts;
use r3_sheet_trial::mutate::{self, Mutation};
use r3_sheet_trial::parts;
use r3_sheet_trial::trial::{self, Cached, Judgement};

const USAGE: &str = "r3-sheet-trial generate --corpus DIR | run --corpus DIR --out DIR [--candidate umya|rewrite] | mutants --corpus DIR --out DIR --candidate C --work DIR | facts --corpus DIR --out DIR | inspect --corpus DIR";

struct Args {
    command: String,
    corpus: Option<PathBuf>,
    out: Option<PathBuf>,
    work: Option<PathBuf>,
    candidate: Option<Candidate>,
}

fn parse(mut raw: impl Iterator<Item = String>) -> Result<Args, String> {
    let command = raw.next().ok_or("no command")?;
    let mut args = Args {
        command,
        corpus: None,
        out: None,
        work: None,
        candidate: None,
    };
    while let Some(flag) = raw.next() {
        let value = raw.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--corpus" => args.corpus = Some(PathBuf::from(value)),
            "--out" => args.out = Some(PathBuf::from(value)),
            "--work" => args.work = Some(PathBuf::from(value)),
            "--candidate" => {
                args.candidate =
                    Some(Candidate::parse(&value).ok_or_else(|| format!("no candidate {value}"))?)
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let needs: &[&str] = match args.command.as_str() {
        "generate" | "inspect" => &["--corpus"],
        "run" | "facts" => &["--corpus", "--out"],
        "mutants" => &["--corpus", "--out", "--work", "--candidate"],
        other => return Err(format!("unknown command {other}")),
    };
    for flag in needs {
        let given = match *flag {
            "--corpus" => args.corpus.is_some(),
            "--out" => args.out.is_some(),
            "--work" => args.work.is_some(),
            _ => args.candidate.is_some(),
        };
        if !given {
            return Err(format!("{} needs {flag}", args.command));
        }
    }
    Ok(args)
}

fn need<'a>(path: &'a Option<PathBuf>, flag: &str) -> Result<&'a Path, String> {
    path.as_deref().ok_or_else(|| format!("{flag} is required"))
}

fn run(args: &Args) -> Result<bool, String> {
    let corpus = need(&args.corpus, "--corpus")?;
    let out = need(&args.out, "--out")?;
    let candidates: Vec<Candidate> = match args.candidate {
        Some(one) => vec![one],
        None => Candidate::ALL.to_vec(),
    };
    let mut all_ran = true;
    for candidate in candidates {
        for spec in corpus::specs() {
            match trial::run_cell(candidate, &spec, corpus, out) {
                Ok(line) => println!("{line}"),
                Err(why) => {
                    all_ran = false;
                    eprintln!(
                        "miss candidate={} file={} cause={}",
                        candidate.name(),
                        spec.file,
                        why.replace(' ', "_")
                    );
                }
            }
        }
    }
    Ok(all_ran)
}

enum Expect {
    Lost(parts::Kind),
    Differs,
    Cached(Cached),
}

impl Expect {
    fn text(&self) -> String {
        match self {
            Expect::Lost(kind) => format!("lost_names_{kind}"),
            Expect::Differs => "other_cells_same=no".into(),
            Expect::Cached(want) => format!("cached_results={}+other_cells_same=yes", want.name()),
        }
    }

    fn met(&self, judgement: &Judgement) -> bool {
        match self {
            Expect::Lost(kind) => judgement.lost.contains(kind),
            Expect::Differs => !judgement.other_cells_same,
            Expect::Cached(want) => judgement.cached == *want && judgement.other_cells_same,
        }
    }
}

struct Case {
    mutations: Vec<Mutation>,
    base: PathBuf,
    label: &'static str,
    expect: Expect,
}

fn cases(spec: &Spec, original: &Path, saved: &Path) -> Result<Vec<Case>, String> {
    let before = parts::inventory(original)?;
    let after = parts::inventory(saved)?;
    let on_saved = |mutations, expect| Case {
        mutations,
        base: saved.to_path_buf(),
        label: "saved",
        expect,
    };
    let mut list = Vec::new();
    for kind in before.kinds() {
        let mutations = vec![Mutation::RemoveKind(kind)];
        if after.count(kind) > 0 {
            list.push(on_saved(mutations, Expect::Lost(kind)));
        } else {
            list.push(Case {
                mutations,
                base: original.to_path_buf(),
                label: "corpus",
                expect: Expect::Lost(kind),
            });
        }
    }
    let target = book::parse_ref(spec.cell).ok_or("bad cell")?;
    let other = book::read(original)?
        .sheet(spec.sheet)
        .and_then(|cells| {
            cells
                .iter()
                .find(|(pos, cell)| **pos != target && cell.formula.is_none())
                .map(|(pos, _)| book::cell_ref(*pos))
        })
        .ok_or("no other cell")?;
    list.push(on_saved(
        vec![Mutation::ReplaceCell {
            sheet: spec.sheet.into(),
            cell: other,
        }],
        Expect::Differs,
    ));
    if let Some(first) = spec.dependents.first() {
        list.push(on_saved(
            vec![Mutation::SetFormula {
                sheet: first.sheet.into(),
                cell: first.cell.into(),
                text: "SUM(A1:A2)".into(),
            }],
            Expect::Differs,
        ));
        let every = |pick: fn(&corpus::Dependent) -> Option<f64>| -> Vec<Mutation> {
            spec.dependents
                .iter()
                .map(|dependent| Mutation::SetCached {
                    sheet: dependent.sheet.into(),
                    cell: dependent.cell.into(),
                    value: pick(dependent).map(|v| format!("{v}")),
                })
                .collect()
        };
        list.push(on_saved(
            every(|d| Some(d.after)),
            Expect::Cached(Cached::Fresh),
        ));
        list.push(on_saved(
            every(|d| Some(d.before)),
            Expect::Cached(Cached::Stale),
        ));
        list.push(on_saved(every(|_| None), Expect::Cached(Cached::Absent)));
    }
    Ok(list)
}

fn mutants(args: &Args) -> Result<bool, String> {
    let corpus = need(&args.corpus, "--corpus")?;
    let out = need(&args.out, "--out")?;
    let work = need(&args.work, "--work")?;
    let candidate = args.candidate.ok_or("--candidate is required")?;
    std::fs::create_dir_all(work).map_err(|e| format!("create {}: {e}", work.display()))?;
    let mut all_met = true;
    for spec in corpus::specs() {
        let original = corpus.join(spec.file);
        let saved = out.join(candidate.name()).join(spec.file);
        for (index, case) in cases(&spec, &original, &saved)?.into_iter().enumerate() {
            let mutant = work.join(format!("{}-{}-{}", candidate.name(), index + 1, spec.file));
            let mut source = case.base.clone();
            for mutation in &case.mutations {
                mutate::apply(&source, &mutant, mutation)?;
                source = mutant.clone();
            }
            let judgement = trial::judge(&original, &mutant, &spec)?;
            let met = case.expect.met(&judgement);
            all_met &= met;
            let line = trial::Line {
                candidate: candidate.name().into(),
                file: spec.file.into(),
                judgement,
            };
            println!(
                "mutant file={} base={} change={} expect={} met={} line: {line}",
                spec.file,
                case.label,
                case.mutations
                    .iter()
                    .map(Mutation::describe)
                    .collect::<Vec<_>>()
                    .join("+"),
                case.expect.text(),
                if met { "yes" } else { "no" }
            );
        }
    }
    Ok(all_met)
}

fn facts_cmd(args: &Args) -> Result<bool, String> {
    let corpus = need(&args.corpus, "--corpus")?;
    let out = need(&args.out, "--out")?;
    for candidate in Candidate::ALL {
        for spec in corpus::specs() {
            let saved = out.join(candidate.name()).join(spec.file);
            for fact in facts::facts(&corpus.join(spec.file), &saved, &spec)? {
                println!(
                    "fact candidate={} file={} {fact}",
                    candidate.name(),
                    spec.file
                );
            }
        }
    }
    Ok(true)
}

fn inspect(args: &Args) -> Result<bool, String> {
    let corpus = need(&args.corpus, "--corpus")?;
    for spec in corpus::specs() {
        let path = corpus.join(spec.file);
        let read = book::read(&path)?;
        let kinds = parts::inventory(&path)?;
        let sheets: Vec<&str> = read.sheets.iter().map(|(name, _)| name.as_str()).collect();
        println!(
            "corpus file={} sheets={} kinds={}",
            spec.file,
            sheets.join(","),
            parts::list(&kinds.kinds())
        );
        for (kind, found) in &kinds.instances {
            println!("corpus file={} kind={kind} instances={found:?}", spec.file);
        }
        for (name, cells) in &read.sheets {
            let rows: std::collections::BTreeSet<u32> = cells.keys().map(|(row, _)| *row).collect();
            let formulas = cells.values().filter(|cell| cell.formula.is_some()).count();
            println!(
                "corpus file={} sheet={name} cells={} rows={} formulas={}",
                spec.file,
                cells.len(),
                rows.len(),
                formulas
            );
            if spec.file == "formulas.xlsx" {
                for row in 0..8u32 {
                    for col in 0..4u32 {
                        let at = book::cell_ref((row, col));
                        match cells.get(&(row, col)) {
                            Some(cell) => println!(
                                "corpus file={} cell={name}!{at} value={:?} formula={:?}",
                                spec.file, cell.value, cell.formula
                            ),
                            None => println!("corpus file={} cell={name}!{at} empty", spec.file),
                        }
                    }
                }
            }
        }
    }
    Ok(true)
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(why) => {
            eprintln!("REFUSED usage: {why}; {USAGE}");
            return ExitCode::from(2);
        }
    };
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| match args.command.as_str() {
        "generate" => {
            need(&args.corpus, "--corpus").and_then(|dir| corpus::generate(dir).map(|()| true))
        }
        "run" => run(&args),
        "mutants" => mutants(&args),
        "facts" => facts_cmd(&args),
        "inspect" => inspect(&args),
        _ => unreachable!("parse refuses an unknown command"),
    })
    .unwrap_or_else(|_| Err("panic".to_string()));
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(why) => {
            eprintln!("miss cause={}", why.replace(' ', "_"));
            ExitCode::from(1)
        }
    }
}
