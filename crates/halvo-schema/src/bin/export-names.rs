use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::process::ExitCode;

pub(crate) const LOCKED: &[&[&str]] = &[
    halvo_schema::category::NAMES,
    halvo_schema::ids::NAMES,
    halvo_schema::kinds::NAMES,
    halvo_schema::record::NAMES,
    halvo_schema::request::NAMES,
    halvo_schema::site::NAMES,
];

pub(crate) const LATER: &[&[&str]] = &[
    halvo_schema::coordinator::NAMES,
    halvo_schema::executable::NAMES,
    halvo_schema::learning::NAMES,
];

pub(crate) fn names_file() -> String {
    let kinds: Vec<&str> = halvo_schema::kinds::registry()
        .flat_map(|kind| [kind.name, kind.family])
        .collect();
    names_with(LATER, &kinds)
}

pub(crate) fn names_with(later: &[&[&str]], kinds: &[&str]) -> String {
    let mut names: Vec<&str> = LOCKED
        .iter()
        .chain(later)
        .flat_map(|list| list.iter().copied())
        .chain(kinds.iter().copied())
        .collect();
    names.sort_unstable();
    names.dedup();
    names.iter().map(|name| format!("{name}\n")).collect()
}

enum Refusal {
    Usage(String),
    Exists(String),
    Output(String),
}

fn out_path(args: Vec<OsString>) -> Result<OsString, Refusal> {
    let usage = |detail: &str| Refusal::Usage(detail.to_owned());
    let mut out = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg != "--out" {
            return Err(Refusal::Usage(arg.to_string_lossy().into_owned()));
        }
        if out.is_some() {
            return Err(usage("--out"));
        }
        match args.next() {
            Some(path) if !path.is_empty() && path != "--out" => out = Some(path),
            _ => return Err(usage("--out")),
        }
    }
    out.ok_or_else(|| usage("missing --out"))
}

fn write(file: &OsString) -> Result<usize, Refusal> {
    let shown = file.to_string_lossy().into_owned();
    if std::fs::symlink_metadata(file).is_ok() {
        return Err(Refusal::Exists(shown));
    }
    let text = names_file();
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(file)
        .map_err(|e| match e.kind() {
            ErrorKind::AlreadyExists => Refusal::Exists(shown.clone()),
            _ => Refusal::Output(shown.clone()),
        })?;
    target
        .write_all(text.as_bytes())
        .and_then(|()| target.flush())
        .map_err(|_| Refusal::Output(shown))?;
    Ok(text.lines().count())
}

fn main() -> ExitCode {
    let result = out_path(std::env::args_os().skip(1).collect()).and_then(|file| write(&file));
    match result {
        Ok(count) => {
            let _ = writeln!(std::io::stdout(), "names {count}");
            ExitCode::SUCCESS
        }
        Err(refusal) => {
            let (cause, detail) = match refusal {
                Refusal::Usage(detail) => ("usage", detail),
                Refusal::Exists(detail) => ("exists", detail),
                Refusal::Output(detail) => ("output", detail),
            };
            let _ = writeln!(std::io::stderr(), "REFUSED {cause}: {detail}");
            ExitCode::from(2)
        }
    }
}
