//! The web executor of A11.1: the agent's browser driven over a pipe by the one process that
//! started it, through a relay that reaches only the person's bound sites.

pub mod cdp;
pub mod checks;
#[cfg(test)]
mod child;
pub mod crash_row;
pub mod guard;
pub mod peer;
pub mod pipe;
pub mod procs;
pub mod relay;
pub mod rows;
#[cfg(all(test, target_os = "macos"))]
mod standin;
pub mod start;

/// The start flags of the product's browser, before the relay's flags (`relay::relay_flags`), each
/// the executor's own (A11.1, A13.3), with its reason:
///
/// - `--disable-background-networking`, `--disable-component-update`, `--disable-sync`,
///   `--disable-field-trial-config`, `--disable-client-side-phishing-detection`,
///   `--disable-default-apps`, `--disable-domain-reliability`: the browser's own connections of
///   A13.2's first set (updates, sync, variations, safe browsing's model, default apps,
///   reliability reports) are off, so the relay sees the agent's traffic alone.
/// - `--metrics-recording-only`: metrics are kept and never sent.
/// - `--no-first-run`, `--no-default-browser-check`: no first-run page and no check of the default
///   browser, which would open pages or ask the person.
/// - `--disable-updater-scheduler`: the browser does not wake its updater, which would look names
///   up itself (U0.7b).
/// - `--dns-prefetch-disable`, `--no-pings`: no lookups for links and no hyperlink pings.
/// - `--disable-extensions`, `--disable-component-extensions-with-background-pages`: no extension
///   runs code or makes requests beside the page.
/// - `--disable-features=...`: fetches after a document is gone, discovery on the local network
///   and casting, signed exchanges, prerendering, prefetch through a proxy, network error and other
///   reports, federated sign-in, payment handlers, a passkey on another device, attribution and
///   the privacy sandbox's interfaces, feed suggestions, optimization hints, translation, and the
///   hiding of local addresses behind mDNS are off: each a way out of A13.7 that U0.7b closed.
pub const PRODUCT_FLAGS: [&str; 16] = [
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-sync",
    "--disable-field-trial-config",
    "--disable-client-side-phishing-detection",
    "--disable-default-apps",
    "--disable-domain-reliability",
    "--metrics-recording-only",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-updater-scheduler",
    "--dns-prefetch-disable",
    "--no-pings",
    "--disable-extensions",
    "--disable-component-extensions-with-background-pages",
    "--disable-features=FetchLaterAPI,MediaRouter,DialMediaRouteProvider,CastMediaRouteProvider,SignedHTTPExchange,SignedExchangeSubresourcePrefetch,Prerender2,PrefetchProxy,NetworkErrorLogging,Reporting,DocumentReporting,FedCm,PaymentHandler,WebAuthenticationCable,AttributionReporting,PrivacySandboxAdsAPIs,InterestFeedContentSuggestions,OptimizationHints,Translate,WebRtcHideLocalIpsWithMdns",
];

/// The product's means against a crash's dump (CP-165 (2), CP-167 E2), flags of the executor's
/// own after `PRODUCT_FLAGS`, `{profile}` standing for the run's profile directory:
///
/// - `--disable-breakpad`: the browser's crash reporting off, A13.2's first set.
/// - `--breakpad-dump-location={profile}/Crashpad`: the crash handler's database inside the run's
///   profile, so that a dump, and the database's own files, stand in the profile, which the stop
///   removes, and never in the crash place of the account's own browser. The switch's name stands
///   in the browser's own binary (Google Chrome Framework of Chrome 153 on the development Mac);
///   run 43452224392 showed it on hosted macOS with Chrome 157 and Edge 156, where the crash with
///   the product's earlier means alone wrote its dump to the account's Crashpad directory.
///
/// The crash row's control starts without them (`cdp::CrashMeans::Without`).
pub const CRASH_MEANS: [&str; 2] = [
    "--disable-breakpad",
    "--breakpad-dump-location={profile}/Crashpad",
];

/// The further candidates of the crash row (CP-167 E2), a closed list, each added after the
/// product's means by `cdp::CrashMeans::Candidate`; each switch name stands in the browser's own
/// binary, and what each does is what the row records.
pub const CRASH_CANDIDATES: [(&str, &[&str]); 3] = [
    (
        "system_port",
        &["--reset-own-crash-exception-port-to-system-default"],
    ),
    ("dumps_dir", &["--crash-dumps-dir={profile}/Crashpad"]),
    ("no_reporter", &["--disable-crash-reporter"]),
];

/// The policy of A4.2 that the executor adds to every document and worker script.
pub const WS_POLICY: &str = "connect-src http: https: data: blob:";

/// The `Local State` of the agent's new profile: secure lookups off.
pub const AGENT_LOCAL_STATE: &str = r#"{"dns_over_https":{"mode":"off","templates":""}}"#;

/// The `Default/Preferences` of the agent's new profile: every permission that a page can ask for
/// denied, prediction and prefetch off, no saved password or payment method, safe browsing,
/// suggestions and spell checking off, peer connections only through the proxy.
pub const AGENT_PREFERENCES: &str = r#"{"profile":{"default_content_setting_values":{"notifications":2,"geolocation":2,"media_stream_camera":2,"media_stream_mic":2,"clipboard":2,"midi_sysex":2,"usb_guard":2,"serial_guard":2,"hid_guard":2,"bluetooth_guard":2,"bluetooth_scanning":2,"file_system_write_guard":2,"file_system_read_guard":2,"automatic_downloads":2,"sensors":2,"idle_detection":2,"window_placement":2,"local_fonts":2,"payment_handler":2,"protocol_handlers":2,"ar":2,"vr":2,"nfc":2,"storage_access":2},"password_manager_enabled":false},"credentials_enable_service":false,"autofill":{"profile_enabled":false,"credit_card_enabled":false},"payments":{"can_make_payment_enabled":false},"net":{"network_prediction_options":2},"safebrowsing":{"enabled":false},"search":{"suggest_enabled":false},"browser":{"enable_spellchecking":false},"dns_prefetching":{"enabled":false},"webrtc":{"ip_handling_policy":"disable_non_proxied_udp","multiple_routes_enabled":false,"nonproxied_udp_enabled":false},"web_package":{"signed_exchange":{"enabled":false}}}"#;

/// `extract_amount(text)` of A11.1: digits with exactly one `.` before exactly two decimals, in
/// minor units; `None` for any other text or an amount that does not fit.
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
