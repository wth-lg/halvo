use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_control::model::CaptureState;
use halvo_control::onboarding::Permission;
use halvo_control::proto::{Capture, Platform, Request};
use halvo_control::testing::{sample_inventory, sample_status, ScriptedConnector, ScriptedDaemon};
use serde::Deserialize;
use serde_json::Value;
use tauri::ipc::{CallbackFn, Invoke, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{App, AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::app;
use crate::commands::capture::AppState;
use crate::commands::control::Gated;
use crate::commands::{self, names};
use crate::engine::{self, EngineSite, GuardAnswer, GuardInstalled};
use crate::launch::Exit;
use crate::poller;
use crate::tray::{self, TrayInstalled};
use crate::view_state::StatusView;

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

fn no_pane(_permission: Permission) -> Result<(), String> {
    Ok(())
}

pub fn macos_fixture_daemon() -> ScriptedDaemon {
    let mut daemon = ScriptedDaemon::new(states::status_with(Platform::Macos, |_| {}));
    daemon.set_inventory(sample_inventory(Vec::new()));
    daemon
}

pub fn state_of(connector: &ScriptedConnector) -> AppState {
    AppState::with_control(Arc::new(Gated::new(connector.clone())), None, no_pane)
}

pub fn test_parts(dir: PathBuf, state: AppState) -> app::Parts<MockRuntime> {
    #[cfg(not(windows))]
    let _ = dir;
    app::Parts {
        #[cfg(windows)]
        webview_data_dir: Some(dir.join("webview")),
        state,
        tray: None,
        guard: guard_installed,
        poll: false,
        window_visible: false,
        engine_rules_dir: None,
    }
}

pub fn guard_installed(
    _window: &WebviewWindow<MockRuntime>,
    _site: EngineSite,
    answer: Sender<GuardAnswer>,
) {
    let _ = answer.send(Ok(engine::installed_for_tests()));
}

pub struct TestApp {
    pub app: App<MockRuntime>,
    pub daemon: ScriptedConnector,
}

impl TestApp {
    pub fn poll(&self) -> crate::view_state::StatusView {
        let state = self.app.state::<AppState>();
        poller::pass(state.control.as_ref(), &state.views, None)
    }

    pub fn deleting_requests(&self) -> usize {
        self.daemon
            .requests()
            .iter()
            .filter(|r| matches!(r, Request::DeleteAll { .. }))
            .count()
    }
}

pub fn test_app(dir: PathBuf) -> TestApp {
    test_app_with(dir, macos_fixture_daemon())
}

pub fn test_app_with(dir: PathBuf, daemon: ScriptedDaemon) -> TestApp {
    let connector = ScriptedConnector::new(daemon);
    let parts = test_parts(dir, state_of(&connector));
    test_app_from(parts, app::context(), connector)
}

pub fn test_app_from(
    parts: app::Parts<MockRuntime>,
    context: tauri::Context<MockRuntime>,
    daemon: ScriptedConnector,
) -> TestApp {
    let mut app = app::build(mock_builder(), parts)
        .build(context)
        .expect("the app builds on the mock runtime");
    first_iteration_runs_the_set_up(&mut app);
    TestApp { app, daemon }
}

fn first_iteration_runs_the_set_up(app: &mut App<MockRuntime>) {
    #[allow(deprecated)]
    app.run_iteration(|_, _| {});
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
    let mut headers = tauri::http::HeaderMap::new();
    headers.insert(
        "Origin",
        url.parse().expect("an address that a header can hold"),
    );
    invoke_with_headers(window, url, command, args, headers)
}

/// A call as the fetch path of the IPC makes it: the Origin header holds the
/// address that the framework's own check reads.
pub fn invoke_with_headers(
    window: &WebviewWindow<MockRuntime>,
    url: &str,
    command: &str,
    args: Value,
    headers: tauri::http::HeaderMap,
) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: command.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: url.parse().expect("a URL"),
        body: InvokeBody::Json(args),
        headers,
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(window, request)
        .map(|body| body.deserialize::<Value>().expect("the reply is JSON"))
}

#[derive(Deserialize)]
struct Call {
    command: String,
    args: Value,
    reply: Option<String>,
    error: Option<String>,
    #[serde(default)]
    no_poll_before: bool,
}

fn calls() -> Vec<Call> {
    serde_json::from_value(read_json(&fixtures_dir().join("calls.json")))
        .expect("calls.json lists calls")
}

fn replies_of_calls() -> Vec<(String, Result<Value, Value>)> {
    let test = test_app(scratch_dir("fixtures"));
    let window = main_window(&test.app);
    calls()
        .iter()
        .enumerate()
        .map(|(n, call)| {
            if !call.no_poll_before {
                test.poll();
            }
            let got = invoke(&window, APP_URL, &call.command, call.args.clone());
            let name = match (&call.reply, &call.error) {
                (Some(name), None) | (None, Some(name)) => name.clone(),
                _ => panic!("call {n} of calls.json names neither one reply nor one error"),
            };
            (name, got)
        })
        .collect()
}

#[test]
fn every_command_answers_the_page_arguments_with_its_fixture() {
    let calls = calls();
    assert!(!calls.is_empty());
    for ((name, got), call) in replies_of_calls().into_iter().zip(&calls) {
        let expected = read_json(&fixtures_dir().join(format!("{name}.json")));
        let expected = if call.error.is_some() {
            Err(expected)
        } else {
            Ok(expected)
        };
        assert_eq!(
            got, expected,
            "{} with {}: the reply differs from the fixture {name}.json",
            call.command, call.args
        );
    }
}

#[test]
#[ignore = "prints the replies of calls.json for a review of the fixtures"]
fn print_the_replies_of_calls_json() {
    for (name, got) in replies_of_calls() {
        let value = got.unwrap_or_else(|e| e);
        println!(
            "== {name}.json\n{}",
            serde_json::to_string_pretty(&value).expect("JSON")
        );
    }
}

fn is_unknown_command(reply: &Result<Value, Value>) -> bool {
    matches!(reply, Err(Value::String(text)) if text.contains("not found"))
}

fn candidate_names() -> BTreeSet<String> {
    let calls = calls();
    names::MODULES
        .iter()
        .flat_map(|m| m.commands.iter().map(|c| c.to_string()))
        .chain(calls.into_iter().map(|c| c.command))
        .chain(["no_such_command".to_string()])
        .collect()
}

fn answered_by(handler: fn(Invoke<MockRuntime>) -> bool, name: &str) -> BTreeSet<String> {
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    let mut app = app::build(
        mock_builder(),
        test_parts(scratch_dir(name), state_of(&connector)),
    )
    .invoke_handler(handler)
    .build(app::context())
    .expect("the app builds on the mock runtime");
    first_iteration_runs_the_set_up(&mut app);
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
        table.iter().map(|m| m.names().module).collect::<Vec<_>>(),
        names::MODULES.iter().map(|n| n.module).collect::<Vec<_>>(),
        "the module table has one line for each module of names::MODULES, in its order"
    );
    for line in &table {
        let listed: BTreeSet<String> = line
            .names()
            .commands
            .iter()
            .map(|c| c.to_string())
            .collect();
        assert_eq!(
            answered_by(line.handler, line.names().module),
            listed,
            "the handler of the module {} answers other names than its list",
            line.names().module
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

pub const POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; webrtc 'block'";

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
    assert!(
        main.incognito,
        "the window main keeps web storage on disk; on Windows the private mode is also the shipped state of the cells s06 (a preconnect hint) and s07 (a dns-prefetch hint), whose measured 0 was read with the private mode off (ruling AP-12 point 2): measure them again before this changes"
    );
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
    );
    #[cfg(windows)]
    let other = other.data_directory(scratch_dir("scope-other").join("webview"));
    let other = other
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
            .join(format!("{}.json", line.names().module));
        assert!(
            file.exists(),
            "the module {} has no capability file",
            line.names().module
        );
    }
    let list: BTreeSet<String> = halvo_coord::types::commands::LIST
        .iter()
        .map(|command| command.name.to_string())
        .collect();
    let table = commands::table::<MockRuntime>();
    let agents = table
        .iter()
        .find(|line| line.names().module == "agents")
        .expect("the module table holds the module agents");
    let names: BTreeSet<String> = agents
        .names()
        .commands
        .iter()
        .map(|c| c.to_string())
        .collect();
    assert_eq!(names, list, "the names of the module agents");
    let allowed: BTreeSet<String> =
        permission_names(&read_json(&src_tauri().join("capabilities/agents.json"))["permissions"])
            .iter()
            .filter_map(|name| name.strip_prefix("allow-"))
            .map(|command| command.replace('-', "_"))
            .collect();
    assert_eq!(allowed, list, "capabilities/agents.json");
    assert_eq!(
        answered_by(agents.handler, "three-lists-agents"),
        list,
        "the handler of the module agents"
    );
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
    assert!(!allowed(&format!("{APP_URL}:8080/index.html")));
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

#[test]
fn the_set_up_counts_the_page_loads_of_the_window_main_from_zero() {
    let test = test_app(scratch_dir("page-loads"));
    assert_eq!(test.app.state::<app::PageLoads>().count(), 0);
}

const ENGINE_URL_FORMS: &str = include_str!("engine-url-forms.txt");

#[test]
fn the_navigation_rule_refuses_every_engine_string_that_does_not_parse_and_every_other_origin() {
    let forms: Vec<&str> = ENGINE_URL_FORMS.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(forms.len(), 1336, "the committed list of engine forms");
    let mut unparsed = 0;
    for form in &forms {
        match tauri::Url::parse(form) {
            Err(_) => {
                unparsed += 1;
                assert!(
                    !app::may_navigate(form),
                    "the rule allows the unparsed string {form}"
                );
            }
            Ok(url) => {
                assert!(
                    !app::is_app_url(&url),
                    "the rule allows the outside form {form}"
                );
                assert!(
                    !app::may_navigate(form),
                    "the rule allows the outside form {form}"
                );
            }
        }
    }
    assert_eq!(
        unparsed, 70,
        "forms of the list that the URL reader refuses"
    );
    assert!(tauri::Url::parse("http://xn--zz.localhost:1/").is_err());
    assert!(!app::may_navigate("http://xn--zz.localhost:1/"));
    assert!(app::may_navigate(&format!("{APP_URL}/index.html")));
    assert!(app::may_navigate(&format!("{APP_URL}/")));
    for outside in [
        "https://example.com/",
        "http://localhost:1420/",
        "http://127.0.0.1:1/leak?d=x",
        "about:blank",
        "data:text/html,x",
    ] {
        assert!(!app::may_navigate(outside), "the rule allows {outside}");
    }
}

const UNIT_PERMISSIONS: [&str; 15] = [
    "allow-capture-status",
    "allow-inventory",
    "allow-set-capture-state",
    "allow-delete-all",
    "allow-set-storage-cap",
    "allow-onboarding-state",
    "allow-request-permission",
    "allow-open-permission-pane",
    "allow-last-delete",
    "allow-acknowledge-lost-permission",
    "allow-start-capture",
    "allow-delete-offline",
    "allow-remove-halvo",
    "allow-reset-settings",
    "core:app:allow-version",
];

const WORKFLOWS_PERMISSIONS: [&str; 4] = [
    "allow-workflows",
    "allow-approve-workflow",
    "allow-dismiss-workflow",
    "allow-delete-workflow",
];

fn permission_names(permissions: &Value) -> BTreeSet<String> {
    permissions
        .as_array()
        .expect("a list of permissions")
        .iter()
        .map(|entry| match entry {
            Value::String(name) => name.clone(),
            other => other.to_string(),
        })
        .collect()
}

fn unit_permissions() -> BTreeSet<String> {
    UNIT_PERMISSIONS.iter().map(|p| p.to_string()).collect()
}

#[test]
fn the_capability_file_holds_the_ten_permissions_of_the_unit_and_no_other() {
    let held =
        permission_names(&read_json(&src_tauri().join("capabilities/capture.json"))["permissions"]);
    let unit = unit_permissions();
    let beside: Vec<&String> = held.difference(&unit).collect();
    let missing: Vec<&String> = unit.difference(&held).collect();
    assert!(
        beside.is_empty(),
        "capabilities/capture.json holds the permissions {beside:?} beside the unit's ten"
    );
    assert!(
        missing.is_empty(),
        "capabilities/capture.json lacks {missing:?}"
    );
}

#[test]
fn the_built_context_holds_the_capability_file_and_no_other_capability() {
    let context = app::context::<MockRuntime>();
    let named = &context.config().app.security.capabilities;
    assert!(
        named.is_empty(),
        "tauri.conf.json names the capabilities {named:?}, beside the files under capabilities/"
    );
    let built = read_json(&src_tauri().join("gen/schemas/capabilities.json"));
    let identifiers: Vec<&String> = built
        .as_object()
        .expect("the capabilities of the build are an object")
        .keys()
        .collect();
    assert_eq!(
        identifiers,
        ["agents", "capture", "workflows"],
        "the build holds the capabilities {identifiers:?} beside agents, capture and workflows"
    );
    let agents = &built["agents"];
    assert_eq!(agents["windows"], serde_json::json!(["main"]), "{agents}");
    assert!(agents.get("webviews").is_none(), "{agents}");
    assert!(agents.get("remote").is_none_or(Value::is_null), "{agents}");
    let capture = &built["capture"];
    assert_eq!(capture["windows"], serde_json::json!(["main"]), "{capture}");
    assert!(capture.get("webviews").is_none(), "{capture}");
    assert!(
        capture.get("remote").is_none_or(Value::is_null),
        "{capture}"
    );
    assert_eq!(
        permission_names(&capture["permissions"]),
        unit_permissions()
    );
}

fn manifest_commands() -> BTreeSet<String> {
    let manifests = read_json(&src_tauri().join("gen/schemas/acl-manifests.json"));
    let mut commands = BTreeSet::new();
    for (key, manifest) in manifests.as_object().expect("the manifests are an object") {
        let Some(permissions) = manifest["permissions"].as_object() else {
            continue;
        };
        for permission in permissions.values() {
            for list in ["allow", "deny"] {
                for command in permission["commands"][list]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let command = command.as_str().expect("a command name");
                    commands.insert(match key.strip_prefix("core:") {
                        Some(plugin) => format!("plugin:{plugin}|{command}"),
                        None => command.to_string(),
                    });
                }
            }
        }
    }
    commands
}

