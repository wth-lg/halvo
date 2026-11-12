use std::ffi::OsString;
use std::path::PathBuf;

pub const USAGE: &str =
    "usage: r3-model --weights FILE --settings FILE [--requests FILE --replies FILE]";

/// The command line of the child (contracts.md A12.2).
#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub weights: PathBuf,
    pub settings: PathBuf,
    /// The request file and the reply file, given together or not at all.
    pub files: Option<(PathBuf, PathBuf)>,
}

const NAMES: [&str; 4] = ["weights", "settings", "requests", "replies"];

pub fn parse(argv: &[OsString]) -> Result<Args, String> {
    let mut given: [Option<PathBuf>; 4] = [None, None, None, None];
    let mut rest = argv.iter();
    while let Some(word) = rest.next() {
        let text = word.to_string_lossy();
        let at = text
            .strip_prefix("--")
            .and_then(|name| NAMES.iter().position(|known| *known == name))
            .ok_or_else(|| format!("{text} is not an argument"))?;
        if given[at].is_some() {
            return Err(format!("{text} is given twice"));
        }
        let value = rest.next().ok_or_else(|| format!("{text} has no value"))?;
        given[at] = Some(PathBuf::from(value));
    }
    let [weights, settings, requests, replies] = given;
    let weights = weights.ok_or_else(|| "--weights is missing".to_string())?;
    let settings = settings.ok_or_else(|| "--settings is missing".to_string())?;
    let files = match (requests, replies) {
        (Some(requests), Some(replies)) => Some((requests, replies)),
        (None, None) => None,
        _ => return Err("--requests and --replies go together".to_string()),
    };
    Ok(Args {
        weights,
        settings,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<OsString> {
        text.split_whitespace().map(OsString::from).collect()
    }

    #[test]
    fn the_two_forms_parse() {
        assert_eq!(
            parse(&words("--weights w.gguf --settings s.toml")),
            Ok(Args {
                weights: PathBuf::from("w.gguf"),
                settings: PathBuf::from("s.toml"),
                files: None,
            })
        );
        assert_eq!(
            parse(&words(
                "--replies out.txt --settings s.toml --requests in.txt --weights w.gguf"
            )),
            Ok(Args {
                weights: PathBuf::from("w.gguf"),
                settings: PathBuf::from("s.toml"),
                files: Some((PathBuf::from("in.txt"), PathBuf::from("out.txt"))),
            })
        );
    }

    #[test]
    fn a_bad_command_line_names_its_fault() {
        let cases = [
            ("--settings s.toml", "--weights is missing"),
            ("--weights w.gguf", "--settings is missing"),
            (
                "--weights w.gguf --settings s.toml --requests in.txt",
                "--requests and --replies go together",
            ),
            (
                "--weights w.gguf --settings s.toml --replies out.txt",
                "--requests and --replies go together",
            ),
            (
                "--weights w.gguf --weights v.gguf --settings s.toml",
                "--weights is given twice",
            ),
            ("--weights w.gguf --settings", "--settings has no value"),
            (
                "--weights w.gguf --settings s.toml --mmap on",
                "--mmap is not an argument",
            ),
            (
                "--weights w.gguf --settings s.toml extra",
                "extra is not an argument",
            ),
            ("", "--weights is missing"),
        ];
        for (line, fault) in cases {
            assert_eq!(parse(&words(line)), Err(fault.to_string()), "{line}");
        }
    }
}
