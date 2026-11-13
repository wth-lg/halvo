//! The web executor of A11.1: the agent's browser driven over a pipe by the one process that
//! started it, through a relay that reaches only the person's bound sites.

pub mod cdp;
pub mod checks;
#[cfg(test)]
mod child;
pub mod guard;
pub mod peer;
pub mod pipe;
pub mod procs;
pub mod relay;
#[cfg(all(test, target_os = "macos"))]
mod standin;
pub mod start;

/// The start flags of the product's browser, before the relay's flags (`relay::relay_flags`): the
/// first flag set of A13.2, which turns off the browser's own connections, and the flags that
/// U0.7b added, each closing a way out of A13.7 (A11.1, A13.3).
pub const PRODUCT_FLAGS: [&str; 17] = [
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
    "--disable-updater-scheduler",
    "--dns-prefetch-disable",
    "--no-pings",
    "--disable-extensions",
    "--disable-component-extensions-with-background-pages",
    "--disable-features=FetchLaterAPI,MediaRouter,DialMediaRouteProvider,CastMediaRouteProvider,SignedHTTPExchange,SignedExchangeSubresourcePrefetch,Prerender2,PrefetchProxy,NetworkErrorLogging,Reporting,DocumentReporting,FedCm,PaymentHandler,WebAuthenticationCable,AttributionReporting,PrivacySandboxAdsAPIs,InterestFeedContentSuggestions,OptimizationHints,Translate,WebRtcHideLocalIpsWithMdns",
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
