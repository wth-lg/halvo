#[cfg(feature = "driver")]
pub mod cdp;
pub mod checks;
pub mod fixture;
pub mod killwatch;
pub mod peer;
#[cfg(feature = "driver")]
pub mod pipe;
pub mod policy;
pub mod procs;
#[cfg(feature = "driver")]
pub mod relay;
#[cfg(all(test, target_os = "macos", feature = "driver"))]
mod standin;
pub mod start;
pub mod tls;

pub const CANDIDATE: &str = "cdp";

pub const STEPS: [&str; 9] = [
    "signin_form",
    "table_read",
    "form_fill",
    "payment_frame",
    "file_upload",
    "session_expiry",
    "changed_page_1",
    "block_request",
    "credential_mark",
];

/// The six rows of A13.7, which `--rows isolation` runs in place of the nine steps.
pub const ISOLATION_STEPS: [&str; 6] = [
    "payment_frame_isolated",
    "changed_page_1_isolated",
    "block_popup",
    "block_frame_post",
    "block_websocket",
    "cert_error",
];

/// The row of rulings CP-22 and CP-24 in its four forms, which `--rows driver` runs: the driver is
/// killed with the system's hard kill while a state-changing request is paused, over the pipe or
/// the port, and without or with the relay in the driver's own process.
pub const DRIVER_STEPS: [&str; 4] = [
    "driver_killed_pipe",
    "driver_killed_port",
    "driver_killed_pipe_relay",
    "driver_killed_port_relay",
];

/// The rows of the relay's list of U0.7b (A13.7), which `--rows relay --round 2` runs: three that
/// decide and `own_traffic`, which is recorded.
pub const RELAY_STEPS: [&str; 4] = ["list_refuse", "list_hops", "list_close", "own_traffic"];

/// The recorded rows of U0.7b (A13.7) that `--rows ways --round 2` runs: `ws_allowed` with its
/// control, which shows that the row can fail, and `frame_doc` on the port with the relay.
pub const WAY_STEPS: [&str; 3] = ["ws_allowed", "ws_allowed_control", "frame_doc"];

/// The rows of CP-78 (U0.7b), which decide: through the relay, a read to a host outside the
/// rule's list from each kind of context (a page, a page opened after the rule was armed, a frame,
/// a dedicated worker, a shared worker, a service worker); ok when no byte reaches that host.
pub const REACH_STEPS: [&str; 6] = [
    "reach_page",
    "reach_opened",
    "reach_frame",
    "reach_worker",
    "reach_shared",
    "reach_service",
];

/// The rows of U0.7b (A13.7) that run with the browser as the command of the repository's egress
/// check (`--rows egress --round 2`), with a list that holds the relay's port alone.
pub const EGRESS_STEPS: [&str; 11] = [
    "no_worker_paths",
    "udp_quic",
    "webtransport",
    "peer_connection",
    "secure_dns",
    "lan_discovery",
    "link_lookup",
    "after_unload",
    "reports",
    "handler_launch",
    "updater_wake",
];

/// The rows of U0.7c (A13.7) that `--rows round3 --round 3` runs, one for each rule of A11.1's
/// start, check and relay that a run of the trial program can show, and the two rows of a payment
/// frame of the same site as its page (CP-95 (2)).
pub const ROUND3_STEPS: [&str; 5] = [
    "pipe_inherit",
    "relay_foreign",
    "start_probe",
    "same_site_pref",
    "same_site_rule",
];

/// The row of U0.7c in which the driver is killed and the browser must be gone before the
/// relay's port is free (`--rows driver --round 3`).
pub const PORT_FREE_STEP: &str = "relay_port_free";

/// The two cells of `org_policy` (U0.7c): the caller sets the organisation's setting before the
/// run and takes it away after it (`--rows org --round 3`).
pub const ORG_STEPS: [&str; 2] = ["org_debugging", "org_proxy"];

/// The `Local State` entry of the profile that isolates the origin `http://pay.agent.test:<port>`
/// without a flag on the command line (row `same_site_pref`): the flags entry `isolate-origins`
/// in `browser.enabled_labs_experiments` with its origin list in
/// `browser.enabled_labs_experiments_origin_lists`, the names that Chrome 153's framework holds,
/// written before the start.
pub fn same_site_local_state(port: u16) -> String {
    serde_json::json!({
        "dns_over_https": {"mode": "off", "templates": ""},
        "browser": {
            "enabled_labs_experiments": ["isolate-origins"],
            "enabled_labs_experiments_origin_lists": {
                "isolate-origins": format!("http://pay.agent.test:{port}")
            }
        }
    })
    .to_string()
}

