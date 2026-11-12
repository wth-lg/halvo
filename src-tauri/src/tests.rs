use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use halvo_capture::consent::onboarding::{GrantState, Permission, PermissionProbe, RequestOutcome};
use halvo_capture::consent::status::{Inventory, InventoryRow, StoreInventory, UtcDay};
use halvo_capture::event::Modality;
use halvo_capture::store::{SegmentExpiry, ShredReport, StoreResult};
use serde::Deserialize;
use serde_json::Value;
use tauri::ipc::{CallbackFn, Invoke, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::app;
use crate::commands::{self, names};

pub const APP_URL: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/tests/fixtures/capture")
}

pub fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is no JSON: {e}", path.display()))
}

/// A fresh directory under the target directory's tmp, the directory that
/// cargo names CARGO_TARGET_TMPDIR for integration tests.
pub fn scratch_dir(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary has a path");
    let target = exe
        .ancestors()
        .nth(3)
        .expect("the test binary sits under target/<profile>/deps");
    let dir = target
        .join("tmp")
        .join(format!("halvo-app-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("an old scratch directory can be removed");
    }
    std::fs::create_dir_all(&dir).expect("the scratch directory can be made");
    dir
}

#[derive(Clone, Default)]
pub struct FakeStore {
    rows: Arc<Mutex<Vec<InventoryRow>>>,
}

impl FakeStore {
    pub fn two_rows_on_one_day() -> Self {
        let day = UtcDay::from_days_since_epoch(20_765).expect("2026-11-08 is a day");
        let row = |modality, events, bytes| InventoryRow {
            day,
            modality,
            events,
            bytes,
        };
        Self {
            rows: Arc::new(Mutex::new(vec![
                row(Modality::Keystroke, 7, 70),
                row(Modality::ScreenFrame, 3, 300),
            ])),
        }
    }
}

impl SegmentExpiry for FakeStore {
    fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport> {
        Ok(ShredReport {
            segments_shredded: 0,
            keys_destroyed: 0,
        })
    }

    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport> {
        let mut rows = self.rows.lock().unwrap();
        let n = rows.len() as u64;
        rows.clear();
        Ok(ShredReport {
            segments_shredded: n,
            keys_destroyed: n,
        })
    }
}

impl StoreInventory for FakeStore {
    fn inventory(&self) -> StoreResult<Inventory> {
        Ok(Inventory::from_rows(self.rows.lock().unwrap().clone()).expect("distinct rows"))
    }

    fn crypto_shred_before(&mut self, day: UtcDay) -> StoreResult<ShredReport> {
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|row| row.day >= day);
        let n = (before - rows.len()) as u64;
        Ok(ShredReport {
            segments_shredded: n,
            keys_destroyed: n,
        })
    }
}

#[derive(Clone, Default)]
pub struct FakeProbe {
    grants: Arc<Mutex<BTreeMap<String, GrantState>>>,
}

impl FakeProbe {
    fn set(&self, grants: &BTreeMap<String, GrantState>) {
        *self.grants.lock().unwrap() = grants.clone();
    }
}

fn wire_name(permission: Permission) -> String {
    serde_json::to_value(permission)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .expect("a permission has a wire name")
}

impl PermissionProbe for FakeProbe {
    fn check(&self, permission: Permission) -> GrantState {
        self.grants
            .lock()
            .unwrap()
            .get(&wire_name(permission))
            .copied()
            .unwrap_or(GrantState::Granted)
    }

    fn request(&self, permission: Permission) -> RequestOutcome {
        match self.check(permission) {
            GrantState::Granted => RequestOutcome::AlreadyGranted,
            _ => RequestOutcome::Requested,
        }
    }

    fn macos_major_version(&self) -> Option<u32> {
        Some(26)
    }
}

fn no_pane(_permission: Permission) -> Result<(), String> {
    Ok(())
}

fn no_tray(_app: &tauri::AppHandle<MockRuntime>) -> tauri::Result<()> {
    Ok(())
}

