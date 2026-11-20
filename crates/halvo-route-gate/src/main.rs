//! `route-gate <command> [ARG...]`, the gate tool of contract T6. Exit status 0: the command did its
//! work and every check held; 1: a check missed; 2: a refusal, one line `REFUSED <cause>: <detail>`
//! on standard error and nothing on standard output.

use std::collections::BTreeMap;
use std::io::Write;
use std::num::NonZeroU32;
use std::process::ExitCode;

use halvo_route::policy::{Policy, PolicyError};
use halvo_route::prices::Prices;
use halvo_route_gate::derive::derive;
use halvo_route_gate::generate::generate;
use halvo_route_gate::out::{self, Refused};
use halvo_route_gate::product::{build_product, recorded_setup, score_once, Freeze, ProductRouter};
use halvo_route_gate::routers::{OneTier, PolicyRouter, Random};
use halvo_route_gate::score::DEFAULT_DRAWS;
use halvo_route_gate::selftest::self_test;
use halvo_route_gate::table::Table;
use halvo_route_gate::{Router, Setup};

struct Refusal {
    cause: &'static str,
    detail: String,
}

fn refuse<T>(cause: &'static str, detail: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal {
        cause,
        detail: detail.into(),
    })
}

enum Done {
    Pass(Vec<String>),
    Fail(Vec<String>),
}

type Run = fn(&[String], &mut dyn Write, &mut dyn Write) -> u8;

struct Command {
    name: &'static str,
    run: Run,
}

const COMMANDS: &[Command] = &[
    Command {
        name: "generate",
        run: |args, out, err| finish(generate_command(args), out, err),
    },
    Command {
        name: "import",
        run: halvo_route_gate::import::command,
    },
    Command {
        name: "derive",
        run: |args, out, err| finish(derive_command(args), out, err),
    },
    Command {
        name: "score",
        run: |args, out, err| finish(score_command(args), out, err),
    },
    Command {
        name: "self-test",
        run: |args, out, err| finish(self_test_command(args), out, err),
    },
    Command {
        name: "spend-sim",
        run: halvo_route_gate::spendsim::command,
    },
    Command {
        name: "probe",
        run: halvo_route_gate::probe::command,
    },
    Command {
        name: "commands",
        run: |args, out, err| finish(commands_command(args), out, err),
    },
];

fn main() -> ExitCode {
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let mut args = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(arg) => args.push(arg),
            Err(_) => {
                return ExitCode::from(finish(
                    refuse("usage", "an argument is not UTF-8"),
                    &mut out,
                    &mut err,
                ))
            }
        }
    }
    ExitCode::from(dispatch(&args, &mut out, &mut err))
}

fn dispatch(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let Some((word, rest)) = args.split_first() else {
        return finish(refuse("usage", "no command"), out, err);
    };
    match COMMANDS.iter().find(|command| command.name == word) {
        Some(command) => (command.run)(rest, out, err),
        None => finish(refuse("usage", "unknown command"), out, err),
    }
}

fn finish(done: Result<Done, Refusal>, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let (lines, code) = match done {
        Ok(Done::Pass(lines)) => (lines, 0),
        Ok(Done::Fail(lines)) => (lines, 1),
        Err(refusal) => {
            let _ = writeln!(err, "REFUSED {}: {}", refusal.cause, refusal.detail);
            return 2;
        }
    };
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    match out.write_all(text.as_bytes()) {
        Ok(()) => code,
        Err(_) => 1,
    }
}

fn commands_command(args: &[String]) -> Result<Done, Refusal> {
    options(args, &[])?;
    Ok(Done::Pass(
        COMMANDS
            .iter()
            .map(|command| command.name.to_owned())
            .collect(),
    ))
}

fn options<'a>(args: &'a [String], known: &[&str]) -> Result<BTreeMap<String, &'a str>, Refusal> {
    let mut found = BTreeMap::new();
    let mut words = args.iter();
    while let Some(name) = words.next() {
        if !known.contains(&name.as_str()) {
            return refuse("usage", format!("unknown argument {}", printable(name)));
        }
        let Some(value) = words.next() else {
            return refuse("usage", format!("{name} has no value"));
        };
        if found.insert(name.clone(), value.as_str()).is_some() {
            return refuse("usage", format!("{name} twice"));
        }
    }
    Ok(found)
}

