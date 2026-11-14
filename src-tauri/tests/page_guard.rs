use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use halvo_capture::consent::onboarding::{GrantState, Permission, PermissionProbe, RequestOutcome};
use halvo_capture::consent::status::{Inventory, StoreInventory, UtcDay};
use halvo_capture::store::{SegmentExpiry, ShredReport, StoreResult};
use serde_json::Value;
use tauri::http::{HeaderMap, HeaderValue};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::{InvokeRequest, WebviewBuilder};
use tauri::{
    App, Context, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

#[expect(
    dead_code,
    reason = "this test crate compiles the app's app.rs by path and uses part of it: \
              WINDOW_SHOWN and PageLoads::count stay unused here; the app's own build lints the whole module"
)]
#[path = "../src/app.rs"]
mod app;
#[expect(
    dead_code,
    reason = "this test crate compiles the app's commands by path and uses part of them: \
              the fields module and reads of commands/names.rs stay unused here; the app's own build lints the whole module"
)]
#[path = "../src/commands/mod.rs"]
mod commands;
#[expect(
    dead_code,
    reason = "this test crate compiles the app's launch.rs by path, because tray.rs names its exit codes: \
              USAGE stays unused here; the app's own build lints the whole module"
)]
#[path = "../src/launch.rs"]
mod launch;
#[expect(
    dead_code,
    reason = "this test crate compiles the app's tray.rs by path, because the capture commands refresh the tray: \
              the four id constants, install, on_menu_event and show_main_window stay unused here; \
              the app's own build lints the whole module"
)]
#[path = "../src/tray.rs"]
mod tray;

const OTHER_LABELS: [&str; 2] = ["helper", "zq-label-of-this-test"];

fn widened_by_the_macro() -> Context<MockRuntime> {
    tauri::generate_context!(
        "tauri.conf.json",
        capabilities = ["src-tauri/tests/fixtures/widened-by-the-macro.json"],
        test = true
    )
}