pub fn test_parts(config_dir: PathBuf, probe: &FakeProbe) -> app::Parts<MockRuntime> {
    app::Parts {
        config_dir: Some(config_dir),
        store: Box::new(FakeStore::two_rows_on_one_day()),
        store_backend: crate::unwired_store::DESCRIPTION,
        probe: Box::new(probe.clone()),
        open_pane: no_pane,
        tray: no_tray,
    }
}

pub struct TestApp {
    pub app: App<MockRuntime>,
    pub probe: FakeProbe,
}

/// The app built on the mock runtime from the real context, with a fake store
/// and a fake probe and its settings under `config_dir`, after one iteration
/// of the event loop, which runs the set-up.
pub fn test_app(config_dir: PathBuf) -> TestApp {
    let probe = FakeProbe::default();
    let parts = test_parts(config_dir, &probe);
    let mut app = app::build(mock_builder(), parts)
        .build(app::context())
        .expect("the app builds on the mock runtime");
    #[allow(deprecated)]
    app.run_iteration(|_, _| {});
    TestApp { app, probe }
}

pub fn main_window(app: &App<MockRuntime>) -> WebviewWindow<MockRuntime> {
    app.get_webview_window("main")
        .expect("the set-up made the window main")
}

pub fn invoke(
    window: &WebviewWindow<MockRuntime>,
    url: &str,
    command: &str,
    args: Value,
) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: command.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: url.parse().expect("a URL"),
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(window, request)
        .map(|body| body.deserialize::<Value>().expect("the reply is JSON"))
}

#[derive(Deserialize)]
struct Call {
    command: String,
    args: Value,
    #[serde(default)]
    probe: BTreeMap<String, GrantState>,
    reply: Option<String>,
    error: Option<String>,
    #[serde(default)]
    reply_off_macos: Option<String>,
}

#[test]
fn every_command_answers_the_page_arguments_with_its_fixture() {
    let calls: Vec<Call> = serde_json::from_value(read_json(&fixtures_dir().join("calls.json")))
        .expect("calls.json lists calls");
    assert!(!calls.is_empty());
    let test = test_app(scratch_dir("fixtures"));
    let window = main_window(&test.app);
    for (n, call) in calls.iter().enumerate() {
        test.probe.set(&call.probe);
        let got = invoke(&window, APP_URL, &call.command, call.args.clone());
        let (name, expected) = match (&call.reply, &call.error) {
            (Some(reply), None) => {
                let name = match (&call.reply_off_macos, cfg!(target_os = "macos")) {
                    (Some(other), false) => other,
                    _ => reply,
                };
                (
                    name,
                    Ok(read_json(&fixtures_dir().join(format!("{name}.json")))),
                )
            }
            (None, Some(error)) => (
                error,
                Err(read_json(&fixtures_dir().join(format!("{error}.json")))),
            ),
            _ => panic!("call {n} of calls.json names neither one reply nor one error"),
        };
        assert_eq!(
            got, expected,
            "call {n}, {} with {}: the reply differs from the fixture {name}.json",
            call.command, call.args
        );
    }
}

fn is_unknown_command(reply: &Result<Value, Value>) -> bool {
    matches!(reply, Err(Value::String(text)) if text.contains("not found"))
}

/// The names that a test asks a handler about: every name of every module,
/// every command of calls.json and one name that no module has.
fn candidate_names() -> BTreeSet<String> {
    let calls: Vec<Call> = serde_json::from_value(read_json(&fixtures_dir().join("calls.json")))
        .expect("calls.json lists calls");
    names::MODULES
        .iter()
        .flat_map(|m| m.commands.iter().map(|c| c.to_string()))
        .chain(calls.into_iter().map(|c| c.command))
        .chain(["no_such_command".to_string()])
        .collect()
}

