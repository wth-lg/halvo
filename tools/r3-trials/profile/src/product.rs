//! The start of the product's browser as the profile trial runs it (A13.3, A11.1; U0.7c's flag
//! set), the `flag` and `pref` lines of the result file, and the reading of the egress check's
//! line for item 3.

use std::path::PathBuf;

use r3_web::cdp::{Channel, StartOptions};
use r3_web::relay::{relay_flags, RelayHandle};
use r3_web::{same_site_local_state, AGENT_PREFERENCES, FIRST_FLAGS, RESOLVER_RULE, ROUND2_FLAGS};

/// The fixture's port of item 3: the one `[agent]` entry of `EGRESS_LIST` (A14.3).
pub const EGRESS_PORT: u16 = 18090;
/// The list file of A14.3 under which item 3 runs, relative to the repository's root.
pub const EGRESS_LIST: &str = "scripts/r3/egress/fixtures.txt";

/// Which start a run of the trial makes.
#[derive(Clone)]
pub enum Start {
    /// The product's start (A11.1): on the debugging pipe, with the agent's profile settings and
    /// the same-site setting written before the start, the first flag set of A13.2, the flags of
    /// U0.7b for the ways out, and the relay of `relay` as the browser's only route.
    Product { relay: RelayHandle, relay_port: u16 },
    /// The product's start without the relay (item 3): the relay's flags that close a way out stay.
    ProductDirect,
    /// The first flag set of A13.2 on the pipe, with no setting written: where item 3 begins.
    First,
}

impl Start {
    /// Whether the start writes the agent's settings into the new profile (`start_settled`).
    pub fn settled(&self) -> bool {
        !matches!(self, Start::First)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Start::Product { .. } => "product",
            Start::ProductDirect => "product_direct",
            Start::First => "first",
        }
    }
}

fn set_by_the_executor(flag: &str) -> bool {
    [
        "--proxy-server=",
        "--proxy-bypass-list=",
        "--host-resolver-rules=",
    ]
    .iter()
    .any(|p| flag.starts_with(p))
}

pub fn product_flags() -> Vec<String> {
    let mut f: Vec<String> = FIRST_FLAGS.iter().map(|s| s.to_string()).collect();
    f.extend(ROUND2_FLAGS.iter().map(|s| s.to_string()));
    f.extend(
        relay_flags(0)
            .into_iter()
            .filter(|f| !set_by_the_executor(f)),
    );
    f
}

/// The start options of one browser of the trial in `start`'s form, with its profile at
/// `profile`, for the fixture on `port`.
pub fn options(browser: PathBuf, profile: PathBuf, port: u16, start: &Start) -> StartOptions {
    let mut flags: Vec<String> = FIRST_FLAGS.iter().map(|s| s.to_string()).collect();
    flags.push(RESOLVER_RULE.to_string());
    let mut relay = None;
    match start {
        Start::Product {
            relay: r,
            relay_port,
        } => {
            flags.extend(ROUND2_FLAGS.iter().map(|s| s.to_string()));
            flags.extend(relay_flags(*relay_port));
            relay = Some(r.clone());
        }
        Start::ProductDirect => {
            flags.extend(ROUND2_FLAGS.iter().map(|s| s.to_string()));
            flags.extend(
                relay_flags(0)
                    .into_iter()
                    .filter(|f| !set_by_the_executor(f)),
            );
        }
        Start::First => {}
    }
    StartOptions {
        browser,
        profile,
        headless: true,
        flags,
        channel: Channel::Pipe,
        relay,
        local_state: start.settled().then(|| same_site_local_state(port)),
        program: None,
    }
}

fn leaves(prefix: &str, v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, child) in map {
                let name = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                leaves(&name, child, out);
            }
        }
        serde_json::Value::String(s) => out.push(format!("pref {prefix}={s}")),
        other => out.push(format!("pref {prefix}={other}")),
    }
}

pub fn pref_lines(port: u16) -> Vec<String> {
    let mut out = Vec::new();
    for text in [same_site_local_state(port), AGENT_PREFERENCES.to_string()] {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            leaves("", &v, &mut out);
        }
    }
    out
}

