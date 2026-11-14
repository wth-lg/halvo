use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    Run,
    Version,
    CheckPage {
        config_dir: PathBuf,
        within: Duration,
    },
}

pub const MAX_WITHIN_S: u64 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Quit,
    Usage,
    PageLoaded,
    PageNotLoaded,
}

impl Exit {
    pub const fn code(self) -> i32 {
        match self {
            Exit::Quit | Exit::PageLoaded => 0,
            Exit::PageNotLoaded => 1,
            Exit::Usage => 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Usage {
    NotUnicode,
    Unknown(String),
    Extra(String),
    Missing(&'static str),
    Repeated(&'static str),
    NoValue(&'static str),
    RelativeDir,
    BadSeconds(String),
}

impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Usage::NotUnicode => write!(f, "an argument is not valid Unicode"),
            Usage::Unknown(arg) => write!(f, "unknown argument {arg}"),
            Usage::Extra(arg) => write!(f, "{arg} takes no other argument"),
            Usage::Missing(option) => write!(f, "--check-page needs {option}"),
            Usage::Repeated(option) => write!(f, "{option} is given twice"),
            Usage::NoValue(option) => write!(f, "{option} needs a value"),
            Usage::RelativeDir => write!(f, "--config-dir needs an absolute path"),
            Usage::BadSeconds(value) => write!(
                f,
                "--within-s needs a whole number of seconds from 1 to {MAX_WITHIN_S}, not {value}"
            ),
        }
    }
}

pub const USAGE: &str = "usage: halvo [--version | --check-page --config-dir DIR --within-s N]";

pub fn parse<I: IntoIterator<Item = OsString>>(
    args_after_program_name: I,
) -> Result<Launch, Usage> {
    let args = args_after_program_name
        .into_iter()
        .map(|arg| arg.into_string().map_err(|_| Usage::NotUnicode))
        .collect::<Result<Vec<String>, Usage>>()?;
    match args.as_slice() {
        [] => Ok(Launch::Run),
        [only] if only == "--version" => Ok(Launch::Version),
        [first, ..] if first == "--version" => Err(Usage::Extra(first.clone())),
        [first, rest @ ..] if first == "--check-page" => check_page(rest),
        [first, ..] => Err(Usage::Unknown(first.clone())),
    }
}

fn check_page(options: &[String]) -> Result<Launch, Usage> {
    let mut config_dir = None;
    let mut within = None;
    let mut rest = options.iter();
    while let Some(option) = rest.next() {
        let (name, slot): (&'static str, &mut Option<String>) = match option.as_str() {
            "--config-dir" => ("--config-dir", &mut config_dir),
            "--within-s" => ("--within-s", &mut within),
            other => return Err(Usage::Unknown(other.to_string())),
        };
        let value = rest.next().ok_or(Usage::NoValue(name))?;
        if slot.replace(value.clone()).is_some() {
            return Err(Usage::Repeated(name));
        }
    }
    let config_dir = PathBuf::from(config_dir.ok_or(Usage::Missing("--config-dir DIR"))?);
    if !config_dir.is_absolute() {
        return Err(Usage::RelativeDir);
    }
    let within = within.ok_or(Usage::Missing("--within-s N"))?;
    let seconds = within
        .parse::<u64>()
        .ok()
        .filter(|s| (1..=MAX_WITHIN_S).contains(s))
        .ok_or_else(|| Usage::BadSeconds(within.clone()))?;
    Ok(Launch::CheckPage {
        config_dir,
        within: Duration::from_secs(seconds),
    })
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
    fn each_exit_has_its_code() {
        assert_eq!(Exit::Quit.code(), 0);
        assert_eq!(Exit::PageLoaded.code(), 0);
        assert_eq!(Exit::PageNotLoaded.code(), 1);
        assert_eq!(Exit::Usage.code(), 64);
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

    fn absolute() -> String {
        if cfg!(windows) {
            r"C:\check".to_string()
        } else {
            "/check".to_string()
        }
    }

    #[test]
    fn check_page_takes_an_absolute_config_dir_and_a_bound_in_seconds() {
        let dir = absolute();
        let expected = Ok(Launch::CheckPage {
            config_dir: PathBuf::from(&dir),
            within: Duration::from_secs(60),
        });
        assert_eq!(
            parse(args(&[
                "--check-page",
                "--config-dir",
                &dir,
                "--within-s",
                "60"
            ])),
            expected
        );
        assert_eq!(
            parse(args(&[
                "--check-page",
                "--within-s",
                "60",
                "--config-dir",
                &dir
            ])),
            expected
        );
        assert_eq!(
            parse(args(&[
                "--check-page",
                "--config-dir",
                &dir,
                "--within-s",
                "3600"
            ])),
            Ok(Launch::CheckPage {
                config_dir: PathBuf::from(&dir),
                within: Duration::from_secs(MAX_WITHIN_S),
            })
        );
    }

    #[test]
    fn check_page_refuses_a_missing_repeated_relative_or_bad_option() {
        let dir = absolute();
        let cases: [(&[&str], Usage); 10] = [
            (&["--check-page"], Usage::Missing("--config-dir DIR")),
            (
                &["--check-page", "--config-dir", &dir],
                Usage::Missing("--within-s N"),
            ),
            (
                &["--check-page", "--within-s", "60"],
                Usage::Missing("--config-dir DIR"),
            ),
            (
                &["--check-page", "--config-dir", "check", "--within-s", "60"],
                Usage::RelativeDir,
            ),
            (
                &["--check-page", "--config-dir", &dir, "--within-s", "0"],
                Usage::BadSeconds("0".into()),
            ),
            (
                &["--check-page", "--config-dir", &dir, "--within-s", "3601"],
                Usage::BadSeconds("3601".into()),
            ),
            (
                &["--check-page", "--config-dir", &dir, "--within-s", "1.5"],
                Usage::BadSeconds("1.5".into()),
            ),
            (
                &["--check-page", "--config-dir", &dir, "--config-dir", &dir],
                Usage::Repeated("--config-dir"),
            ),
            (
                &["--check-page", "--within-s"],
                Usage::NoValue("--within-s"),
            ),
            (
                &["--check-page", "--version"],
                Usage::Unknown("--version".into()),
            ),
        ];
        for (list, usage) in cases {
            assert_eq!(parse(args(list)), Err(usage), "{list:?}");
        }
        assert_eq!(
            parse(args(&["--config-dir", &dir, "--check-page"])),
            Err(Usage::Unknown("--config-dir".into()))
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
