use halvo_wire::types::{Cap, Period};
use halvo_wire::{CAP_AMOUNT_MAX, CAP_AMOUNT_MIN};
use std::ffi::OsString;
use std::num::NonZeroU64;

/// The two builds whose grammars differ: a debug build, cargo's dev profile, and a release build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Build {
    Debug,
    Release,
}

/// The build that this crate was compiled as: `Debug` when debug assertions are on, as in
/// cargo's dev profile, and `Release` otherwise. `halvo-secrets` gives its debug-only store
/// to the same builds.
pub const THIS_BUILD: Build = if cfg!(debug_assertions) {
    Build::Debug
} else {
    Build::Release
};

/// What `subscribe` and `manage` do with the URL of the page that the backend answers:
/// open it, or, after `--print-url` in a debug build, print it and open nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    Open,
    Print,
}

/// One command of the closed grammar; `subscribe` and `set-cap` carry the cap as given.
#[derive(Clone, Debug)]
pub enum Command {
    Status,
    Subscribe(Cap),
    SetCap(Cap),
    Cap,
    Manage,
    Delete,
}

/// The whole of what the arguments say.
#[derive(Clone, Debug)]
pub struct Invocation {
    pub show: Show,
    pub command: Command,
}

/// Why the arguments are outside the grammar. The helper then sends nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Usage {
    /// No command is named.
    NoCommand,
    /// An argument is not Unicode.
    NotText,
    /// The first word is no command of the grammar, or a flag that this build does not take.
    Unknown,
    /// `subscribe` or `set-cap` lacks its amount or its period.
    Missing,
    /// An argument follows the command's own operands.
    Extra,
    /// The amount is not decimal digits from `CAP_AMOUNT_MIN` to `CAP_AMOUNT_MAX`.
    Amount,
    /// The period is not `week` or `month`.
    Period,
}

/// Reads `halvo-account [--print-url] COMMAND`, where COMMAND is `status`,
/// `subscribe AMOUNT PERIOD`, `set-cap AMOUNT PERIOD`, `cap`, `manage` or `delete`, and
/// `--print-url` belongs to the grammar of a debug build alone and stands before the command.
/// AMOUNT is decimal digits with no sign and no leading zero, from `CAP_AMOUNT_MIN` to
/// `CAP_AMOUNT_MAX`; PERIOD is `week` or `month`. Anything else is refused.
pub fn parse(args: &[OsString], build: Build) -> Result<Invocation, Usage> {
    let mut words = Vec::with_capacity(args.len());
    for arg in args {
        words.push(arg.to_str().ok_or(Usage::NotText)?);
    }
    let (show, rest) = match (build, words.split_first()) {
        (Build::Debug, Some((&"--print-url", rest))) => (Show::Print, rest),
        _ => (Show::Open, words.as_slice()),
    };
    let (&name, operands) = rest.split_first().ok_or(Usage::NoCommand)?;
    let command = match (name, operands) {
        ("status", []) => Command::Status,
        ("cap", []) => Command::Cap,
        ("manage", []) => Command::Manage,
        ("delete", []) => Command::Delete,
        ("subscribe", [amount, period]) => Command::Subscribe(cap(amount, period)?),
        ("set-cap", [amount, period]) => Command::SetCap(cap(amount, period)?),
        ("status" | "cap" | "manage" | "delete", _) => return Err(Usage::Extra),
        ("subscribe" | "set-cap", [_, _, _, ..]) => return Err(Usage::Extra),
        ("subscribe" | "set-cap", _) => return Err(Usage::Missing),
        _ => return Err(Usage::Unknown),
    };
    Ok(Invocation { show, command })
}

fn cap(amount: &str, period: &str) -> Result<Cap, Usage> {
    let amount = parse_amount(amount).ok_or(Usage::Amount)?;
    let period = match period {
        "week" => Period::Week,
        "month" => Period::Month,
        _ => return Err(Usage::Period),
    };
    Ok(Cap { amount, period })
}

fn parse_amount(text: &str) -> Option<NonZeroU64> {
    let bytes = text.as_bytes();
    let plain = matches!(bytes.first(), Some(b'1'..=b'9'))
        && bytes.len() <= 20
        && bytes.iter().all(u8::is_ascii_digit);
    if !plain {
        return None;
    }
    let value: u64 = text.parse().ok()?;
    if !(CAP_AMOUNT_MIN..=CAP_AMOUNT_MAX).contains(&value) {
        return None;
    }
    NonZeroU64::new(value)
}
