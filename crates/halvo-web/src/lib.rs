//! The web executor of A11.1: the agent's browser driven over a pipe by the one process that
//! started it, through a relay that reaches only the person's bound sites.
//!
//! A named limit, held by no row and no promise (CP-186 N9): what each start leaves outside the
//! run's profile on macOS. In every reading of the crash row in run 43484944398, the start with no
//! crash among them, a start of Chrome left four files of 4,196 bytes in all, two in the account's
//! cache of `com.google.Chrome.helper` and two links in the system's temporary directory, and a
//! start of Edge four files of 4,923 bytes; after a crash the singleton directory stays in the
//! system's temporary directory. The stop removes the profile and the temporary directory
//! that its start made and nothing else (A11.1), so nothing in this crate removes those files, and
//! no text of this run says who does. On Windows the same readings left none.

pub mod cdp;
pub mod checks;
#[cfg(test)]
mod child;
pub mod crash_row;
#[cfg(test)]
mod fence_manifest;
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
///   the product's earlier means alone wrote its dump to the account's Crashpad directory. No place
///   in the browser's source or documentation is named for it: the four cells of run 43484944398,
///   with their controls, stand in that place's stead for Chrome 156 and 157 and Edge 156 and for
///   no other version (`crash_row::LAST_READ`), and the crash row is run again when a browser's
///   major version changes: the row `crash_major` of web-rows reads the runner's major against
///   them (`crash_row::read_at`; CP-165 (2) (d), CP-186 N3).
///
/// The crash row's control starts without them (`cdp::CrashMeans::Without`).
pub const CRASH_MEANS: [&str; 2] = [
    "--disable-breakpad",
    "--breakpad-dump-location={profile}/Crashpad",
];

/// The further candidates of the crash row (CP-167 E2), a closed list, each added after the
/// product's means by `cdp::CrashMeans::Candidate`; each switch name stands in the browser's own
/// binary, and what each does is what the row records. Behind the fence: a build without the
/// feature `test-fence`, which is no test build, has no candidate (CP-186 N8).
#[cfg(any(test, feature = "test-fence"))]
pub const CRASH_CANDIDATES: [(&str, &[&str]); 3] = [
    (
        "system_port",
        &["--reset-own-crash-exception-port-to-system-default"],
    ),
    ("dumps_dir", &["--crash-dumps-dir={profile}/Crashpad"]),
    ("no_reporter", &["--disable-crash-reporter"]),
];

/// The fence's marker (CP-186 N8, in the form of the capture run's W3-8 (b) (3)): a byte string
/// that stands in a build with the feature `test-fence` and in no build without it, so that a
/// check can read a built file for it (`fence/shipped.mjs`). The feature stands behind the crash
/// row's two means (`cdp::CrashMeans::Without`, `cdp::CrashMeans::Candidate`), its forced crash
/// (`cdp::Executor::crash`), `relay::Names::Fixture`, `CRASH_CANDIDATES` and the programs
/// `web-probe`, `web-rows` and `web-standin`; the crate's dev-dependency on itself turns it on for
/// its own tests, and no other manifest names it. Every path that reaches one of those means
/// reads the marker, and each of the three programs reads it at its start, so that a built file
/// that links them holds it; `CRASH_CANDIDATES` is a constant and reads none: a program that
/// names it alone holds its flags and no marker.
#[cfg(feature = "test-fence")]
pub static FENCE_MARKER: [u8; 39] = *b"halvo-web:test-fence:this-build-has-it!";

