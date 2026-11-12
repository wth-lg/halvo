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
use tauri::{App, Manager, WebviewWindow};

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