fn is_refused(reply: &Result<Value, Value>) -> bool {
    matches!(reply, Err(Value::String(text)) if text.contains("not allowed") || text.contains("not found") || text == commands::REFUSED)
}

#[test]
fn every_command_of_the_manifests_answers_from_the_window_main_at_the_app_origin_alone() {
    let commands = manifest_commands();
    assert_eq!(commands.len(), 192, "commands of the generated manifests");
    let test = test_app(scratch_dir("every-command"));
    let main = main_window(&test.app);
    let other = WebviewWindowBuilder::new(
        test.app.handle(),
        "settings",
        WebviewUrl::App("index.html".into()),
    );
    #[cfg(windows)]
    let other = other.data_directory(scratch_dir("every-command-other").join("webview"));
    let other = other
        .build()
        .expect("a second window builds on the mock runtime");
    let reached = |window: &WebviewWindow<MockRuntime>, origin: &str| -> BTreeSet<String> {
        commands
            .iter()
            .filter(|command| !is_refused(&invoke(window, origin, command, serde_json::json!({}))))
            .cloned()
            .collect()
    };
    let expected: BTreeSet<String> = names::MODULES
        .iter()
        .flat_map(|module| module.commands.iter().map(|c| c.to_string()))
        .chain(["plugin:app|version".to_string()])
        .collect();
    assert_eq!(
        reached(&main, APP_URL),
        expected,
        "the commands that answer the window main at the app's origin"
    );
    let other_app_origin = if cfg!(windows) {
        "tauri://localhost"
    } else {
        "http://tauri.localhost"
    };
    for origin in [
        "https://example.com",
        "https://a.example.org",
        "http://localhost:1420",
        other_app_origin,
    ] {
        let answered = reached(&main, origin);
        assert!(
            answered.is_empty(),
            "the window main at {origin} reaches {answered:?}"
        );
    }
    let answered = reached(&other, APP_URL);
    assert!(
        answered.is_empty(),
        "the window settings at the app's origin reaches {answered:?}"
    );
    widened_labels_reach_no_command_of_the_unit();
}