/// The flag by which the trial program, started in the browser's place, knows that it is the
/// browser's launcher under the egress check; its value is the hex of a JSON array.
pub const EGRESS_WRAP_FLAG: &str = "--r3-egress-wrap=";

/// The policy of A4.2 that the executor adds to every document and worker script (CP-67).
pub const WS_POLICY: &str = "connect-src http: https: data: blob:";

/// The one start flag that the rows of A13.7 add: it puts the payment frame of the fixture on
/// `port` into a process of its own.
pub fn isolate_flag(port: u16) -> String {
    format!("--isolate-origins=http://pay.agent.test:{port}")
}

pub const FIRST_FLAGS: [&str; 11] = [
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-sync",
    "--disable-field-trial-config",
    "--disable-client-side-phishing-detection",
    "--disable-default-apps",
    "--disable-breakpad",
    "--disable-domain-reliability",
    "--metrics-recording-only",
    "--no-first-run",
    "--no-default-browser-check",
];

pub const RESOLVER_RULE: &str = "--host-resolver-rules=MAP *.test 127.0.0.1,MAP * ~NOTFOUND";

/// The start flags that U0.7b adds to the first flag set, the resolver rule and the relay's flags,
/// each to close a way out of A13.7: the browser's waking of its updater, which then looks names up
/// itself, lookups for links, hyperlink pings, the browser's extensions,
/// and the features of fetches after a document is gone, discovery on the local network, signed
/// exchanges, prerendering, prefetch through a proxy, reports, federated sign-in, payment handlers,
/// a passkey on another device and the measurement interfaces.
pub const ROUND2_FLAGS: [&str; 6] = [
    "--disable-updater-scheduler",
    "--dns-prefetch-disable",
    "--no-pings",
    "--disable-extensions",
    "--disable-component-extensions-with-background-pages",
    "--disable-features=FetchLaterAPI,MediaRouter,DialMediaRouteProvider,CastMediaRouteProvider,SignedHTTPExchange,SignedExchangeSubresourcePrefetch,Prerender2,PrefetchProxy,NetworkErrorLogging,Reporting,DocumentReporting,FedCm,PaymentHandler,WebAuthenticationCable,AttributionReporting,PrivacySandboxAdsAPIs,InterestFeedContentSuggestions,OptimizationHints,Translate,WebRtcHideLocalIpsWithMdns",
];

/// The `Local State` of the agent's new profile: secure lookups off.
pub const AGENT_LOCAL_STATE: &str = r#"{"dns_over_https":{"mode":"off","templates":""}}"#;

/// The `Default/Preferences` of the agent's new profile: every permission that a page can ask for
/// denied, prediction and prefetch off, no saved password or payment method, safe browsing,
/// suggestions and spell checking off.
pub const AGENT_PREFERENCES: &str = r#"{"profile":{"default_content_setting_values":{"notifications":2,"geolocation":2,"media_stream_camera":2,"media_stream_mic":2,"clipboard":2,"midi_sysex":2,"usb_guard":2,"serial_guard":2,"hid_guard":2,"bluetooth_guard":2,"bluetooth_scanning":2,"file_system_write_guard":2,"file_system_read_guard":2,"automatic_downloads":2,"sensors":2,"idle_detection":2,"window_placement":2,"local_fonts":2,"payment_handler":2,"protocol_handlers":2,"ar":2,"vr":2,"nfc":2,"storage_access":2},"password_manager_enabled":false},"credentials_enable_service":false,"autofill":{"profile_enabled":false,"credit_card_enabled":false},"payments":{"can_make_payment_enabled":false},"net":{"network_prediction_options":2},"safebrowsing":{"enabled":false},"search":{"suggest_enabled":false},"browser":{"enable_spellchecking":false},"dns_prefetching":{"enabled":false},"webrtc":{"ip_handling_policy":"disable_non_proxied_udp","multiple_routes_enabled":false,"nonproxied_udp_enabled":false},"web_package":{"signed_exchange":{"enabled":false}}}"#;

