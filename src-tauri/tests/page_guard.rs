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
};

#[allow(dead_code)]
#[path = "../src/app.rs"]
mod app;
#[allow(dead_code)]
#[path = "../src/commands/mod.rs"]
mod commands;
#[allow(dead_code)]
#[path = "../src/launch.rs"]
mod launch;
#[allow(dead_code)]
#[path = "../src/tray.rs"]
mod tray;

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
        #[allow(deprecated)]
        app.run_iteration(|_, _| {});
        Self { app, shreds }
    }

    fn main(&self) -> WebviewWindow<MockRuntime> {
        self.app
            .get_webview_window("main")
            .expect("the set-up made the window main")
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

fn unit_commands() -> Vec<&'static str> {
    commands::names::MODULES
        .iter()
        .flat_map(|module| module.commands.iter().copied())
        .collect()
}

#[test]
fn a_child_web_view_of_the_window_main_gets_no_command_of_the_unit_and_deletes_nothing() {
    let test = Guarded::new("child-web-view", app::context());
    let main = test.main();
    let child = WebviewBuilder::new("child", WebviewUrl::App("index.html".into()));
    #[cfg(windows)]
    let child = child.data_directory(scratch("child-web-view-data").join("webview"));
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
                "{command} from the child web view of the window main, Origin header {origin:?}"
            );
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
    assert!(
        !app::may_navigate(&page_address),
        "the rule allows {page_address}"
    );
    let test = Guarded::new("password", app::context());
    let main = test.main();
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
