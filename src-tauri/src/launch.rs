use std::ffi::OsString;
use std::fmt;

/// How the app was started, parsed once from its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// No argument: the app with its window and its tray.
    Run,
    /// `--version`: print the version and exit 0.
    Version,
}

/// Why the arguments are no launch. The app prints it with `USAGE` and exits 64.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Usage {
    NotUnicode,
    Unknown(String),
    Extra(String),
}

impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Usage::NotUnicode => write!(f, "an argument is not valid Unicode"),
            Usage::Unknown(arg) => write!(f, "unknown argument {arg}"),
            Usage::Extra(arg) => write!(f, "{arg} takes no other argument"),
        }
    }
}

pub const USAGE: &str = "usage: halvo [--version]";

/// Parses the arguments of the app, without the program name.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Launch, Usage> {
    let args = args
        .into_iter()
        .map(|arg| arg.into_string().map_err(|_| Usage::NotUnicode))
        .collect::<Result<Vec<String>, Usage>>()?;
    match args.as_slice() {
        [] => Ok(Launch::Run),
        [only] if only == "--version" => Ok(Launch::Version),
        [first, ..] if first == "--version" => Err(Usage::Extra(first.clone())),
        [first, ..] => Err(Usage::Unknown(first.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[cfg(unix)]
    fn not_unicode() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xff, b'x'])
    }

    #[cfg(windows)]
    fn not_unicode() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xd800, u16::from(b'x')])
    }

    #[test]
    fn no_argument_runs_the_app_and_version_alone_prints_the_version() {
        assert_eq!(parse(args(&[])), Ok(Launch::Run));
        assert_eq!(parse(args(&["--version"])), Ok(Launch::Version));
    }

    #[test]
    fn an_argument_that_is_not_unicode_is_a_usage_error() {
        assert_eq!(parse(vec![not_unicode()]), Err(Usage::NotUnicode));
        assert_eq!(
            parse(vec![OsString::from("--version"), not_unicode()]),
            Err(Usage::NotUnicode)
        );
    }

    #[test]
    fn an_unknown_or_an_extra_argument_is_a_usage_error() {
        assert_eq!(
            parse(args(&["--versions"])),
            Err(Usage::Unknown("--versions".into()))
        );
        assert_eq!(
            parse(args(&["--version", "--version"])),
            Err(Usage::Extra("--version".into()))
        );
        assert_eq!(
            parse(args(&["-psn_0_1", "--version"])),
            Err(Usage::Unknown("-psn_0_1".into()))
        );
    }
}