/// The two ways in which a capability can reach a build beside the file: the
/// context's own authority, as a capability handed to the context macro lands,
/// and a capability added at run time after the set-up.
#[derive(Clone, Copy, Debug)]
enum Route {
    Context,
    RunTime,
}

const MADE_UP_LABEL: &str = "zq-label-of-this-test";

fn widening(labels: &[&str], identifier: &str) -> String {
    serde_json::json!({
        "identifier": identifier,
        "windows": labels,
        "permissions": UNIT_PERMISSIONS
            .iter()
            .map(|p| p.to_string())
            .chain(permission_names(
                &read_json(&src_tauri().join("capabilities/agents.json"))["permissions"],
            ))
            .chain(WORKFLOWS_PERMISSIONS.iter().map(|p| p.to_string()))
            .collect::<Vec<String>>(),
    })
    .to_string()
}

/// The app with every command of the unit granted to `labels` by `route`.
fn widened_app(name: &str, labels: &[&str], route: Route) -> TestApp {
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    let parts = test_parts(scratch_dir(name), state_of(&connector));
    let mut context = app::context::<MockRuntime>();
    if let Route::Context = route {
        context
            .runtime_authority_mut()
            .add_capability(widening(labels, "widened-in-the-context"))
            .expect("the context takes a capability");
    }
    let test = test_app_from(parts, context, connector);
    if let Route::RunTime = route {
        test.app
            .add_capability(widening(labels, "widened-at-run-time"))
            .expect("the app takes a capability at run time");
    }
    test
}

fn window_of(test: &TestApp, label: &str) -> WebviewWindow<MockRuntime> {
    let window = WebviewWindowBuilder::new(
        test.app.handle(),
        label,
        WebviewUrl::App("index.html".into()),
    );
    #[cfg(windows)]
    let window = window.data_directory(scratch_dir(&format!("window-{label}")).join("webview"));
    window.build().expect("a window builds on the mock runtime")
}

fn unit_commands() -> Vec<&'static str> {
    names::MODULES
        .iter()
        .flat_map(|module| module.commands.iter().copied())
        .collect()
}

#[test]
fn a_window_other_than_main_gets_no_command_of_the_unit_whatever_a_capability_grants() {
    for route in [Route::Context, Route::RunTime] {
        for label in ["helper", MADE_UP_LABEL] {
            let test = widened_app(&format!("widened-{route:?}-{label}"), &[label], route);
            let window = window_of(&test, label);
            for command in unit_commands() {
                let reply = invoke(&window, APP_URL, command, serde_json::json!({}));
                assert_eq!(
                    reply,
                    Err(Value::String(commands::REFUSED.to_string())),
                    "{command} from the window {label} at the app origin, granted by the route {route:?}"
                );
            }
        }
    }
}

fn near_origin(port: bool, user: bool) -> String {
    let (scheme, host) = app::APP_ORIGIN;
    let user = if user { "user@" } else { "" };
    let port = if port { ":8080" } else { "" };
    format!("{scheme}://{user}{host}{port}")
}

#[test]
fn a_call_of_the_app_host_from_another_port_or_with_user_information_is_refused() {
    let test = test_app(scratch_dir("near-origins"));
    let main = main_window(&test.app);
    for address in [near_origin(true, false), near_origin(false, true)] {
        for command in unit_commands() {
            let reply = invoke(&main, &address, command, serde_json::json!({}));
            assert_eq!(
                reply,
                Err(Value::String(commands::REFUSED.to_string())),
                "{command} from the window main at {address}"
            );
        }
    }
    let status = invoke_with_headers(
        &main,
        APP_URL,
        "capture_status",
        serde_json::json!({}),
        Default::default(),
    );
    assert!(
        status.is_ok(),
        "a call with no Origin header from the app's page: {status:?}"
    );
    let near = near_origin(true, false);
    main.navigate(near.parse().expect("a URL"))
        .expect("the mock runtime records the address");
    for command in unit_commands() {
        let reply = invoke_with_headers(
            &main,
            &near,
            command,
            serde_json::json!({}),
            Default::default(),
        );
        assert_eq!(
            reply,
            Err(Value::String(commands::REFUSED.to_string())),
            "{command} from the window main whose own address is {near}, with no Origin header"
        );
    }
}

fn widened_labels_reach_no_command_of_the_unit() {
    let commands: BTreeSet<String> = unit_commands().iter().map(|c| c.to_string()).collect();
    for route in [Route::Context, Route::RunTime] {
        let test = widened_app(
            &format!("every-command-{route:?}"),
            &["helper", MADE_UP_LABEL],
            route,
        );
        for label in ["helper", MADE_UP_LABEL] {
            let window = window_of(&test, label);
            let answered: Vec<&String> = commands
                .iter()
                .filter(|c| !is_refused(&invoke(&window, APP_URL, c, serde_json::json!({}))))
                .collect();
            assert!(
                answered.is_empty(),
                "the window {label}, widened by the route {route:?}, reaches {answered:?}"
            );
        }
    }
}

/// The source files of the app that a build compiles outside the tests: every
/// file under src-tauri/src but tests.rs, and build.rs.
/// The sources of the app as the text tests read them, each with its line ends
/// as LF: a Windows checkout writes CRLF, and a needle that ends a line ends in
/// `\n` alone.
fn sources_of_the_app() -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("a source directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
                && path.file_name().and_then(|n| n.to_str()) != Some("tests.rs")
            {
                out.push(path);
            }
        }
    }
    let mut files = vec![src_tauri().join("build.rs")];
    walk(&src_tauri().join("src"), &mut files);
    files.sort();
    files
        .into_iter()
        .map(|file| {
            let text = std::fs::read_to_string(&file).expect("a source file reads");
            (file, text.replace("\r\n", "\n"))
        })
        .collect()
}