fn printable(word: &str) -> String {
    if word.starts_with("--") && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        word.to_owned()
    } else {
        "of another form".to_owned()
    }
}

fn required<'a>(found: &BTreeMap<String, &'a str>, name: &str) -> Result<&'a str, Refusal> {
    match found.get(name) {
        Some(value) => Ok(value),
        None => refuse("usage", format!("{name} is required")),
    }
}

fn count(text: &str, name: &str) -> Result<u64, Refusal> {
    let canonical =
        !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && !text.starts_with('0');
    match text.parse::<u64>() {
        Ok(value) if canonical => Ok(value),
        _ => refuse("usage", format!("{name} takes a whole number from 1")),
    }
}

fn nonzero_u32(text: &str, name: &str) -> Result<NonZeroU32, Refusal> {
    match u32::try_from(count(text, name)?)
        .ok()
        .and_then(NonZeroU32::new)
    {
        Some(value) => Ok(value),
        None => refuse(
            "usage",
            format!("{name} takes a whole number from 1 to 4294967295"),
        ),
    }
}

fn read_table(path: &str) -> Result<Table, Refusal> {
    let Ok(bytes) = std::fs::read(path) else {
        return refuse("file", "--table cannot be read");
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return refuse("table", "text:utf-8");
    };
    Table::parse(&text).or_else(|error| refuse("table", error.detail()))
}

fn write_new(path: &str, text: &str) -> Result<(), Refusal> {
    match out::write_new(path, text, || {}) {
        Ok(()) => Ok(()),
        Err(Refused::Exists) => refuse("file", "--out exists"),
        Err(Refused::Unwritable) => refuse("file", "--out cannot be written"),
    }
}

fn setup_for(table: &Table, found: &BTreeMap<String, &str>) -> Result<Setup, Refusal> {
    let freeze = found.get("--freeze");
    if table.is_generated() {
        if freeze.is_some() {
            return refuse(
                "usage",
                "--freeze names the freeze file of a recorded table",
            );
        }
        return Ok(Setup::generated());
    }
    let Some(path) = freeze else {
        return recorded_setup(None).or_else(|detail| refuse("file", detail));
    };
    let Ok(bytes) = std::fs::read(path) else {
        return refuse("file", "--freeze cannot be read");
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return refuse("file", "--freeze is not UTF-8 text");
    };
    let setup = recorded_setup(Some(&text)).or_else(|detail| refuse("file", detail))?;
    let rows = Freeze::parse(&text)
        .map(|freeze| freeze.rows)
        .or_else(|detail| refuse("file", detail))?;
    if rows != table.rows().len() as u64 {
        return refuse(
            "file",
            format!(
                "--freeze counts {rows} rows and --table holds {}",
                table.rows().len()
            ),
        );
    }
    Ok(setup)
}

fn generate_command(args: &[String]) -> Result<Done, Refusal> {
    let found = options(args, &["--seed", "--out"])?;
    let seed = count(required(&found, "--seed")?, "--seed")?;
    let out = required(&found, "--out")?;
    write_new(out, &generate(seed).to_text())?;
    Ok(Done::Pass(Vec::new()))
}

fn derive_command(args: &[String]) -> Result<Done, Refusal> {
    let found = options(args, &["--table", "--freeze", "--out"])?;
    let path = required(&found, "--table")?;
    let out = required(&found, "--out")?;
    let table = read_table(path)?;
    let setup = setup_for(&table, &found)?;
    write_new(out, &derive(&table, &setup).to_text())?;
    Ok(Done::Pass(Vec::new()))
}

fn read_policy(path: &str, setup: &Setup) -> Result<Policy, Refusal> {
    let Ok(bytes) = std::fs::read(path) else {
        return refuse("file", "--policy cannot be read");
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return refuse("policy", PolicyError::Rows.as_str());
    };
    Policy::parse_for(&text, &setup.allowed).or_else(|error| refuse("policy", error.as_str()))
}