fn scratch(name: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("halvo-page-guard");
    std::fs::create_dir_all(&base).expect("the scratch base can be made");
    for n in 0u32.. {
        let dir = base.join(format!("{name}-{}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return dir,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => panic!("cannot make {}: {e}", dir.display()),
        }
    }
    unreachable!("a free scratch name below u32::MAX")
}

#[derive(Clone, Default)]
struct CountingStore {
    shreds: Arc<AtomicU32>,
}

const NOTHING: ShredReport = ShredReport {
    segments_shredded: 0,
    keys_destroyed: 0,
};

impl SegmentExpiry for CountingStore {
    fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport> {
        Ok(NOTHING)
    }

    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport> {
        self.shreds.fetch_add(1, Ordering::SeqCst);
        Ok(ShredReport {
            segments_shredded: 1,
            keys_destroyed: 1,
        })
    }
}

impl StoreInventory for CountingStore {
    fn inventory(&self) -> StoreResult<Inventory> {
        Ok(Inventory::default())
    }

    fn crypto_shred_before(&mut self, _day: UtcDay) -> StoreResult<ShredReport> {
        Ok(NOTHING)
    }
}

struct GrantedProbe;

impl PermissionProbe for GrantedProbe {
    fn check(&self, _permission: Permission) -> GrantState {
        GrantState::Granted
    }

    fn request(&self, _permission: Permission) -> RequestOutcome {
        RequestOutcome::AlreadyGranted
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

fn first_iteration_runs_the_set_up(app: &mut App<MockRuntime>) {
    #[allow(deprecated)]
    app.run_iteration(|_, _| {});
}

struct Guarded {
    app: App<MockRuntime>,
    shreds: Arc<AtomicU32>,
}

impl Guarded {
    fn new(name: &str, context: Context<MockRuntime>) -> Self {
        let dir = scratch(name);
        let store = CountingStore::default();
        let shreds = store.shreds.clone();
        let parts = app::Parts {
            #[cfg(windows)]
            webview_data_dir: Some(dir.join("webview")),
            config_dir: Some(dir),
            store: Box::new(store),
            store_backend: "the counting store of the guard tests",
            probe: Box::new(GrantedProbe),
            open_pane: no_pane,
            tray: no_tray,
            window_visible: false,
        };
        let mut app = app::build(mock_builder(), parts)
            .build(context)
            .expect("the app builds on the mock runtime");
        first_iteration_runs_the_set_up(&mut app);
        Self { app, shreds }
    }

    fn main(&self) -> WebviewWindow<MockRuntime> {
        self.app
            .get_webview_window("main")
            .expect("the set-up made the window main")
    }

    fn window(&self, label: &str) -> WebviewWindow<MockRuntime> {
        let builder = WebviewWindowBuilder::new(
            self.app.handle(),
            label,
            WebviewUrl::App("index.html".into()),
        );
        #[cfg(windows)]
        let builder = builder.data_directory(scratch(&format!("window-{label}")).join("webview"));
        builder
            .build()
            .expect("a window builds on the mock runtime")
    }

    fn shreds(&self) -> u32 {
        self.shreds.load(Ordering::SeqCst)
    }
}

struct Caller(Webview<MockRuntime>);

impl AsRef<Webview<MockRuntime>> for Caller {
    fn as_ref(&self) -> &Webview<MockRuntime> {
        &self.0
    }
}

fn app_address(user_info: &str, path: &str) -> String {
    let (scheme, host) = app::APP_ORIGIN;
    format!("{scheme}://{user_info}{host}{path}")
}

fn app_origin() -> Option<HeaderValue> {
    Some(HeaderValue::from_str(&app_address("", "")).expect("an address that a header can hold"))
}

fn call(
    webview: &Webview<MockRuntime>,
    command: &str,
    origin: Option<HeaderValue>,
) -> Result<Value, Value> {
    let mut headers = HeaderMap::new();
    if let Some(origin) = origin {
        headers.insert("Origin", origin);
    }
    let request = InvokeRequest {
        cmd: command.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: app_address("", "").parse().expect("the app's address"),
        body: InvokeBody::Json(serde_json::json!({})),
        headers,
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(&Caller(webview.clone()), request)
        .map(|body| body.deserialize::<Value>().expect("the reply is JSON"))
}

fn refused() -> Result<Value, Value> {
    Err(Value::String(commands::REFUSED.to_string()))
}

const APP_RS: &str = include_str!("../src/app.rs");

fn arms_named_by_app_rs() -> [(String, String); 2] {
    let head = "pub const APP_ORIGIN: (&str, &str) = if cfg!(windows) {";
    let start = APP_RS.find(head).expect("app.rs defines APP_ORIGIN");
    let body = &APP_RS[start + head.len()..];
    let body = &body[..body.find("};").expect("the definition of APP_ORIGIN ends")];
    let quoted: Vec<&str> = body.split('"').skip(1).step_by(2).collect();
    let [windows_scheme, windows_host, other_scheme, other_host] = quoted[..] else {
        panic!("APP_ORIGIN names two pairs: {quoted:?}");
    };
    let windows = (windows_scheme.to_string(), windows_host.to_string());
    let other = (other_scheme.to_string(), other_host.to_string());
    let compiled = (app::APP_ORIGIN.0.to_string(), app::APP_ORIGIN.1.to_string());
    assert_eq!(
        if cfg!(windows) { &windows } else { &other },
        &compiled,
        "the text of APP_ORIGIN and the compiled constant"
    );
    [other, windows]
}

const C10_9_COMMANDS: [&str; 8] = [
    "capture_status",
    "inventory",
    "set_capture_state",
    "delete_all",
    "set_storage_cap",
    "onboarding_state",
    "request_permission",
    "open_permission_pane",
];

fn unit_commands() -> Vec<&'static str> {
    let commands: Vec<&'static str> = commands::names::MODULES
        .iter()
        .flat_map(|module| module.commands.iter().copied())
        .collect();
    for name in C10_9_COMMANDS {
        assert!(
            commands.contains(&name),
            "the commands of the app's modules lack {name}: {commands:?}"
        );
    }
    commands
}

#[test]
fn a_child_web_view_of_the_window_main_gets_no_command_of_the_unit_and_deletes_nothing() {
    let test = Guarded::new("child-web-view", app::context());
    let main = test.main();
    for label in ["child", "main-child"] {
        let child = WebviewBuilder::new(label, WebviewUrl::App("index.html".into()));
        #[cfg(windows)]
        let child = child.data_directory(scratch(&format!("{label}-data")).join("webview"));
        let child = main
            .as_ref()
            .window()
            .add_child(
                child,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(100.0, 100.0),
            )
            .expect("a child web view builds on the mock runtime");
        assert_eq!(child.window().label(), "main");
        assert!(
            child.url().is_ok_and(|url| app::is_app_url(&url)),
            "the child web view's own address is of the app's origin: {:?}",
            child.url()
        );
        for command in unit_commands() {
            for origin in [app_origin(), None] {
                assert_eq!(
                    call(&child, command, origin.clone()),
                    refused(),
                    "{command} from the child web view {label} of the window main, Origin header {origin:?}"
                );
            }
        }
    }
    assert_eq!(
        test.shreds(),
        0,
        "a call of the child web view deleted the data"
    );
    assert!(call(main.as_ref(), "delete_all", app_origin()).is_ok());
    assert_eq!(test.shreds(), 1, "the web view main deletes the data");
}

#[test]
fn a_call_whose_origin_header_is_no_address_is_refused() {
    let test = Guarded::new("origin-no-address", app::context());
    let main = test.main();
    let page = main.as_ref();
    assert!(call(page, "capture_status", app_origin()).is_ok());
    assert!(call(page, "capture_status", None).is_ok());
    let mut not_utf8 = app_address("", "/").into_bytes();
    not_utf8.push(0xff);
    let not_addresses = [
        HeaderValue::from_static("null"),
        HeaderValue::from_static(""),
        HeaderValue::from_static("not an address"),
        HeaderValue::from_static(app::APP_ORIGIN.1),
        HeaderValue::from_bytes(&not_utf8).expect("a header holds a byte above 0x7f"),
    ];
    for value in not_addresses {
        for command in unit_commands() {
            assert_eq!(
                call(page, command, Some(value.clone())),
                refused(),
                "{command} with the Origin header {value:?}"
            );
        }
    }
    assert_eq!(test.shreds(), 0);
}

#[test]
fn an_address_of_the_app_host_with_a_password_and_no_user_name_is_refused() {
    let origin = app_address(":secret@", "");
    let page_address = app_address(":secret@", "/index.html");
    let parsed: Url = origin.parse().expect("a URL");
    assert_eq!((parsed.username(), parsed.password()), ("", Some("secret")));
    assert!(!app::is_app_url(&parsed), "the rule allows {origin}");
    let arms = [("tauri", "localhost"), ("http", "tauri.localhost")];
    assert_eq!(
        arms_named_by_app_rs(),
        arms.map(|(scheme, host)| (scheme.to_string(), host.to_string())),
        "the arms that the test asks are those that APP_ORIGIN names in src-tauri/src/app.rs"
    );
    for (scheme, host) in arms {
        let arm: Url = format!("{scheme}://:secret@{host}/")
            .parse()
            .expect("a URL");
        assert!(
            !app::is_origin(&arm, (scheme, host)),
            "the arm {scheme}://{host} allows {arm}"
        );
    }
    assert!(
        !app::may_navigate(&page_address),
        "the rule allows {page_address}"
    );
    let test = Guarded::new("password", app::context());
    let main = test.main();
    assert!(call(main.as_ref(), "capture_status", app_origin()).is_ok());
    assert!(call(main.as_ref(), "capture_status", None).is_ok());
    let header = HeaderValue::from_str(&origin).expect("an address that a header can hold");
    for command in unit_commands() {
        assert_eq!(
            call(main.as_ref(), command, Some(header.clone())),
            refused(),
            "{command} with the Origin header {origin}"
        );
    }
    main.navigate(page_address.parse().expect("a URL"))
        .expect("the mock runtime records the address");
    for command in unit_commands() {
        assert_eq!(
            call(main.as_ref(), command, None),
            refused(),
            "{command} from the window main whose own address is {page_address}, with no Origin header"
        );
    }
    assert_eq!(test.shreds(), 0);
}

#[test]
fn the_web_view_main_moved_into_another_window_gets_no_command_of_the_unit_and_deletes_nothing() {
    let test = Guarded::new("moved-web-view", widened_by_the_macro());
    let main = test.main();
    let page = main.as_ref();
    let window_main = page.window();
    let helper = test.window(OTHER_LABELS[0]);
    page.reparent(&helper.as_ref().window())
        .expect("the mock runtime moves the web view main into the window helper");
    assert_eq!(page.label(), "main");
    assert_eq!(page.window().label(), OTHER_LABELS[0]);
    assert!(
        page.url().is_ok_and(|url| app::is_app_url(&url)),
        "the moved web view's own address is of the app's origin: {:?}",
        page.url()
    );
    assert_eq!(
        call(page, "plugin:app|version", app_origin()),
        Ok(Value::String(app::VERSION.to_string())),
        "the capability handed to the context macro reaches the window helper"
    );
    for command in unit_commands() {
        for origin in [app_origin(), None] {
            assert_eq!(
                call(page, command, origin.clone()),
                refused(),
                "{command} from the web view main in the window helper, Origin header {origin:?}"
            );
        }
    }
    assert_eq!(
        test.shreds(),
        0,
        "a call of the moved web view deleted the data"
    );
    page.reparent(&window_main)
        .expect("the mock runtime moves the web view main back");
    assert_eq!(page.window().label(), "main");
    assert!(call(page, "delete_all", app_origin()).is_ok());
    assert_eq!(
        test.shreds(),
        1,
        "the web view main in the window main deletes the data"
    );
}

#[test]
fn a_capability_handed_to_the_context_macro_reaches_no_command_of_the_unit_from_another_window() {
    let plain = Guarded::new("macro-plain", app::context());
    let widened = Guarded::new("macro-widened", widened_by_the_macro());
    let version = Ok(Value::String(app::VERSION.to_string()));
    for label in OTHER_LABELS {
        let unwidened = plain.window(label);
        assert_ne!(
            call(unwidened.as_ref(), "plugin:app|version", app_origin()),
            version,
            "the window {label} reads the version with no capability of its own"
        );
        let window = widened.window(label);
        assert_eq!(
            call(window.as_ref(), "plugin:app|version", app_origin()),
            version,
            "the capability handed to the context macro reaches the window {label}"
        );
        for command in unit_commands() {
            assert_eq!(
                call(window.as_ref(), command, app_origin()),
                refused(),
                "{command} from the window {label}, granted by the context macro"
            );
        }
    }
    assert_eq!(widened.shreds(), 0);
    assert!(call(widened.main().as_ref(), "delete_all", app_origin()).is_ok());
    assert_eq!(widened.shreds(), 1);
}