/// This test reads text: it catches a line that adds a capability at run time
/// or hands the context macro a capability (tauri 2.12.5 `Manager::add_capability`,
/// `Context::runtime_authority_mut` with `RuntimeAuthority::add_capability` and
/// `__allow_command`; tauri-macros 2.7.4 `generate_context!(capabilities = ...)`).
/// The guard of the app's handler stops the call itself.
#[test]
fn no_source_of_the_app_adds_a_capability_at_run_time_or_hands_one_to_the_context_macro() {
    let calls = [
        ["add", "_capability"].concat(),
        ["runtime", "_authority_mut"].concat(),
        ["__allow", "_command"].concat(),
    ];
    let macro_call = ["generate", "_context!("].concat();
    for (file, text) in sources_of_the_app() {
        for (n, line) in text.lines().enumerate() {
            for call in &calls {
                assert!(
                    !line.contains(call.as_str()),
                    "{}:{} calls {call}",
                    file.display(),
                    n + 1
                );
            }
            if let Some(at) = line.find(&macro_call) {
                assert!(
                    line[at + macro_call.len()..].starts_with(')'),
                    "{}:{} hands the context macro an argument: {line}",
                    file.display(),
                    n + 1
                );
            }
        }
    }
}

/// This test reads text, because the mock runtime never navigates: the one
/// builder chain of a window under src-tauri/src hands the rule to Tauri with
/// may_navigate and nothing beside it. Only a real engine shows that the built
/// window asks the rule.
#[test]
fn the_set_up_hands_may_navigate_to_the_window_main_and_no_other_window_builder_stands() {
    let sources = sources_of_the_app();
    let count = |needle: &str| -> usize {
        sources
            .iter()
            .map(|(_, text)| text.matches(needle).count())
            .sum()
    };
    assert_eq!(
        count("WebviewWindowBuilder::"),
        1,
        "window builders under src-tauri/src"
    );
    assert_eq!(
        count("WebviewBuilder::"),
        0,
        "webview builders under src-tauri/src"
    );
    assert_eq!(
        count(".add_child("),
        0,
        "child webviews under src-tauri/src"
    );
    assert_eq!(
        count(".on_navigation("),
        1,
        "navigation hooks under src-tauri/src"
    );
    assert_eq!(
        count(".on_navigation(|url| may_navigate(url.as_str()))\n"),
        1,
        "the navigation hook of the window main hands may_navigate and nothing beside it"
    );
}

/// This test reads text, because the mock runtime runs no script: the close
/// handler sends WINDOW_HIDDEN to the page before it hides the window. Only the
/// real window shows that the page disarms.
#[test]
fn the_close_handler_tells_the_page_before_it_hides_the_window() {
    let text = std::fs::read_to_string(src_tauri().join("src/app.rs")).expect("app.rs reads");
    let close = text
        .find("WindowEvent::CloseRequested")
        .expect("the close handler");
    let eval = text[close..]
        .find("webview.eval(WINDOW_HIDDEN)")
        .expect("the close handler sends WINDOW_HIDDEN");
    let hide = text[close..]
        .find("window.hide()")
        .expect("the close handler hides the window");
    assert!(
        eval < hide,
        "the close handler hides the window before it tells the page"
    );
}

const MACOS_NEAR_MISSES: [&str; 7] = [
    "http://localhost/",
    "https://localhost/",
    "ws://localhost/",
    "tauri://a.localhost/",
    "tauri://notlocalhost/",
    "tauri://user@localhost/",
    "tauri://user:pw@localhost/",
];
const WINDOWS_NEAR_MISSES: [&str; 6] = [
    "https://tauri.localhost/",
    "http://a.tauri.localhost/",
    "http://nottauri.localhost/",
    "http://tauri.localhost:8080/",
    "http://user@tauri.localhost/",
    "http://user:pw@tauri.localhost/",
];

#[test]
fn each_arm_of_the_rule_refuses_the_near_misses_of_its_origin() {
    for (origin, own, misses) in [
        (
            ("tauri", "localhost"),
            "tauri://localhost/index.html",
            &MACOS_NEAR_MISSES[..],
        ),
        (
            ("http", "tauri.localhost"),
            "http://tauri.localhost/index.html",
            &WINDOWS_NEAR_MISSES[..],
        ),
    ] {
        let parse = |address: &str| address.parse::<tauri::Url>().expect("a URL");
        assert!(app::is_origin(&parse(own), origin), "{own}");
        for miss in misses {
            assert!(
                !app::is_origin(&parse(miss), origin),
                "the arm {origin:?} allows {miss}"
            );
        }
    }
    let own_misses = if cfg!(windows) {
        &WINDOWS_NEAR_MISSES[..]
    } else {
        &MACOS_NEAR_MISSES[..]
    };
    for miss in own_misses.iter().chain(["tauri://localhost:8080/"].iter()) {
        assert!(
            !app::may_navigate(miss),
            "the rule of this system allows {miss}"
        );
    }
}

#[test]
fn the_configuration_holds_no_dev_policy_and_no_dev_address() {
    let config = app::context::<MockRuntime>().config().clone();
    assert!(
        config.app.security.dev_csp.is_none(),
        "tauri.conf.json sets app.security.devCsp, which a dev build serves in place of the policy"
    );
    assert!(
        config.build.dev_url.is_none(),
        "tauri.conf.json sets build.devUrl, which a dev build loads and counts as the app's origin"
    );
}

mod answers;
mod layer;
mod states;

#[derive(Default)]
pub struct GuardCalls {
    urls: Mutex<Vec<String>>,
    held: Mutex<Option<Sender<GuardAnswer>>>,
}

fn called_at(window: &WebviewWindow<MockRuntime>) {
    let url = window.url().map(|url| url.to_string()).unwrap_or_default();
    window
        .state::<GuardCalls>()
        .urls
        .lock()
        .expect("the calls")
        .push(url);
}

fn guard_held(window: &WebviewWindow<MockRuntime>, _site: EngineSite, answer: Sender<GuardAnswer>) {
    called_at(window);
    *window.state::<GuardCalls>().held.lock().expect("the slot") = Some(answer);
}

fn guard_refused(
    window: &WebviewWindow<MockRuntime>,
    _site: EngineSite,
    answer: Sender<GuardAnswer>,
) {
    called_at(window);
    let _ = answer.send(Err(engine::NotInstalled::from("refused by the stand-in")));
}

fn guard_dropped(
    window: &WebviewWindow<MockRuntime>,
    _site: EngineSite,
    answer: Sender<GuardAnswer>,
) {
    called_at(window);
    drop(answer);
}

fn tray_stand_in(_app: &AppHandle<MockRuntime>) -> tauri::Result<TrayInstalled> {
    Ok(tray::installed_for_tests())
}

fn app_with_guard(
    name: &str,
    guard: engine::GuardInstall<MockRuntime>,
    tray: Option<app::TrayInstall<MockRuntime>>,
) -> App<MockRuntime> {
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    let mut parts = test_parts(scratch_dir(name), state_of(&connector));
    parts.guard = guard;
    parts.tray = tray;
    let mut app = app::build(mock_builder().manage(GuardCalls::default()), parts)
        .build(app::context())
        .expect("the app builds on the mock runtime");
    first_iteration_runs_the_set_up(&mut app);
    app
}

fn guard_urls(app: &App<MockRuntime>) -> Vec<String> {
    app.state::<GuardCalls>()
        .urls
        .lock()
        .expect("the calls")
        .clone()
}

fn url_of_main(app: &App<MockRuntime>) -> Option<String> {
    app.get_webview_window("main")
        .and_then(|window| window.url().ok())
        .map(|url| url.to_string())
}

