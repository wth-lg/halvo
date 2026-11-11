use std::ffi::OsStr;
use std::fmt;
use std::path::PathBuf;

/// The window arm of I11.3: `main` is today's form, `a` the capture run's form, `a2` adds the
/// quieting of a hidden web view, `b` destroys the window on close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    Main,
    A,
    A2,
    B,
}

/// The way state reaches the page in the sync trial of I11.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncWay {
    Get,
    Channel,
    Events,
}

/// What `builder` makes: the control port (`None` takes a free port and prints it), the pages
/// directory, the arm, the sync way and whether the tray item is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub control_port: Option<u16>,
    pub pages: PathBuf,
    pub arm: Arm,
    pub sync: Option<SyncWay>,
    pub tray: bool,
}

/// Why the app does not start; `cause` is the word that the refusal line prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Usage(String),
    NotHosted,
}

impl Arm {
    pub fn name(self) -> &'static str {
        match self {
            Arm::Main => "main",
            Arm::A => "a",
            Arm::A2 => "a2",
            Arm::B => "b",
        }
    }

    fn parse(word: &str) -> Option<Arm> {
        [Arm::Main, Arm::A, Arm::A2, Arm::B]
            .into_iter()
            .find(|arm| arm.name() == word)
    }
}

impl SyncWay {
    pub fn name(self) -> &'static str {
        match self {
            SyncWay::Get => "get",
            SyncWay::Channel => "channel",
            SyncWay::Events => "events",
        }
    }

    fn parse(word: &str) -> Option<SyncWay> {
        [SyncWay::Get, SyncWay::Channel, SyncWay::Events]
            .into_iter()
            .find(|way| way.name() == word)
    }
}

impl Refusal {
    pub fn cause(&self) -> &'static str {
        match self {
            Refusal::Usage(_) => "usage",
            Refusal::NotHosted => "not_hosted",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Usage(why) => write!(f, "refused cause=usage {why}"),
            Refusal::NotHosted => write!(f, "refused cause=not_hosted"),
        }
    }
}

/// The app's own `pages/` directory, where `--pages` is not given.
pub fn default_pages_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pages")
}

impl Default for Options {
    fn default() -> Self {
        Options {
            control_port: None,
            pages: default_pages_dir(),
            arm: Arm::Main,
            sync: None,
            tray: true,
        }
    }
}

/// Reads the flags `--arm main|a|a2|b`, `--sync get|channel|events`, `--control-port N` and
/// `--pages DIR`; anything else, a missing value or a flag given twice is `Refusal::Usage`.
pub fn parse_args<I, S>(args: I) -> Result<Options, Refusal>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let usage = |why: String| Refusal::Usage(why);
    let mut options = Options::default();
    let mut seen: Vec<String> = Vec::new();
    let mut args = args.into_iter().map(Into::into);
    while let Some(flag) = args.next() {
        if !matches!(
            flag.as_str(),
            "--arm" | "--sync" | "--control-port" | "--pages"
        ) {
            return Err(usage(format!("unknown argument {flag:?}")));
        }
        if seen.contains(&flag) {
            return Err(usage(format!("{flag} given twice")));
        }
        let value = args
            .next()
            .ok_or_else(|| usage(format!("{flag} needs a value")))?;
        match flag.as_str() {
            "--arm" => {
                options.arm = Arm::parse(&value)
                    .ok_or_else(|| usage("--arm takes main, a, a2 or b".into()))?;
            }
            "--sync" => {
                options.sync = Some(
                    SyncWay::parse(&value)
                        .ok_or_else(|| usage("--sync takes get, channel or events".into()))?,
                );
            }
            "--control-port" => {
                let port = value
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                    .ok_or_else(|| usage("--control-port takes a port from 1 to 65535".into()))?;
                options.control_port = Some(port);
            }
            _ => {
                if value.is_empty() {
                    return Err(usage("--pages takes a directory".into()));
                }
                options.pages = PathBuf::from(value);
            }
        }
        seen.push(flag);
    }
    Ok(options)
}

/// `Ok` where `CI` or `R4_GUEST` is set to a value that is not empty, else `Refusal::NotHosted`
/// (I11.1: no window and no tray item in the login session of the development Mac).
pub fn check_hosted(ci: Option<&OsStr>, guest: Option<&OsStr>) -> Result<(), Refusal> {
    let set = |value: Option<&OsStr>| value.is_some_and(|v| !v.is_empty());
    if set(ci) || set(guest) {
        Ok(())
    } else {
        Err(Refusal::NotHosted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, Refusal> {
        parse_args(args.iter().copied())
    }

    #[test]
    fn no_flags_give_the_defaults() {
        let o = parse(&[]).unwrap();
        assert_eq!(o.control_port, None);
        assert_eq!(o.arm, Arm::Main);
        assert_eq!(o.sync, None);
        assert!(o.tray);
        assert_eq!(o.pages, default_pages_dir());
        assert_eq!(o, Options::default());
    }

    #[test]
    fn default_pages_dir_is_the_crates_own_pages() {
        let dir = default_pages_dir();
        assert_eq!(dir, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pages"));
    }

    #[test]
    fn every_arm_and_way_parses_and_names_itself() {
        for (word, arm) in [
            ("main", Arm::Main),
            ("a", Arm::A),
            ("a2", Arm::A2),
            ("b", Arm::B),
        ] {
            assert_eq!(parse(&["--arm", word]).unwrap().arm, arm);
            assert_eq!(arm.name(), word);
        }
        for (word, way) in [
            ("get", SyncWay::Get),
            ("channel", SyncWay::Channel),
            ("events", SyncWay::Events),
        ] {
            assert_eq!(parse(&["--sync", word]).unwrap().sync, Some(way));
            assert_eq!(way.name(), word);
        }
    }

    #[test]
    fn port_and_pages_are_read() {
        let o = parse(&[
            "--control-port",
            "47001",
            "--pages",
            "some/dir",
            "--arm",
            "b",
        ])
        .unwrap();
        assert_eq!(o.control_port, Some(47001));
        assert_eq!(o.pages, PathBuf::from("some/dir"));
        assert_eq!(o.arm, Arm::B);
    }

    #[test]
    fn bad_flags_are_usage() {
        for args in [
            &["--arm"][..],
            &["--arm", "c"],
            &["--sync", "poll"],
            &["--control-port", "x"],
            &["--control-port", "70000"],
            &["--control-port", "0"],
            &["--pages"],
            &["--pages", ""],
            &["--window"],
            &["show"],
            &["--arm", "a", "--arm", "b"],
            &["--sync", "get", "--sync", "get"],
        ] {
            let r = parse(args);
            assert!(matches!(r, Err(Refusal::Usage(_))), "{args:?} gave {r:?}");
            assert_eq!(r.unwrap_err().cause(), "usage");
        }
    }

    #[test]
    fn hosted_needs_ci_or_guest() {
        let set = Some(OsStr::new("true"));
        let empty = Some(OsStr::new(""));
        assert_eq!(check_hosted(None, None), Err(Refusal::NotHosted));
        assert_eq!(check_hosted(empty, empty), Err(Refusal::NotHosted));
        assert_eq!(check_hosted(set, None), Ok(()));
        assert_eq!(check_hosted(None, set), Ok(()));
        assert_eq!(Refusal::NotHosted.cause(), "not_hosted");
    }

    #[test]
    fn refusal_line_names_the_cause() {
        assert_eq!(Refusal::NotHosted.to_string(), "refused cause=not_hosted");
        let usage = Refusal::Usage("unknown flag".into()).to_string();
        assert!(usage.starts_with("refused cause=usage "), "{usage}");
    }
}