/// Reads the fence's marker, so that a build that links the caller holds it; each program behind
/// the fence calls it at its start.
#[cfg(feature = "test-fence")]
pub fn fence_marker() {
    std::hint::black_box(&FENCE_MARKER);
}

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    const FENCE: &str = r#"#[cfg(any(test, feature = "test-fence"))]"#;

    /// CP-186 N8: the fence is on in this test target from its start: the crate's dev-dependency
    /// on itself gives its own tests the feature `test-fence`.
    #[test]
    fn the_fence_is_on_in_this_test_target() {
        assert!(
            std::hint::black_box(cfg!(feature = "test-fence")),
            "the fence is off in the library's test target"
        );
    }

    /// CP-186 N8: every test target of the crate other than the library's own (the programs of
    /// src/bin and the files of tests) holds the test `the_fence_is_on_in_this_test_target`, each
    /// program of src/bin needs the feature, and the manifest gives the crate's own tests the
    /// feature through its dev-dependency on itself, so that no test target loses the fence's
    /// means unseen.
    #[test]
    fn every_test_target_holds_the_test_that_its_fence_is_on() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut targets = Vec::new();
        for dir in ["src/bin", "tests"] {
            let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
                continue;
            };
            for entry in entries {
                let path = entry.expect("an entry").path();
                if path.extension().is_some_and(|e| e == "rs") {
                    targets.push(path);
                }
            }
        }
        assert!(targets.len() >= 2, "{targets:?}");
        let missing: Vec<&PathBuf> = targets
            .iter()
            .filter(|p| {
                !std::fs::read_to_string(p)
                    .unwrap_or_default()
                    .contains("fn the_fence_is_on_in_this_test_target()")
            })
            .collect();
        assert!(
            missing.is_empty(),
            "test targets without the test: {missing:?}"
        );
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("the manifest");
        assert!(
            manifest.contains(r#"halvo-web = { path = ".", features = ["test-fence"] }"#),
            "the manifest does not give the crate's own tests the feature test-fence"
        );
        let bins = manifest.matches("[[bin]]").count();
        let fenced = manifest
            .matches(r#"required-features = ["test-fence"]"#)
            .count();
        assert_eq!(
            (bins, fenced),
            (3, 3),
            "each program needs the feature test-fence"
        );
    }

    /// Whether the line that stands directly above the first line of `text` that begins with
    /// `item` (leading spaces aside) is the fence's attribute.
    fn fenced(text: &str, item: &str) -> bool {
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        lines
            .iter()
            .position(|l| l.starts_with(item))
            .is_some_and(|i| i > 0 && lines[i - 1] == FENCE)
    }

    /// CP-186 N8: the crash means' two row-only entries, `Names::Fixture` and the closed list of
    /// the crash row's candidates stand behind the fence: in a build without the feature, which
    /// is no test build, none of them exists.
    #[test]
    fn the_row_only_means_stand_behind_the_fence() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let read = |f: &str| std::fs::read_to_string(src.join(f)).expect("a source file");
        let (cdp, relay, lib) = (read("cdp.rs"), read("relay.rs"), read("lib.rs"));
        for (text, item) in [
            (&cdp, "Without,"),
            (&cdp, "Candidate(usize),"),
            (&relay, "Fixture,"),
            (&lib, "pub const CRASH_CANDIDATES"),
            (&cdp, "pub async fn crash("),
        ] {
            assert!(fenced(text, item), "{item} stands outside the fence");
        }
        assert!(!fenced("#[test]\nWithout,", "Without,"));
        assert!(fenced(&format!("x\n  {FENCE}\n  Without,"), "Without,"));
    }

    /// The manifests under `dir`, at any depth, but in a directory whose name begins with a dot or
    /// is `target` or `node_modules`.
    fn manifests(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !(name.starts_with('.') || name == "target" || name == "node_modules") {
                    manifests(&path, out);
                }
            } else if name == "Cargo.toml" {
                out.push(path);
            }
        }
    }

    /// CP-186 N8, with the capture run's W3-8 (b) (2): no manifest of the repository but the
    /// crate's own turns halvo-web's feature `test-fence` on, and the crate's own `default` does
    /// not, so that no build for a person turns the fence's means on through a manifest. Each
    /// manifest is read as cargo decodes it (`fence_manifest::naming_the_fence`, U2.6 round 1,
    /// R3); the forms that it reads and its cases stand in that module.
    #[test]
    fn no_other_manifest_names_the_fence() {
        let own = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = own.ancestors().nth(2).expect("the repository");
        let mut found = Vec::new();
        manifests(root, &mut found);
        assert!(found.len() > 10, "{found:?}");
        let files: Vec<(PathBuf, String)> = found
            .into_iter()
            .map(|m| {
                let text = std::fs::read_to_string(&m).expect("a manifest");
                (m, text)
            })
            .collect();
        let naming = crate::fence_manifest::naming_the_fence(&files, &own.join("Cargo.toml"));
        assert!(naming.is_empty(), "{naming:?}");
    }

    /// CP-186 N8, with W3-8 (b) (3): a build with the feature holds the fence's marker, the byte
    /// string that the check of the shipped files reads for: this test's own program holds it.
    #[cfg(feature = "test-fence")]
    #[test]
    fn the_test_build_holds_the_marker() {
        let exe = std::fs::read(std::env::current_exe().expect("this program")).expect("read");
        let marker = &super::FENCE_MARKER[..];
        assert!(
            exe.windows(marker.len()).any(|w| w == marker),
            "the test build lacks the marker"
        );
    }
}