/// The candidate names that `handler` answers, on an app built with it alone.
fn answered_by(handler: fn(Invoke<MockRuntime>) -> bool, name: &str) -> BTreeSet<String> {
    let probe = FakeProbe::default();
    let mut app = app::build(mock_builder(), test_parts(scratch_dir(name), &probe))
        .invoke_handler(handler)
        .build(app::context())
        .expect("the app builds on the mock runtime");
    #[allow(deprecated)]
    app.run_iteration(|_, _| {});
    let window = main_window(&app);
    candidate_names()
        .into_iter()
        .filter(|command| {
            !is_unknown_command(&invoke(&window, APP_URL, command, serde_json::json!({})))
        })
        .collect()
}

#[test]
fn the_module_table_routes_exactly_the_names_that_each_handler_answers() {
    let table = commands::table::<MockRuntime>();
    assert_eq!(
        table.iter().map(|m| m.names.module).collect::<Vec<_>>(),
        names::MODULES.iter().map(|n| n.module).collect::<Vec<_>>(),
        "the module table has one line for each module of names::MODULES, in its order"
    );
    for line in &table {
        let listed: BTreeSet<String> = line.names.commands.iter().map(|c| c.to_string()).collect();
        assert_eq!(
            answered_by(line.handler, line.names.module),
            listed,
            "the handler of the module {} answers other names than its list",
            line.names.module
        );
    }
    let test = test_app(scratch_dir("table"));
    let window = main_window(&test.app);
    let routed: BTreeSet<String> = candidate_names()
        .into_iter()
        .filter(|command| {
            !is_unknown_command(&invoke(&window, APP_URL, command, serde_json::json!({})))
        })
        .collect();
    let listed: BTreeSet<String> = names::MODULES
        .iter()
        .flat_map(|m| m.commands.iter().map(|c| c.to_string()))
        .collect();
    assert_eq!(
        routed, listed,
        "the app answers other names than names::MODULES"
    );
}

pub const POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

fn src_tauri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files_in(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some(extension))
        .collect();
    files.sort();
    files
}

#[test]
fn the_policy_names_no_remote_origin_and_the_global_api_is_off() {
    let test = test_app(scratch_dir("policy"));
    let config = test.app.config();
    let csp = config
        .app
        .security
        .csp
        .as_ref()
        .expect("tauri.conf.json sets a content security policy")
        .to_string();
    assert_eq!(csp, POLICY);
    for word in csp.split([' ', ';']).filter(|w| !w.is_empty()) {
        let remote = ["http:", "https:", "ws:", "wss:", "*", "data:", "blob:"]
            .iter()
            .any(|p| word.starts_with(p));
        assert!(
            !remote || word == "http://ipc.localhost",
            "the policy names {word}"
        );
        assert!(!word.starts_with("'unsafe-"), "the policy names {word}");
    }
    assert!(!config.app.with_global_tauri, "the global API is on");
    let main = config
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .expect("tauri.conf.json has the window main");
    assert!(main.incognito, "the window main keeps web storage on disk");
    assert!(
        !main.create,
        "Tauri creates the window main without the navigation rule"
    );
}

#[test]
fn only_the_window_main_of_the_app_origin_reaches_a_command() {
    let test = test_app(scratch_dir("scope"));
    let main = main_window(&test.app);
    let version = invoke(&main, APP_URL, "plugin:app|version", serde_json::json!({}));
    assert_eq!(version, Ok(Value::String(app::VERSION.to_string())));
    let status = invoke(&main, APP_URL, "capture_status", serde_json::json!({}));
    assert!(status.is_ok(), "{status:?}");
    for (origin, command) in [
        ("https://example.com", "capture_status"),
        ("http://localhost:1420", "capture_status"),
        (APP_URL, "plugin:window|close"),
        (APP_URL, "plugin:event|listen"),
        (APP_URL, "plugin:webview|create_webview_window"),
    ] {
        let reply = invoke(&main, origin, command, serde_json::json!({}));
        assert!(reply.is_err(), "{command} from {origin} answered {reply:?}");
    }
    let other = WebviewWindowBuilder::new(
        test.app.handle(),
        "other",
        WebviewUrl::App("index.html".into()),
    )
    .build()
    .expect("a second window builds on the mock runtime");
    for module in names::MODULES {
        for command in module.commands {
            let reply = invoke(&other, APP_URL, command, serde_json::json!({}));
            assert!(
                reply.is_err(),
                "{command} from the window other answered {reply:?}"
            );
        }
    }
    let reply = invoke(&other, APP_URL, "plugin:app|version", serde_json::json!({}));
    assert!(
        reply.is_err(),
        "the window other read the version: {reply:?}"
    );
}

