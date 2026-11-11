use std::ffi::OsString;

use halvo_cap::Limit;

#[derive(Debug, PartialEq, Eq)]
pub enum Form {
    Measure {
        limit: Limit,
        runs: u64,
        command: Vec<OsString>,
    },
    Control {
        limit: Limit,
    },
    ControlChild {
        limit: Limit,
    },
}

fn positive(value: Option<&OsString>, option: &str) -> Result<u64, String> {
    let value = value.ok_or_else(|| format!("{option} needs a value"))?;
    let text = value
        .to_str()
        .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| format!("{option} takes a positive decimal integer"))?;
    match text.parse::<u64>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!(
            "{option} takes a positive decimal integer below 2^64, got {text}"
        )),
    }
}

pub fn parse(argv: &[OsString]) -> Result<Form, String> {
    let mut limit = None;
    let mut runs = None;
    let mut control = false;
    let mut control_child = false;
    let mut command = None;
    let mut i = 0;
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--" {
            command = Some(argv[i + 1..].to_vec());
            break;
        }
        match arg.to_str() {
            Some("--limit") if limit.is_none() => {
                i += 1;
                limit = Some(positive(argv.get(i), "--limit")?);
            }
            Some("--runs") if runs.is_none() => {
                i += 1;
                runs = Some(positive(argv.get(i), "--runs")?);
            }
            Some("--control") if !control => control = true,
            Some("--control-child") if !control_child => control_child = true,
            Some("--limit" | "--runs" | "--control" | "--control-child") => {
                return Err(format!("{} is given twice", arg.to_string_lossy()));
            }
            _ => return Err(format!("unknown argument {}", arg.to_string_lossy())),
        }
        i += 1;
    }
    let limit = limit
        .and_then(Limit::new)
        .ok_or_else(|| "--limit is missing".to_string())?;
    if control_child {
        if control || runs.is_some() || command.is_some() {
            return Err(
                "--control-child takes --limit alone, with no --runs, --control or command"
                    .to_string(),
            );
        }
        return Ok(Form::ControlChild { limit });
    }
    if control {
        if runs.is_some() || command.is_some() {
            return Err("--control takes no --runs and no command".to_string());
        }
        return Ok(Form::Control { limit });
    }
    let runs = runs.ok_or_else(|| "--runs is missing".to_string())?;
    match command {
        Some(command) if !command.is_empty() => Ok(Form::Measure {
            limit,
            runs,
            command,
        }),
        _ => Err("no command after --".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_command_form_keeps_every_word_after_the_first_separator() {
        let form = parse(&args(&[
            "--limit", "7", "--runs", "5", "--", "a", "--", "--limit",
        ]));
        assert_eq!(
            form,
            Ok(Form::Measure {
                limit: Limit::new(7).unwrap(),
                runs: 5,
                command: args(&["a", "--", "--limit"]),
            })
        );
    }

    #[test]
    fn the_control_forms_take_the_limit_alone() {
        assert_eq!(
            parse(&args(&["--control", "--limit", "9"])),
            Ok(Form::Control {
                limit: Limit::new(9).unwrap()
            })
        );
        assert_eq!(
            parse(&args(&["--limit", "9", "--control-child"])),
            Ok(Form::ControlChild {
                limit: Limit::new(9).unwrap()
            })
        );
    }

    #[test]
    fn a_count_is_a_positive_decimal_integer() {
        for bad in [
            "0",
            "",
            "1.5",
            "1e3",
            " 4",
            "4 ",
            "-1",
            "+1",
            "0x10",
            "18446744073709551616",
        ] {
            assert!(
                parse(&args(&["--limit", bad, "--runs", "1", "--", "x"])).is_err(),
                "{bad:?}"
            );
        }
        assert!(parse(&args(&["--limit", "007", "--runs", "1", "--", "x"])).is_ok());
        assert!(parse(&args(&["--limit", "18446744073709551615", "--control"])).is_ok());
    }
}