/// The key ` majors=<v>:<n>,...` of a cell line whose runs reported more than one major version
/// (CP-73), from the sorted majors of its runs; empty when they reported one.
/// The `kill` line of a form of `driver_killed` (A13.7): the machine, the browser, the form, the
/// kills and how many of them let a byte of the paused request arrive.
pub fn kill_line(machine: &str, browser: &str, form: &str, kills: u32, arrived: u32) -> String {
    format!("kill machine={machine} browser={browser} form={form} kills={kills} arrived={arrived}")
}

pub fn majors_key(sorted: &[u32]) -> String {
    let mut per: Vec<(u32, usize)> = Vec::new();
    for m in sorted {
        match per.last_mut() {
            Some((v, n)) if v == m => *n += 1,
            _ => per.push((*m, 1)),
        }
    }
    if per.len() < 2 {
        return String::new();
    }
    let parts: Vec<String> = per.iter().map(|(v, n)| format!("{v}:{n}")).collect();
    format!(" majors={}", parts.join(","))
}

pub fn variants_of(step: &str) -> &'static [&'static str] {
    match step {
        "session_expiry" => &["session_expiry"],
        "changed_page_1" | "changed_page_1_isolated" => &["changed_page_1"],
        "credential_mark" => &["second_factor", "captcha"],
        _ => &[],
    }
}

pub const BROWSER_NAMES: [(&str, &str); 7] = [
    ("Google Chrome", "chrome"),
    ("Google Chrome for Testing", "chrome"),
    ("chrome.exe", "chrome"),
    ("chrome", "chrome"),
    ("Microsoft Edge", "edge"),
    ("msedge.exe", "edge"),
    ("msedge", "edge"),
];

pub fn browser_kind(path: &str) -> Option<&'static str> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    BROWSER_NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, kind)| *kind)
}

pub fn major_of(product: &str) -> Option<u32> {
    product.rsplit('/').next()?.split('.').next()?.parse().ok()
}

pub fn median(values: &mut [u128]) -> u128 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2
    }
}