/// The `flag` and `pref` lines of the result file, in that order.
pub fn set_lines(port: u16) -> Vec<String> {
    let mut lines: Vec<String> = product_flags()
        .into_iter()
        .map(|f| format!("flag {f}"))
        .collect();
    lines.extend(pref_lines(port));
    lines
}

/// The cause words of the egress check's refusal lines (`REFUSED <cause>: <detail>`), joined by
/// commas, or `-`; the detail, which can name a path, is left out.
pub fn refusal_causes(check_stderr: &str) -> String {
    let causes: Vec<&str> = check_stderr
        .lines()
        .filter_map(|l| l.strip_prefix("REFUSED "))
        .map(|rest| rest.split(':').next().unwrap_or("-"))
        .collect();
    if causes.is_empty() {
        "-".to_string()
    } else {
        causes.join(",")
    }
}

/// The `egress` line of item 3 from the egress check's standard error (its last line that starts
/// `egress mode=`) and whether the browser started: `egress machine=<m> mode=<mode>
/// browser_started=<yes|no> denied=<n>`. None when the check printed no such line.
pub fn egress_line(machine: &str, check_stderr: &str, browser_started: bool) -> Option<String> {
    let line = check_stderr
        .lines()
        .rev()
        .find(|l| l.starts_with("egress mode="))?;
    let field = |key: &str| {
        line.split(' ')
            .find_map(|w| w.strip_prefix(key))
            .map(str::to_string)
    };
    let mode = field("mode=")?;
    let denied = field("denied=")?;
    if denied.is_empty() || !denied.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "egress machine={machine} mode={mode} browser_started={} denied={denied}",
        if browser_started { "yes" } else { "no" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(start: &Start) -> StartOptions {
        options(
            PathBuf::from("browser"),
            PathBuf::from("profile"),
            18090,
            start,
        )
    }

    #[test]
    fn the_product_start_is_on_the_pipe_with_the_relay_and_every_flag_of_the_set() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let relay = rt.block_on(r3_web::relay::Relay::start()).expect("relay");
        let o = opts(&Start::Product {
            relay: relay.handle(),
            relay_port: relay.port,
        });
        assert_eq!(o.channel, Channel::Pipe);
        assert!(o.relay.is_some());
        for f in product_flags() {
            assert!(o.flags.contains(&f), "{f}");
        }
        assert!(o
            .flags
            .contains(&format!("--proxy-server=socks5://127.0.0.1:{}", relay.port)));
        assert_eq!(
            o.flags.last().map(String::as_str),
            Some(r3_web::relay::RELAY_RESOLVER_RULE)
        );
        assert_eq!(o.local_state, Some(same_site_local_state(18090)));
    }

    #[test]
    fn the_direct_start_of_item_3_carries_the_set_and_no_route() {
        let o = opts(&Start::ProductDirect);
        assert_eq!(o.channel, Channel::Pipe);
        assert!(o.relay.is_none());
        for f in product_flags() {
            assert!(o.flags.contains(&f), "{f}");
        }
        assert!(!o.flags.iter().any(|f| f.starts_with("--proxy-")));
        assert_eq!(
            o.flags
                .iter()
                .filter(|f| f.starts_with("--host-resolver-rules="))
                .count(),
            1
        );
        assert_eq!(o.local_state, Some(same_site_local_state(18090)));
        assert!(Start::ProductDirect.settled());
    }

    #[test]
    fn the_first_start_is_the_first_set_with_the_resolver_rule_alone() {
        let o = opts(&Start::First);
        let mut want: Vec<String> = FIRST_FLAGS.iter().map(|s| s.to_string()).collect();
        want.push(RESOLVER_RULE.to_string());
        assert_eq!(o.flags, want);
        assert_eq!(o.local_state, None);
        assert!(!Start::First.settled());
        assert_eq!(o.channel, Channel::Pipe);
    }

    #[test]
    fn the_flag_lines_hold_the_sets_and_none_that_the_executor_sets() {
        let flags = product_flags();
        assert_eq!(flags.len(), FIRST_FLAGS.len() + ROUND2_FLAGS.len() + 2);
        assert!(flags.contains(&"--disable-quic".to_string()));
        assert!(flags
            .contains(&"--force-webrtc-ip-handling-policy=disable_non_proxied_udp".to_string()));
        for f in &flags {
            assert!(!set_by_the_executor(f), "{f}");
            assert!(!f.contains("remote-debugging"), "{f}");
            assert!(!f.contains("user-data-dir"), "{f}");
            assert!(!f.contains("headless"), "{f}");
            assert!(!f.contains("mock-keychain"), "{f}");
        }
    }

    #[test]
    fn the_pref_lines_name_each_setting_of_the_start() {
        let lines = pref_lines(18090);
        assert!(lines.contains(&"pref dns_over_https.mode=off".to_string()));
        assert!(lines
            .contains(&"pref browser.enabled_labs_experiments=[\"isolate-origins\"]".to_string()));
        assert!(lines.contains(
            &"pref browser.enabled_labs_experiments_origin_lists.isolate-origins=http://pay.agent.test:18090"
                .to_string()
        ));
        assert!(lines
            .contains(&"pref profile.default_content_setting_values.notifications=2".to_string()));
        assert!(
            lines.contains(&"pref webrtc.ip_handling_policy=disable_non_proxied_udp".to_string())
        );
        assert!(lines.contains(&"pref credentials_enable_service=false".to_string()));
        assert!(lines
            .iter()
            .all(|l| l.starts_with("pref ") && l.matches(' ').count() == 1));
    }

    #[test]
    fn the_egress_line_comes_from_the_checks_last_line() {
        let err = "noise\negress mode=sandbox list=x command=1 denied=3 log=target/egress/1/denied.log\negress mode=wfp list=y command=0 denied=0 log=target/egress/2/denied.log\n";
        assert_eq!(
            egress_line("hosted-win", err, true).as_deref(),
            Some("egress machine=hosted-win mode=wfp browser_started=yes denied=0")
        );
        assert_eq!(
            egress_line(
                "mac",
                "egress mode=sandbox list=x command=1 denied=12 log=l\n",
                false
            )
            .as_deref(),
            Some("egress machine=mac mode=sandbox browser_started=no denied=12")
        );
        assert_eq!(egress_line("mac", "REFUSED mode: x\n", true), None);
        assert_eq!(
            egress_line("mac", "egress mode=sandbox denied= log=l\n", true),
            None
        );
        assert_eq!(
            egress_line("mac", "egress mode=sandbox denied=1x log=l\n", true),
            None
        );
    }

    #[test]
    fn the_list_is_checked_out_with_lf_line_ends() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let attributes = std::fs::read_to_string(root.join(".gitattributes")).expect("attributes");
        assert!(attributes
            .lines()
            .any(|l| l == format!("/{EGRESS_LIST} text eol=lf")));
        let list = std::fs::read(root.join(EGRESS_LIST)).expect("the list file");
        assert!(!list.contains(&b'\r'));
    }

    #[test]
    fn the_refusal_cause_is_read_without_its_detail() {
        assert_eq!(
            refusal_causes("REFUSED list: C:/x/fixtures.txt line 1\negress x\n"),
            "list"
        );
        assert_eq!(refusal_causes("egress mode=wfp denied=0\n"), "-");
    }

    #[test]
    fn the_result_files_last_block_holds_the_set_as_the_program_prints_it() {
        let file = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../docs/r3/trials/browser-profile.md"),
        )
        .expect("the result file");
        let start = file.rfind("```text\n").expect("a block") + "```text\n".len();
        let block = &file[start..];
        let block = &block[..block.find("```").expect("the block's end")];
        let lines: Vec<&str> = block
            .lines()
            .filter(|l| l.starts_with("flag ") || l.starts_with("pref "))
            .collect();
        let want = set_lines(EGRESS_PORT);
        assert_eq!(lines, want.iter().map(String::as_str).collect::<Vec<_>>());
    }

    #[test]
    fn the_egress_port_is_the_agent_entry_of_the_list() {
        let list = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .join(EGRESS_LIST),
        )
        .expect("the list file");
        let entries: Vec<&str> = list
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        assert_eq!(
            entries,
            [
                "[fixed]",
                "[agent]",
                &format!("127.0.0.1:{EGRESS_PORT}")[..]
            ]
        );
    }
}