#[test]
fn the_guard_is_asked_while_the_window_main_is_blank_and_the_app_page_loads_after_its_answer() {
    let app = app_with_guard("guard-held", guard_held, None);
    assert_eq!(guard_urls(&app), ["about:blank"]);
    assert_eq!(url_of_main(&app).as_deref(), Some("about:blank"));
    assert!(app.try_state::<GuardInstalled>().is_none());

    let sender = app
        .state::<GuardCalls>()
        .held
        .lock()
        .expect("the slot")
        .take()
        .expect("the stand-in holds the sender");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(url_of_main(&app).as_deref(), Some("about:blank"));
    sender
        .send(Ok(engine::installed_for_tests()))
        .expect("the set-up waits for the answer");

    let deadline = Instant::now() + Duration::from_secs(10);
    while app.try_state::<GuardInstalled>().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.try_state::<GuardInstalled>().is_some());
    assert_eq!(url_of_main(&app), Some(app::app_url().to_string()));
    assert_eq!(guard_urls(&app), ["about:blank"]);
}

#[test]
fn an_answer_at_once_loads_the_app_page_within_the_set_up() {
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    let parts = test_parts(scratch_dir("guard-at-once"), state_of(&connector));
    let test = test_app_from(parts, app::context(), connector);
    assert!(test.app.try_state::<GuardInstalled>().is_some());
    assert_eq!(url_of_main(&test.app), Some(app::app_url().to_string()));
    assert!(test.app.try_state::<app::WindowMissing>().is_none());
}

#[test]
fn with_the_guard_not_installed_and_a_tray_the_app_runs_on_without_the_window_main() {
    for (name, guard) in [
        (
            "guard-refused",
            guard_refused as engine::GuardInstall<MockRuntime>,
        ),
        ("guard-dropped", guard_dropped),
    ] {
        let app = app_with_guard(name, guard, Some(tray_stand_in));
        assert_eq!(guard_urls(&app), ["about:blank"], "{name}");
        assert_eq!(url_of_main(&app).as_deref(), Some("about:blank"), "{name}");
        assert!(app.try_state::<GuardInstalled>().is_none(), "{name}");
        assert!(app.try_state::<app::WindowMissing>().is_some(), "{name}");
        let state = app.state::<AppState>();
        assert!(state
            .tray_installed
            .load(std::sync::atomic::Ordering::SeqCst));
        let view = state.views.view();
        let shown = tray::shown(app.handle(), &view);
        let usual = tray::labels(&view);
        assert_eq!(
            shown.status_text,
            if cfg!(target_os = "macos") {
                "Halvo's window cannot open on this Mac right now. You can still pause or resume here. Quit Halvo and start it again; if the window still does not open, a newer Halvo is needed to see or delete what Halvo has stored."
            } else {
                "Halvo's window is not available. Quit Halvo and start it again."
            },
            "{name}"
        );
        assert!(!shown.open_enabled, "{name}");
        assert!(usual.open_enabled, "{name}");
        assert_eq!(
            (
                shown.title,
                shown.tooltip,
                shown.toggle_text,
                shown.toggle_enabled
            ),
            (
                usual.title,
                usual.tooltip,
                usual.toggle_text,
                usual.toggle_enabled
            ),
            "{name}"
        );
    }
}

#[test]
fn the_engine_guard_on_the_mock_runtime_never_runs_its_closure_and_reads_as_not_installed() {
    let app = app_with_guard("guard-engine-on-mock", engine::guard, Some(tray_stand_in));
    assert_eq!(url_of_main(&app).as_deref(), Some("about:blank"));
    assert!(app.try_state::<GuardInstalled>().is_none());
    assert!(app.try_state::<app::WindowMissing>().is_some());
}

#[test]
fn the_deciding_function_loads_on_installed_runs_on_with_a_tray_and_ends_without_one() {
    assert!(matches!(
        app::decide(Ok(engine::installed_for_tests()), false),
        app::Opening::Load(_)
    ));
    assert!(matches!(
        app::decide(Ok(engine::installed_for_tests()), true),
        app::Opening::Load(_)
    ));
    assert!(matches!(
        app::decide(Err(engine::NotInstalled::from("x")), true),
        app::Opening::RunWithoutWindow(refusal) if refusal.reason == "x" && refusal.restart_cures
    ));
    let app::Opening::End(reason) = app::decide(
        Err(engine::NotInstalled::from(
            "the install's closure did not run",
        )),
        false,
    ) else {
        panic!("without a tray the app ends");
    };
    assert_eq!(
        app::not_opened_line(&reason.reason),
        "halvo: the window main is not opened, because its engine guard is not installed: the install's closure did not run"
    );
    let code = Exit::GuardNotInstalled.code();
    assert!(![0, 1, 64].contains(&code), "{code}");
}

#[test]
fn the_window_main_is_built_blank_and_hidden_from_the_set_up_alone_and_counts_loads_of_the_app() {
    let app = std::fs::read_to_string(src_tauri().join("src/app.rs"))
        .expect("app.rs reads")
        .replace("\r\n", "\n");
    for (text, count) in [
        ("main_window(", 1),
        ("fn main_window<", 1),
        ("window.destroy()", 1),
        ("config.url = WebviewUrl::External(blank());\n", 1),
        (".visible(false);\n", 1),
        (".visible(", 1),
        (".navigate(", 1),
        (
            "window.navigate(app_url())?;\n    if visible {\n        window.show()?;\n    }\n",
            1,
        ),
        ("&& is_app_url(payload.url())\n", 1),
        (
            "        .on_download(|_, _| false)\n        .initialization_script_for_all_frames(NO_PEER_SCRIPT)\n        .visible(false);\n",
            1,
        ),
        ("initialization_script", 1),
        (
            "    #[cfg(target_os = \"macos\")]\n    let builder = match crate::engine::configuration_at_creation() {\n        Some(configuration) => builder.with_webview_configuration(configuration),\n        None => builder,\n    };\n",
            1,
        ),
        ("with_webview_configuration", 1),
        ("app.manage(installed);", 1),
    ] {
        assert_eq!(app.matches(text).count(), count, "{text}");
    }
    let guard = app
        .find("(parts.guard)(&window, site, sender)")
        .expect("the guard's call");
    let tray = app
        .find("let tray = match parts.tray")
        .expect("the tray's install");
    let decided = app.find("match answer.try_recv()").expect("the decision");
    assert!(guard < tray && tray < decided);
}

#[cfg(windows)]
#[test]
fn the_engine_start_arguments_repeat_the_runtime_default_and_map_every_host_to_no_address() {
    assert_eq!(
        app::ENGINE_ARGS,
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --host-resolver-rules=\"MAP * ~NOTFOUND\" --proxy-server=http://255.255.255.255:9 --proxy-bypass-list=<-loopback> --webrtc-ip-handling-policy=disable_non_proxied_udp"
    );
    let app = std::fs::read_to_string(src_tauri().join("src/app.rs")).expect("app.rs reads");
    assert_eq!(
        app.matches(".additional_browser_args(ENGINE_ARGS)").count(),
        1
    );
    assert_eq!(app.matches("proxy_url").count(), 0);
}

#[test]
fn the_script_of_every_frame_takes_the_peer_connection_constructors_away() {
    assert_eq!(
        app::NO_PEER_SCRIPT,
        "(function () { for (const name of ['RTCPeerConnection', 'webkitRTCPeerConnection', 'RTCSessionDescription', 'RTCIceCandidate', 'RTCDataChannel']) { try { Object.defineProperty(window, name, { value: undefined, writable: false, configurable: false }); } catch (e) {} } })();"
    );
}