fn score_command(args: &[String]) -> Result<Done, Refusal> {
    let found = options(
        args,
        &["--table", "--router", "--policy", "--freeze", "--draws"],
    )?;
    let path = required(&found, "--table")?;
    let name = required(&found, "--router")?;
    if !["derived", "product", "one_tier", "random", "peek"].contains(&name) {
        return refuse(
            "usage",
            "--router is one of derived, product, one_tier, random, peek",
        );
    }
    let policy = match (name, found.get("--policy")) {
        ("product", Some(policy)) => Some(*policy),
        ("product", None) => {
            return refuse("usage", "--policy is required with the router product")
        }
        (_, Some(_)) => return refuse("usage", "--policy goes with the router product alone"),
        (_, None) => None,
    };
    let draws = match found.get("--draws") {
        Some(text) => nonzero_u32(text, "--draws")?,
        None => DEFAULT_DRAWS,
    };
    let table = read_table(path)?;
    let setup = setup_for(&table, &found)?;
    let router: Box<dyn Router> = match (name, policy) {
        ("product", Some(policy)) => Box::new(ProductRouter::new(read_policy(policy, &setup)?)),
        ("derived", _) => Box::new(PolicyRouter::derived(&table, &setup)),
        ("one_tier", _) => Box::new(OneTier {
            recorded: setup.recorded,
        }),
        ("random", _) => Box::new(Random::new(&table, &setup)),
        _ => Box::new(PolicyRouter::peek(&table, &setup)),
    };
    let result = match score_once(&table, &setup, router.as_ref(), Prices::release(), draws) {
        Ok(result) => result,
        Err(panicked) => return Ok(Done::Fail(vec![panicked.line()])),
    };
    if result.passed() {
        Ok(Done::Pass(result.lines()))
    } else {
        Ok(Done::Fail(result.lines()))
    }
}

fn self_test_command(args: &[String]) -> Result<Done, Refusal> {
    let found = options(args, &["--tables", "--seed"])?;
    let tables = nonzero_u32(required(&found, "--tables")?, "--tables")?;
    let seed = count(required(&found, "--seed")?, "--seed")?;
    let Ok(result) = self_test(tables, seed, Some(&build_product)) else {
        return refuse("usage", "the seeds from --seed on do not fit a u64");
    };
    if result.passed() {
        Ok(Done::Pass(result.lines()))
    } else {
        Ok(Done::Fail(result.lines()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::dispatch;

    const UNKNOWN: (u8, &str, &str) = (2, "", "REFUSED usage: unknown command\n");

    fn answer(words: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = dispatch(&args, &mut out, &mut err);
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).expect("route-gate writes UTF-8");
        (code, text(out), text(err))
    }

    fn printed() -> Vec<String> {
        let (code, out, err) = answer(&["commands"]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(out.ends_with('\n'));
        out.lines().map(str::to_owned).collect()
    }

    #[test]
    fn the_printing_names_exactly_the_commands_that_the_dispatch_runs() {
        let names = printed();
        assert_eq!(
            names,
            [
                "generate",
                "import",
                "derive",
                "score",
                "self-test",
                "spend-sim",
                "probe",
                "commands"
            ]
        );
        assert_eq!(
            names.iter().collect::<BTreeSet<_>>().len(),
            names.len(),
            "a name printed twice"
        );
        for name in &names {
            let (code, out, err) = answer(&[name]);
            assert_ne!((code, out.as_str(), err.as_str()), UNKNOWN, "{name}");
        }
        for word in [
            "fetch",
            "Generate",
            "generate ",
            "self_test",
            "probe2",
            "--commands",
            "",
        ] {
            assert!(!names.iter().any(|name| name == word), "{word}");
            let (code, out, err) = answer(&[word]);
            assert_eq!((code, out.as_str(), err.as_str()), UNKNOWN, "{word:?}");
        }
    }

    #[test]
    fn the_printing_takes_no_argument_and_no_command_is_refused() {
        let (code, out, err) = answer(&["commands", "--all"]);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (2, "", "REFUSED usage: unknown argument --all\n")
        );
        let (code, out, err) = answer(&[]);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (2, "", "REFUSED usage: no command\n")
        );
    }
}
