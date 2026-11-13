use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use halvo_sample::form::{parse_seed, Refusal};

const USAGE: &str =
    "halvo-sample compose|split|build|check-labels|input-copy|leak|mask-layers ARGUMENTS";

/// The flags of a command with their values, and its other arguments in order. A flag twice, an unknown flag
/// or a flag with no value is `usage`.
fn parse(
    args: &[String],
    with_value: &[&str],
    alone: &[&str],
) -> Result<(BTreeMap<String, String>, Vec<String>), Refusal> {
    let mut flags = BTreeMap::new();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with("--") {
            if with_value.contains(&a.as_str()) {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| Refusal::new("usage", a.clone()))?;
                if flags.insert(a.clone(), v.clone()).is_some() {
                    return Err(Refusal::new("usage", a.clone()));
                }
                i += 2;
                continue;
            }
            if alone.contains(&a.as_str()) {
                if flags.insert(a.clone(), String::new()).is_some() {
                    return Err(Refusal::new("usage", a.clone()));
                }
                i += 1;
                continue;
            }
            return Err(Refusal::new("usage", a.clone()));
        }
        rest.push(a.clone());
        i += 1;
    }
    Ok((flags, rest))
}

fn need(flags: &BTreeMap<String, String>, name: &str) -> Result<String, Refusal> {
    flags
        .get(name)
        .cloned()
        .ok_or_else(|| Refusal::new("usage", format!("missing {name}")))
}

fn seed(flags: &BTreeMap<String, String>) -> Result<u64, Refusal> {
    let s = need(flags, "--seed")?;
    parse_seed(&s).ok_or_else(|| Refusal::new("usage", "--seed"))
}

fn paths(list: &[String]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

fn run(args: &[String]) -> Result<(Vec<String>, bool), Refusal> {
    let Some((command, args)) = args.split_first() else {
        return Err(Refusal::new("usage", USAGE));
    };
    let done = |lines: Vec<String>| (lines, true);
    match command.as_str() {
        "build" => {
            let (f, rest) = parse(args, &["--tier", "--layer-records", "--seed", "--out"], &[])?;
            let tier = f.get("--tier").cloned();
            if let Some(t) = &tier {
                if !["gen-dev", "gen-unseen", "gen-check", "gen-heldout"].contains(&t.as_str()) {
                    return Err(Refusal::new("usage", "--tier"));
                }
            }
            let sealed = matches!(tier.as_deref(), Some("gen-check" | "gen-heldout"));
            if sealed != f.contains_key("--layer-records") {
                return Err(Refusal::new("usage", "--layer-records"));
            }
            if rest.is_empty() {
                return Err(Refusal::new("usage", "missing SCRIPT"));
            }
            let a = halvo_sample::build::BuildArgs {
                tier,
                layer_records: f.get("--layer-records").map(PathBuf::from),
                seed: seed(&f)?,
                out: PathBuf::from(need(&f, "--out")?),
                scripts: paths(&rest),
            };
            halvo_sample::build::build(&a).map(done)
        }
        _ => Err(Refusal::new("usage", USAGE)),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok((lines, pass)) => {
            let mut text = String::new();
            for l in lines {
                text.push_str(&l);
                text.push('\n');
            }
            print!("{text}");
            if pass {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(r) => {
            eprintln!("REFUSED {}: {}", r.cause, r.detail);
            ExitCode::from(2)
        }
    }
}
