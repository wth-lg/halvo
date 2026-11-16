use r4_trial_app::{default_pages_dir, Arm, Refusal};

use crate::card::Form;

#[cfg(all(feature = "webdriver", feature = "wdio"))]
compile_error!("build the trial with one candidate: the feature webdriver or the feature wdio");

/// The candidate whose WebDriver server this build links, by the cargo feature it was built
/// with, or `None`.
pub const CANDIDATE: Option<&str> = if cfg!(feature = "webdriver") {
    Some("webdriver")
} else if cfg!(feature = "wdio") {
    Some("wdio")
} else {
    None
};

/// The most shows of one dialog run.
pub const MAX_SHOWS: u32 = 9;

/// The flags of the trial binary: the control port of the trial app, the port of the
/// candidate's server (required in a build with a candidate, refused in one without), and the
/// dialog form with how many times it is shown one after another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub control_port: Option<u16>,
    pub webdriver_port: Option<u16>,
    pub dialog: Option<Form>,
    pub shows: u32,
}

impl Options {
    /// The trial app's options: arm `main`, no sync way, no tray item, its own pages.
    pub fn trial(&self) -> r4_trial_app::Options {
        r4_trial_app::Options {
            control_port: self.control_port,
            pages: default_pages_dir(),
            arm: Arm::Main,
            sync: None,
            tray: false,
        }
    }
}

fn port(value: &str, flag: &str) -> Result<u16, Refusal> {
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| Refusal::Usage(format!("{flag} takes a port from 1 to 65535")))
}

/// Reads `--control-port N`, `--webdriver-port N`, `--dialog plugin|system` and `--shows N`
/// (1 to `MAX_SHOWS`, with `--dialog` alone); anything else, a missing value or a flag given
/// twice is `Refusal::Usage`.
pub fn parse_args<I, S>(args: I) -> Result<Options, Refusal>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let usage = |why: String| Refusal::Usage(why);
    let mut options = Options {
        control_port: None,
        webdriver_port: None,
        dialog: None,
        shows: 1,
    };
    let mut seen: Vec<String> = Vec::new();
    let mut args = args.into_iter().map(Into::into);
    while let Some(flag) = args.next() {
        if !matches!(
            flag.as_str(),
            "--control-port" | "--webdriver-port" | "--dialog" | "--shows"
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
            "--control-port" => options.control_port = Some(port(&value, &flag)?),
            "--webdriver-port" => options.webdriver_port = Some(port(&value, &flag)?),
            "--dialog" => {
                options.dialog = Some(
                    Form::parse(&value)
                        .ok_or_else(|| usage("--dialog takes plugin or system".into()))?,
                );
            }
            _ => {
                options.shows = value
                    .parse::<u32>()
                    .ok()
                    .filter(|n| (1..=MAX_SHOWS).contains(n))
                    .ok_or_else(|| usage(format!("--shows takes 1 to {MAX_SHOWS}")))?;
            }
        }
        seen.push(flag);
    }
    if seen.iter().any(|f| f == "--shows") && options.dialog.is_none() {
        return Err(usage("--shows goes with --dialog".into()));
    }
    match (CANDIDATE, options.webdriver_port) {
        (Some(name), None) => Err(usage(format!(
            "this build links the {name} server and needs --webdriver-port"
        ))),
        (None, Some(_)) => Err(usage(
            "this build links no WebDriver server; build it with a candidate feature".into(),
        )),
        _ => Ok(options),
    }
}