/// wry 0.57.1 refuses every request for a new window when the app sets no handler
/// (src/webview2/mod.rs lines 861 to 863, src/wkwebview/class/wry_web_view_ui_delegate.rs
/// lines 288 to 290; tauri-runtime-wry 2.12.3 src/lib.rs line 4829), so no handler stands.
#[test]
fn no_handler_for_a_new_window_stands_so_the_runtime_refuses_every_new_window() {
    let sources = sources_of_the_app();
    let handlers: usize = sources
        .iter()
        .map(|(_, text)| text.matches(".on_new_window(").count())
        .sum();
    assert_eq!(handlers, 0, "new-window handlers under src-tauri/src");
}

#[test]
fn the_pins_of_the_new_window_cells_stand_in_the_lockfile() {
    let lock = std::fs::read_to_string(src_tauri().join("../Cargo.lock"))
        .expect("Cargo.lock reads")
        .replace("\r\n", "\n");
    for (crate_name, version) in [("wry", "0.57.1"), ("tauri-runtime-wry", "2.12.3")] {
        let name_line = format!("name = \"{crate_name}\"\n");
        assert_eq!(
            (
                lock.matches(&name_line).count(),
                lock.matches(&format!("{name_line}version = \"{version}\"\n"))
                    .count()
            ),
            (1, 1),
            "Cargo.lock holds {crate_name} {version} once: the cells s04 and s05 (a link with a target and window.open, no route found) rest on wry 0.57.1 src/webview2/mod.rs lines 861 to 863, src/wkwebview/class/wry_web_view_ui_delegate.rs lines 288 to 290 and tauri-runtime-wry 2.12.3 src/lib.rs line 4829; read those lines again in the new versions and measure s04 and s05 before the pin moves"
        );
    }
}

#[test]
fn no_source_of_the_app_names_dns_prefetching() {
    let word = ["dns", "prefetching"].concat();
    let named: Vec<String> = sources_of_the_app()
        .iter()
        .filter(|(_, text)| text.to_lowercase().contains(&word))
        .map(|(file, _)| file.display().to_string())
        .collect();
    assert_eq!(
        named,
        Vec::<String>::new(),
        "a source under src-tauri/src names DNS prefetching (DNSPrefetchingEnabled or _setDNSPrefetchingEnabled), a setting that Halvo does not set: the cell s07 on macOS was closed only with DNS prefetching on (ruling AP-12 point 2)"
    );
}

#[test]
fn no_file_of_the_page_holds_a_preconnect_or_dns_prefetch_link() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("a directory of ui/app") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&src_tauri().join("../ui/app"), &mut files);
    assert!(files.len() > 1, "the files of ui/app are listed");
    let words = [["pre", "connect"].concat(), ["dns-", "prefetch"].concat()];
    let held: Vec<String> = files
        .iter()
        .filter(|file| {
            let text = std::fs::read_to_string(file)
                .expect("a file of ui/app reads")
                .to_lowercase();
            words.iter().any(|word| text.contains(word.as_str()))
        })
        .map(|file| file.display().to_string())
        .collect();
    assert_eq!(
        held,
        Vec::<String>::new(),
        "a file of ui/app holds a preconnect or dns-prefetch link, the hints of the cells s06 and s07 (ruling AP-12 point 2)"
    );
}

#[test]
fn the_engine_arguments_of_the_environment_are_taken_out_on_windows_alone_at_the_start_of_main() {
    assert_eq!(
        app::ENGINE_ARGUMENTS_VARIABLE,
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"
    );
    let main = std::fs::read_to_string(src_tauri().join("src/main.rs"))
        .expect("main.rs reads")
        .replace("\r\n", "\n");
    assert!(
        main.contains("fn main() {\n    #[cfg(windows)]\n    app::take_engine_arguments_out_of_the_environment();\n    let started = std::time::Instant::now();\n    match launch::parse("),
        "the first statement of main, before any thread exists, under cfg(windows)"
    );
    let app = std::fs::read_to_string(src_tauri().join("src/app.rs"))
        .expect("app.rs reads")
        .replace("\r\n", "\n");
    assert!(
        app.contains("#[cfg(windows)]\npub fn take_engine_arguments_out_of_the_environment() {")
    );
    assert_eq!(
        app.matches("take_engine_arguments_out_of_the_environment();")
            .count(),
        0,
        "no later call while threads exist"
    );
    assert_eq!(
        main.matches("take_engine_arguments_out_of_the_environment();")
            .count(),
        1
    );
}

#[cfg(windows)]
#[test]
fn on_windows_the_engine_arguments_of_the_environment_are_taken_out() {
    std::env::set_var(app::ENGINE_ARGUMENTS_VARIABLE, "--proxy-server=direct://");
    app::take_engine_arguments_out_of_the_environment();
    assert!(std::env::var_os(app::ENGINE_ARGUMENTS_VARIABLE).is_none());
}

fn guard_refused_on_this_system(
    window: &WebviewWindow<MockRuntime>,
    _site: EngineSite,
    answer: Sender<GuardAnswer>,
) {
    called_at(window);
    let _ = answer.send(Err(engine::NotInstalled::on_this_system(
        "the preferences of this system have no peer-connection setting",
    )));
}

thread_local! {
    static REFUSAL: std::cell::RefCell<Option<engine::NotInstalled>> =
        const { std::cell::RefCell::new(None) };
}

fn guard_refusing_as_set(
    window: &WebviewWindow<MockRuntime>,
    _site: EngineSite,
    answer: Sender<GuardAnswer>,
) {
    called_at(window);
    let refusal = REFUSAL
        .with(|refusal| refusal.borrow_mut().take())
        .expect("the test sets the refusal");
    let _ = answer.send(Err(refusal));
}

#[test]
fn every_cause_that_keeps_the_window_closed_gives_the_tray_one_sentence_on_macos() {
    let causes = [
        engine::NotInstalled::from("no store of rule lists"),
        engine::NotInstalled::from("the rule list did not compile: x"),
        engine::NotInstalled::on_this_system(
            "the preferences of this system have no peer-connection setting",
        ),
        engine::store_keeps_data(),
        engine::every_network_host_allowed(),
        engine::network_hosts_allowed(2),
        engine::NotInstalled::on_this_system(engine::DELEGATE_READS_NONE),
        engine::NotInstalled::on_this_system(engine::DELEGATE_READS_ANOTHER),
        engine::NotInstalled::on_this_system(engine::GUARD_ANSWERS_OTHERWISE),
        engine::NotInstalled::on_this_system(engine::PROXY_NOT_READ_BACK),
        engine::NotInstalled::on_this_system("the data store reads back 0 proxy configurations"),
    ];
    for cause in causes {
        let name = cause.reason.clone();
        let restart_cures = cause.restart_cures;
        REFUSAL.with(|refusal| *refusal.borrow_mut() = Some(cause));
        let app = app_with_guard("guard-cause", guard_refusing_as_set, Some(tray_stand_in));
        assert_eq!(
            app.state::<app::WindowMissing>().restart_cures,
            restart_cures,
            "{name}"
        );
        let view = app.state::<AppState>().views.view();
        let expected = if cfg!(target_os = "macos") {
            "Halvo's window cannot open on this Mac right now. You can still pause or resume here. Quit Halvo and start it again; if the window still does not open, a newer Halvo is needed to see or delete what Halvo has stored."
        } else if restart_cures {
            "Halvo's window is not available. Quit Halvo and start it again."
        } else {
            "Halvo's window cannot open on this version of macOS. You can still pause or resume here. A newer Halvo is needed to see or delete what Halvo has stored."
        };
        assert_eq!(
            tray::shown(app.handle(), &view).status_text,
            expected,
            "{name}"
        );
    }
}

