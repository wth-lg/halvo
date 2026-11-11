//! `route-gate <command> [ARG...]`, the gate tool of contract T6. Exit status 0: the command did its
//! work and every check held; 1: a check missed; 2: a refusal, one line `REFUSED <cause>: <detail>`
//! on standard error and nothing on standard output.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::num::NonZeroU32;
use std::process::ExitCode;

use halvo_route::prices::Prices;
use halvo_route_gate::derive::derive;
use halvo_route_gate::generate::generate;
use halvo_route_gate::routers::{OneTier, PolicyRouter, Random};
use halvo_route_gate::score::{score, DEFAULT_DRAWS};
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

fn main() -> ExitCode {
    let mut args = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(arg) => args.push(arg),
            Err(_) => return finish(refuse("usage", "an argument is not UTF-8")),
        }
    }
    let rest = args.get(1..).unwrap_or_default();
    let done = match args.first().map(String::as_str) {
        Some("generate") => generate_command(rest),
        Some("import") => not_built(),
        Some("derive") => derive_command(rest),
        Some("score") => score_command(rest),
        Some("self-test") => self_test_command(rest),
        Some("spend-sim") => not_built(),
        Some("probe") => not_built(),
        Some(_) => refuse("usage", "unknown command"),
        None => refuse("usage", "no command"),
    };
    finish(done)
}

fn finish(done: Result<Done, Refusal>) -> ExitCode {
    let (lines, code) = match done {
        Ok(Done::Pass(lines)) => (lines, 0),
        Ok(Done::Fail(lines)) => (lines, 1),
        Err(refusal) => {
            eprintln!("REFUSED {}: {}", refusal.cause, refusal.detail);
            return ExitCode::from(2);
        }
    };
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Ok(()) => ExitCode::from(code),
        Err(_) => ExitCode::from(1),
    }
}

fn not_built() -> Result<Done, Refusal> {
    refuse("usage", "not built")
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
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return refuse("file", "--out exists")
        }
        Err(_) => return refuse("file", "--out cannot be written"),
    };
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    match written {
        Ok(()) => Ok(()),
        Err(_) => {
            let _ = std::fs::remove_file(path);
            refuse("file", "--out cannot be written")
        }
    }
}

fn generated_only(table: &Table, found: &BTreeMap<String, &str>) -> Result<(), Refusal> {
    if !table.is_generated() {
        return not_built().map(|_| ());
    }
    if found.contains_key("--freeze") {
        return refuse(
            "usage",
            "--freeze names the freeze file of a recorded table",
        );
    }
    Ok(())
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
    generated_only(&table, &found)?;
    write_new(out, &derive(&table, &Setup::generated()).to_text())?;
    Ok(Done::Pass(Vec::new()))
}

fn score_command(args: &[String]) -> Result<Done, Refusal> {
    let found = options(
        args,
        &["--table", "--router", "--policy", "--freeze", "--draws"],
    )?;
    let path = required(&found, "--table")?;
    let name = required(&found, "--router")?;
    match name {
        "derived" | "one_tier" | "random" | "peek" => {}
        "product" => return not_built(),
        _ => {
            return refuse(
                "usage",
                "--router is one of derived, product, one_tier, random, peek",
            )
        }
    }
    if found.contains_key("--policy") {
        return refuse("usage", "--policy goes with the router product alone");
    }
    let draws = match found.get("--draws") {
        Some(text) => nonzero_u32(text, "--draws")?,
        None => DEFAULT_DRAWS,
    };
    let table = read_table(path)?;
    generated_only(&table, &found)?;
    let setup = Setup::generated();
    let router: Box<dyn Router> = match name {
        "derived" => Box::new(PolicyRouter::derived(&table, &setup)),
        "one_tier" => Box::new(OneTier {
            recorded: setup.recorded,
        }),
        "random" => Box::new(Random::new(&table, &setup)),
        _ => Box::new(PolicyRouter::peek(&table, &setup)),
    };
    let result = score(&table, &setup, router.as_ref(), Prices::release(), draws);
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
    let Ok(result) = self_test(tables, seed, None) else {
        return refuse("usage", "the seeds from --seed on do not fit a u64");
    };
    if result.passed() {
        Ok(Done::Pass(result.lines()))
    } else {
        Ok(Done::Fail(result.lines()))
    }
}