pub fn extract_amount(text: &str) -> Option<u64> {
    let (whole, cents) = text.split_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || cents.len() != 2 || !digits(cents) {
        return None;
    }
    let whole: u64 = whole.parse().ok()?;
    let cents: u64 = cents.parse().ok()?;
    whole.checked_mul(100)?.checked_add(cents)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_kill_line_names_its_browser() {
        assert_eq!(
            kill_line("hosted-win", "edge", "pipe_relay", 20, 0),
            "kill machine=hosted-win browser=edge form=pipe_relay kills=20 arrived=0"
        );
        assert_eq!(
            kill_line("mac", "chrome", "port", 20, 20),
            "kill machine=mac browser=chrome form=port kills=20 arrived=20"
        );
    }

    use super::*;

    #[test]
    fn extract_amount_takes_two_decimals_only() {
        assert_eq!(extract_amount("84.37"), Some(8437));
        assert_eq!(extract_amount("0.05"), Some(5));
        for bad in [
            "84.3", "84.370", "84", ".37", "8a.37", "-1.00", "84.37 ", "1,084.37", "",
        ] {
            assert_eq!(extract_amount(bad), None, "{bad}");
        }
    }

    #[test]
    fn browser_kind_reads_the_file_name() {
        assert_eq!(
            browser_kind("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            Some("chrome")
        );
        assert_eq!(
            browser_kind("C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe"),
            Some("edge")
        );
        assert_eq!(browser_kind("/usr/bin/firefox"), None);
        assert_eq!(browser_kind("/x/google chrome"), None);
        assert_eq!(browser_kind("C:\\x\\MSEDGE.EXE"), None);
    }

    #[test]
    fn run_sh_takes_the_browser_names_of_browser_kind() {
        let script = include_str!("../run.sh");
        let line = script
            .lines()
            .skip_while(|l| !l.contains(r#"case "$(basename "$browser")" in"#))
            .nth(1)
            .unwrap_or_default();
        let mut taken: Vec<String> = line
            .trim()
            .trim_end_matches(";;")
            .trim()
            .trim_end_matches(')')
            .split('|')
            .map(|n| n.trim().trim_matches('\'').to_string())
            .collect();
        taken.sort();
        let mut known: Vec<String> = BROWSER_NAMES.iter().map(|(n, _)| n.to_string()).collect();
        known.sort();
        assert_eq!(taken, known);
    }

    #[test]
    fn major_of_reads_the_product() {
        assert_eq!(major_of("HeadlessChrome/158.0.8127.18"), Some(158));
        assert_eq!(major_of("Chrome/x"), None);
    }

    #[test]
    fn median_of_even_and_odd() {
        assert_eq!(median(&mut [5, 1, 3]), 3);
        assert_eq!(median(&mut [4, 1, 3, 2]), 2);
        assert_eq!(median(&mut []), 0);
    }

    #[test]
    fn every_step_has_its_variants() {
        assert_eq!(
            variants_of("credential_mark"),
            &["second_factor", "captcha"]
        );
        assert!(variants_of("signin_form").is_empty());
        assert_eq!(STEPS.len(), 9);
    }

    #[test]
    fn the_isolated_rows_take_the_variants_of_their_rows_of_a13_2() {
        assert_eq!(variants_of("changed_page_1_isolated"), &["changed_page_1"]);
        for row in ISOLATION_STEPS
            .iter()
            .filter(|r| **r != "changed_page_1_isolated")
        {
            assert!(variants_of(row).is_empty(), "{row}");
        }
        assert!(ISOLATION_STEPS.iter().all(|r| !STEPS.contains(r)));
    }

    #[test]
    fn run_sh_adds_the_isolate_flag_and_takes_the_six_rows() {
        assert_eq!(
            isolate_flag(18190),
            "--isolate-origins=http://pay.agent.test:18190"
        );
        let script = include_str!("../run.sh");
        assert!(script.contains(r#"--add-flag "--isolate-origins=http://pay.agent.test:$port""#));
        let line = script
            .lines()
            .find(|l| l.trim_start().starts_with("payment_frame_isolated |"))
            .unwrap_or_default();
        let mut taken: Vec<&str> = line
            .trim()
            .trim_end_matches(";;")
            .trim()
            .trim_end_matches(')')
            .split('|')
            .map(|n| n.trim())
            .collect();
        taken.sort_unstable();
        let mut rows = ISOLATION_STEPS.to_vec();
        rows.sort_unstable();
        assert_eq!(taken, rows);
    }

    #[test]
    fn the_agent_settings_are_json_with_secure_lookups_off_and_permissions_denied() {
        let local: serde_json::Value = serde_json::from_str(AGENT_LOCAL_STATE).expect("json");
        assert_eq!(local["dns_over_https"]["mode"], "off");
        assert_eq!(majors_key(&[156, 157, 157]), " majors=156:1,157:2");
        assert_eq!(majors_key(&[157, 157]), "");
        assert_eq!(majors_key(&[]), "");
        let prefs: serde_json::Value = serde_json::from_str(AGENT_PREFERENCES).expect("json");
        let settings = &prefs["profile"]["default_content_setting_values"];
        for key in [
            "notifications",
            "clipboard",
            "geolocation",
            "file_system_write_guard",
        ] {
            assert_eq!(settings[key], 2, "{key}");
        }
        assert_eq!(prefs["net"]["network_prediction_options"], 2);
        assert_eq!(prefs["credentials_enable_service"], false);
        assert!(ROUND2_FLAGS.iter().any(|f| f.contains("FetchLaterAPI")));
        assert!(ROUND2_FLAGS.contains(&"--disable-updater-scheduler"));
    }

    #[test]
    fn the_same_site_setting_names_the_flags_entry_and_its_origin_list() {
        let v: serde_json::Value =
            serde_json::from_str(&same_site_local_state(18190)).expect("json");
        assert_eq!(
            v["browser"]["enabled_labs_experiments"][0],
            "isolate-origins"
        );
        assert_eq!(
            v["browser"]["enabled_labs_experiments_origin_lists"]["isolate-origins"],
            "http://pay.agent.test:18190"
        );
        assert_eq!(v["dns_over_https"]["mode"], "off");
    }

    #[test]
    fn no_flag_is_the_resolver_rule() {
        assert!(FIRST_FLAGS
            .iter()
            .all(|f| !f.contains("host-resolver-rules")));
    }
}