fn allowed_by_capabilities() -> BTreeSet<String> {
    let mut allowed = BTreeSet::new();
    for file in files_in(&src_tauri().join("capabilities"), "json") {
        for permission in read_json(&file)["permissions"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has a list of permissions", file.display()))
        {
            let name = permission.as_str().expect("a permission is a name");
            if let Some(command) = name.strip_prefix("allow-") {
                allowed.insert(command.replace('-', "_"));
            }
        }
    }
    allowed
}

fn listed_in_manifest() -> BTreeSet<String> {
    files_in(&src_tauri().join("permissions/autogenerated"), "toml")
        .iter()
        .map(|file| {
            file.file_stem()
                .and_then(|s| s.to_str())
                .expect("a file name")
                .to_string()
        })
        .collect()
}

#[test]
fn the_handler_names_the_manifest_and_the_capabilities_are_one_set() {
    let test = test_app(scratch_dir("three-lists"));
    let window = main_window(&test.app);
    let handled: BTreeSet<String> = candidate_names()
        .into_iter()
        .filter(|command| {
            !is_unknown_command(&invoke(&window, APP_URL, command, serde_json::json!({})))
        })
        .collect();
    assert!(!handled.is_empty());
    assert_eq!(listed_in_manifest(), handled, "the app manifest");
    assert_eq!(allowed_by_capabilities(), handled, "the capabilities");
    for line in commands::table::<MockRuntime>() {
        let file = src_tauri()
            .join("capabilities")
            .join(format!("{}.json", line.names.module));
        assert!(
            file.exists(),
            "the module {} has no capability file",
            line.names.module
        );
    }
}

#[test]
fn every_capability_names_the_window_main_alone_and_no_remote_url() {
    let files = files_in(&src_tauri().join("capabilities"), "json");
    assert!(!files.is_empty());
    for file in files {
        let capability = read_json(&file);
        assert_eq!(
            capability["windows"],
            serde_json::json!(["main"]),
            "{}",
            file.display()
        );
        assert!(capability.get("webviews").is_none(), "{}", file.display());
        assert!(
            capability.get("remote").is_none(),
            "{} names remote URLs",
            file.display()
        );
        assert_ne!(
            capability["local"],
            Value::Bool(false),
            "{}",
            file.display()
        );
        let text = std::fs::read_to_string(&file).expect("a capability file reads");
        for scheme in ["http://", "https://", "ws://", "wss://"] {
            assert!(!text.contains(scheme), "{} names {scheme}", file.display());
        }
    }
}

#[test]
fn the_navigation_rule_allows_the_app_origin_alone() {
    let allowed = |url: &str| app::is_app_url(&url.parse().expect("a URL"));
    assert!(allowed(&format!("{APP_URL}/index.html")));
    assert!(allowed(&format!("{APP_URL}/")));
    for url in [
        "https://example.com/",
        "http://localhost:1420/",
        "file:///etc/hosts",
        "about:blank",
        "data:text/html,x",
        "tauri://other/index.html",
        "https://tauri.localhost/",
        if cfg!(windows) {
            "tauri://localhost/index.html"
        } else {
            "http://tauri.localhost/index.html"
        },
    ] {
        assert!(!allowed(url), "the rule allows {url}");
    }
}