#[test]
fn a_window_that_cannot_open_keeps_the_trays_other_texts_and_disables_open() {
    let app = app_with_guard(
        "guard-this-system",
        guard_refused_on_this_system,
        Some(tray_stand_in),
    );
    assert_eq!(url_of_main(&app).as_deref(), Some("about:blank"));
    let missing = app.state::<app::WindowMissing>();
    assert!(!missing.restart_cures);
    let view = app.state::<AppState>().views.view();
    let shown = tray::shown(app.handle(), &view);
    assert!(!shown.open_enabled);
    let usual = tray::labels(&view);
    assert_eq!(
        (
            shown.title,
            shown.tooltip,
            shown.toggle_text,
            shown.toggle_enabled
        ),
        (
            usual.title,
            usual.tooltip,
            usual.toggle_text,
            usual.toggle_enabled
        )
    );
}

/// This test reads the manifests: no build of the app turns on the feature `scripted` of
/// halvo-coord-app, the stand-in presenter of that crate's tests, neither by the app's own
/// line nor by a default feature of the crate.
#[test]
fn no_manifest_turns_the_feature_scripted_of_halvo_coord_app_on_for_the_app() {
    let read = |path: &str| {
        std::fs::read_to_string(src_tauri().join(path))
            .unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
            .replace("\r\n", "\n")
    };
    let app = read("Cargo.toml");
    let lines: Vec<&str> = app
        .lines()
        .filter(|line| line.contains("halvo-coord-app"))
        .collect();
    assert_eq!(
        lines,
        [r#"halvo-coord-app = { path = "../crates/halvo-coord-app" }"#],
        "src-tauri/Cargo.toml names halvo-coord-app on these lines"
    );
    let own = read("../crates/halvo-coord-app/Cargo.toml");
    let defaults: Vec<&str> = own
        .lines()
        .filter(|line| line.trim_start().starts_with("default") && line.contains("scripted"))
        .collect();
    assert!(
        defaults.is_empty(),
        "crates/halvo-coord-app/Cargo.toml turns scripted on by default: {defaults:?}"
    );
}

/// The tray's count of agents that need the person (I10.3): above zero the icon variant and
/// the tooltip of `Attention::of`, and on macOS the tray's own title with the count after it,
/// so that the title still says whether Halvo captures; at zero the tray's own.
#[test]
fn the_tray_shows_the_count_of_agents_that_need_the_person_as_attention_of_gives_it() {
    let test = test_app(scratch_dir("tray-count"));
    let handle = test.app.handle();
    let view = test.app.state::<AppState>().views.view();
    let own = tray::shown(handle, &view);
    for count in [0, 1, 12] {
        tray::set_attention(handle, count);
        let attention = halvo_coord_app::Attention::of(count);
        let shown = tray::with_attention(handle, &view);
        assert_eq!(shown.variant, attention.variant, "{count}");
        let tooltip = attention.tooltip.unwrap_or_else(|| own.tooltip.to_string());
        assert_eq!(shown.tooltip, tooltip, "{count}");
        let title = if count > 0 && cfg!(target_os = "macos") {
            format!("{} {count}", own.title)
        } else {
            own.title.to_string()
        };
        assert_eq!(shown.title, title, "{count}");
    }
    let title_with = |capture, desired| {
        let StatusView::Running(mut running) = StatusView::of(&sample_status(Platform::Macos))
        else {
            panic!("the sample status makes a running view")
        };
        running.capture = capture;
        running.desired = desired;
        tray::with_attention(handle, &StatusView::Running(running)).title
    };
    assert_ne!(
        title_with(Capture::Running, CaptureState::Running),
        title_with(Capture::Paused, CaptureState::Paused),
        "with 12 agents that need the person, the title still says whether Halvo captures"
    );
}

/// A stand-in for the app's `Core` that records each call with the thread that made it, and
/// whose tick panics at the numbers in `panics`.
struct Recorder {
    calls: Mutex<Vec<(&'static str, std::thread::ThreadId)>>,
    panics: Vec<usize>,
    hold: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl Recorder {
    fn new(panics: &[usize]) -> Arc<Recorder> {
        Arc::new(Recorder {
            calls: Mutex::new(Vec::new()),
            panics: panics.to_vec(),
            hold: Mutex::new(None),
        })
    }

    /// A recorder whose quit hook waits until `release` is sent or dropped.
    fn holding_quit(release: std::sync::mpsc::Receiver<()>) -> Arc<Recorder> {
        Arc::new(Recorder {
            calls: Mutex::new(Vec::new()),
            panics: Vec::new(),
            hold: Mutex::new(Some(release)),
        })
    }

    fn record(&self, name: &'static str) -> usize {
        let mut calls = self
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        calls.push((name, std::thread::current().id()));
        calls.iter().filter(|(n, _)| *n == name).count()
    }

    fn calls(&self) -> Vec<(&'static str, std::thread::ThreadId)> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn names(&self) -> Vec<&'static str> {
        self.calls().into_iter().map(|(name, _)| name).collect()
    }

    fn wait_for(&self, name: &str, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.names().iter().filter(|n| **n == name).count() < count {
            assert!(
                Instant::now() < deadline,
                "the stand-in saw {:?}",
                self.names()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl halvo_coord_app::Core for Recorder {
    fn call(&self, _command: &str, _args: Option<&Value>) -> halvo_coord_app::Reply {
        self.record("call");
        halvo_coord_app::Reply::Closed
    }

    fn shown(&self) {
        self.record("shown");
    }

    fn hidden(&self) {
        self.record("hidden");
    }

    fn destroyed(&self) {
        self.record("destroyed");
    }

    fn tick(&self) {
        let n = self.record("tick");
        assert!(!self.panics.contains(&n), "tick {n} of the stand-in panics");
    }

    fn quit(
        &self,
        budget: Option<Duration>,
    ) -> Vec<(halvo_coord::types::AgentId, halvo_coord::types::RunId)> {
        self.record(if budget.is_none() {
            "quit"
        } else {
            "quit with a budget"
        });
        let hold = self
            .hold
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(release) = hold {
            let _ = release.recv();
        }
        Vec::new()
    }

    fn hold_runs(
        &self,
        held: bool,
    ) -> Vec<(halvo_coord::types::AgentId, halvo_coord::types::RunId)> {
        self.record(if held { "hold runs" } else { "lift runs" });
        Vec::new()
    }

    fn on_screen(&self, _item: &halvo_coord::types::ItemId) {
        self.record("on_screen");
    }

    fn answer_native(
        &self,
        _item: &halvo_coord::types::ItemId,
        _answer: halvo_coord::items::ConfirmAnswer,
    ) -> Result<(), halvo_coord::types::Cause> {
        self.record("answer_native");
        Err(halvo_coord::types::Cause::NoService)
    }
}

/// The app on the mock runtime with `core` as the Core that it manages.
fn app_with_core(
    name: &str,
    core: Arc<Recorder>,
    guard: engine::GuardInstall<MockRuntime>,
    tray: Option<app::TrayInstall<MockRuntime>>,
) -> TestApp {
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    let mut parts = test_parts(scratch_dir(name), state_of(&connector));
    parts.guard = guard;
    parts.tray = tray;
    let core: Arc<dyn halvo_coord_app::Core> = core;
    let builder = mock_builder()
        .manage(GuardCalls::default())
        .manage(halvo_coord_app::Agents(core));
    let mut app = app::build(builder, parts)
        .build(app::context())
        .expect("the app builds on the mock runtime");
    first_iteration_runs_the_set_up(&mut app);
    TestApp {
        app,
        daemon: connector,
    }
}

/// The hooks of the app's Core from its own poller and window (contracts I10, ruling RD-74
/// 3.6), on the mock runtime: a tick at the start, off the thread that ran the set-up; a tick
/// at each pass of the poller, also after a tick that panicked; `shown` at a show, before the
/// page hears of it; `hidden` where the close handler hides the window; and `destroyed` where
/// the app destroys it. The mock runtime fires no close request and runs no script, so the test
/// calls the function that the close handler calls and reads the order of the show in tray.rs.
#[test]
fn the_app_calls_the_hooks_of_its_core_from_its_poller_and_its_window() {
    let set_up = std::thread::current().id();
    let core = Recorder::new(&[2]);
    let test = app_with_core("hooks", core.clone(), guard_installed, None);
    core.wait_for("tick", 1);
    assert_eq!(core.names(), ["tick"], "the start");
    assert!(
        core.calls().iter().all(|(_, thread)| *thread != set_up),
        "the tick of the start runs off the thread of the set-up"
    );
    let state = test.app.state::<AppState>();
    poller::round(state.control.as_ref(), &state.views, None);
    poller::round(state.control.as_ref(), &state.views, None);
    assert_eq!(
        core.names(),
        ["tick", "tick", "tick"],
        "a tick at each pass of the poller, also after a tick that panicked"
    );
    tray::show_main_window(test.app.handle());
    assert_eq!(core.names().last(), Some(&"shown"));
    let tray_rs = std::fs::read_to_string(src_tauri().join("src/tray.rs"))
        .expect("tray.rs reads")
        .replace("\r\n", "\n");
    let show = tray_rs
        .find("pub fn show_main_window<")
        .expect("show_main_window");
    let shown = tray_rs[show..]
        .find("core.shown()")
        .expect("show_main_window calls Core::shown");
    let heard = tray_rs[show..]
        .find("window.eval(crate::app::WINDOW_SHOWN)")
        .expect("show_main_window tells the page");
    assert!(
        shown < heard,
        "Core::shown comes before the page hears of the show"
    );
    app::hide_on_close(&main_window(&test.app).as_ref().window());
    assert_eq!(core.names().last(), Some(&"hidden"));
    let without_window = Recorder::new(&[]);
    let _app = app_with_core(
        "hooks-destroyed",
        without_window.clone(),
        guard_refused,
        Some(tray_stand_in),
    );
    assert!(
        without_window.names().contains(&"destroyed"),
        "{:?}",
        without_window.names()
    );
}

#[test]
fn a_reopen_shows_the_hidden_window_main_as_the_trays_item_does_and_without_one_shows_nothing() {
    let core = Recorder::new(&[]);
    let test = app_with_core("reopen", core.clone(), guard_installed, None);
    core.wait_for("tick", 1);
    app::hide_on_close(&main_window(&test.app).as_ref().window());
    assert_eq!(core.names(), ["tick", "hidden"]);
    tray::show_main_window(test.app.handle());
    assert_eq!(core.names(), ["tick", "hidden", "shown"]);
    let without_window = Recorder::new(&[]);
    let bare_core: Arc<dyn halvo_coord_app::Core> = without_window.clone();
    let bare = mock_builder()
        .manage(halvo_coord_app::Agents(bare_core))
        .build(app::context())
        .expect("the app builds on the mock runtime");
    assert!(bare.get_webview_window("main").is_none());
    tray::show_main_window(bare.handle());
    assert!(
        without_window.names().is_empty(),
        "{:?}",
        without_window.names()
    );
    let main_rs = std::fs::read_to_string(src_tauri().join("src/main.rs"))
        .expect("main.rs reads")
        .replace("\r\n", "\n");
    assert!(main_rs.contains(
        "    #[cfg(target_os = \"macos\")]\n    app.run(|app, event| {\n        if let tauri::RunEvent::Reopen { .. } = event {\n            tray::show_main_window(app);\n        }\n    });\n    #[cfg(not(target_os = \"macos\"))]\n    app.run(|_, _| {});\n}\n"
    ));
    assert_eq!(main_rs.matches("RunEvent::").count(), 1);
    assert_eq!(main_rs.matches("app.run(").count(), 2);
    assert!(!main_rs.contains("has_visible_windows"));
}

/// The tray's Quit (I5.7, ruling RD-8): it calls `Core::quit` once, off the main thread and with
/// no budget, before it asks for the exit, and no tick and no other call of the Core follows it.
/// The mock runtime cannot exit, so the test hands the Quit an exit that records.
#[test]
fn the_trays_quit_calls_the_quit_hook_once_before_the_exit_and_nothing_after_it() {
    let set_up = std::thread::current().id();
    let core = Recorder::new(&[]);
    let test = app_with_core("quit", core.clone(), guard_installed, None);
    core.wait_for("tick", 1);
    let (sender, exits) = std::sync::mpsc::channel();
    let seen = core.clone();
    let again = sender.clone();
    tray::quit(test.app.handle(), move |_| {
        let _ = sender.send(seen.calls());
    });
    let at_exit = exits
        .recv_timeout(Duration::from_secs(20))
        .expect("the Quit asks for the exit");
    let names: Vec<&str> = at_exit.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names.iter().filter(|name| name.starts_with("quit")).count(),
        1,
        "{names:?}"
    );
    assert_eq!(
        names.last(),
        Some(&"quit"),
        "the quit hook, with no budget, before the exit"
    );
    assert!(
        at_exit
            .iter()
            .all(|(name, thread)| *name != "quit" || *thread != set_up),
        "the quit hook runs off the main thread"
    );
    let state = test.app.state::<AppState>();
    poller::round(state.control.as_ref(), &state.views, None);
    tray::show_main_window(test.app.handle());
    app::hide_on_close(&main_window(&test.app).as_ref().window());
    tray::quit(test.app.handle(), move |_| {
        let _ = again.send(Vec::new());
    });
    assert!(
        exits.recv_timeout(Duration::from_millis(500)).is_err(),
        "a second Quit asks for no exit"
    );
    assert_eq!(
        core.names(),
        names,
        "no call of the Core follows the quit hook"
    );
}

/// Once the tray's Quit has begun, every command of the page gets the fixed reply that the app's
/// `NoService` gives it and reaches no call of the Core, while `Core::quit` runs and after it has
/// returned.
#[test]
fn once_the_trays_quit_has_begun_every_command_of_the_page_gets_its_fixed_reply_and_no_call_of_the_core(
) {
    let (release, held) = std::sync::mpsc::channel();
    let core = Recorder::holding_quit(held);
    let test = app_with_core("quit-commands", core.clone(), guard_installed, None);
    core.wait_for("tick", 1);
    let window = main_window(&test.app);
    let check = |moment: &str| {
        for command in halvo_coord::types::commands::LIST.iter().map(|c| c.name) {
            let fixed = serde_json::to_value(halvo_coord_app::Core::call(
                &commands::agents::NoService,
                command,
                None,
            ))
            .expect("a reply is JSON");
            assert_eq!(
                invoke(&window, APP_URL, command, serde_json::json!({})),
                Ok(fixed),
                "{command} {moment}"
            );
        }
    };
    let (sender, exits) = std::sync::mpsc::channel();
    tray::quit(test.app.handle(), move |_| {
        let _ = sender.send(());
    });
    core.wait_for("quit", 1);
    check("while Core::quit runs");
    release
        .send(())
        .expect("the quit hook waits for the release");
    exits
        .recv_timeout(Duration::from_secs(20))
        .expect("the Quit asks for the exit");
    check("after Core::quit has returned");
    assert!(
        !core.names().contains(&"call"),
        "a command reached the Core: {:?}",
        core.names()
    );
}
