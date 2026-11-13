use std::path::PathBuf;
use std::sync::mpsc::Sender;

use tauri::{Runtime, WebviewWindow};

/// Whether the engine may start a navigation to `address`, the string as the engine itself
/// holds it, before the runtime reads it: the navigation rule of the window main
/// (`app::may_navigate`), which refuses a string that the URL reader refuses. The runtime
/// alone allows such a string without asking the rule (tauri-runtime-wry 2.12.3 src/lib.rs
/// lines 4820 to 4826); the handlers of this module ask this function first.
pub fn engine_allows(address: &str) -> bool {
    crate::app::may_navigate(address)
}

#[cfg(any(windows, test))]
pub const REQUEST_HOSTS: [&str; 2] = ["tauri.localhost", "ipc.localhost"];

#[cfg(any(windows, test))]
pub fn request_allowed(address: &str) -> bool {
    tauri::Url::parse(address).is_ok_and(|url| {
        url.scheme() == "http"
            && url
                .host_str()
                .is_some_and(|host| REQUEST_HOSTS.contains(&host))
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

#[cfg(any(target_os = "macos", test))]
pub const RULE_LIST_ID: &str = "halvo-window-main";

#[cfg(any(target_os = "macos", test))]
pub const RULE_LIST: &str = r#"[{"trigger":{"url-filter":".*"},"action":{"type":"block"}},{"trigger":{"url-filter":"^tauri://localhost/"},"action":{"type":"ignore-previous-rules"}},{"trigger":{"url-filter":"^ipc://localhost/"},"action":{"type":"ignore-previous-rules"}}]"#;

#[cfg(any(target_os = "macos", test))]
pub const PROXY_HOST: &str = "255.255.255.255";
#[cfg(any(target_os = "macos", test))]
pub const PROXY_PORT: &str = "9";

pub struct GuardInstalled(());

pub type GuardAnswer = Result<GuardInstalled, String>;

pub struct EngineSite {
    #[cfg_attr(
        not(target_os = "macos"),
        expect(
            dead_code,
            reason = "the rule list, which the directory holds, is of macOS alone"
        )
    )]
    pub rules_dir: PathBuf,
}

pub type GuardInstall<R> = fn(&WebviewWindow<R>, EngineSite, Sender<GuardAnswer>);

pub fn guard<R: Runtime>(window: &WebviewWindow<R>, site: EngineSite, answer: Sender<GuardAnswer>) {
    let unreached = answer.clone();
    let asked = window.with_webview(move |webview| platform::install(&webview, &site, answer));
    if let Err(e) = asked {
        let _ = unreached.send(Err(format!("the engine cannot be reached: {e}")));
    }
}

#[cfg(test)]
pub fn installed_for_tests() -> GuardInstalled {
    GuardInstalled(())
}

#[cfg(windows)]
mod platform {
    use std::sync::mpsc::Sender;

    use tauri::webview::PlatformWebview;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2NavigationStartingEventArgs, ICoreWebView2_2, ICoreWebView2_22,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
    };
    use webview2_com::{
        take_pwstr, NavigationStartingEventHandler, WebResourceRequestedEventHandler,
    };
    use windows_core::{Interface, HRESULT, HSTRING, PWSTR};

    use super::{EngineSite, GuardAnswer, GuardInstalled};

    const E_POINTER: HRESULT = HRESULT(0x8000_4003_u32 as i32);

    pub fn install(webview: &PlatformWebview, _site: &EngineSite, answer: Sender<GuardAnswer>) {
        let installed = install_every_means(webview)
            .map(|()| GuardInstalled(()))
            .map_err(|e| format!("{e}"));
        let _ = answer.send(installed);
    }

    fn navigation_address(args: &ICoreWebView2NavigationStartingEventArgs) -> Option<String> {
        let mut uri = PWSTR::null();
        // SAFETY: Uri writes a string that the engine allocated into `uri`, which take_pwstr
        // takes over and frees.
        unsafe { args.Uri(&mut uri) }.ok()?;
        Some(take_pwstr(uri))
    }

    fn install_every_means(webview: &PlatformWebview) -> windows_core::Result<()> {
        // SAFETY: CoreWebView2 reads a property of the controller that the runtime created and
        // still holds; the call runs on the thread of the window, where with_webview runs it.
        let core: ICoreWebView2 = unsafe { webview.controller().CoreWebView2() }?;
        let navigations = NavigationStartingEventHandler::create(Box::new(|_, args| {
            let Some(args) = args else {
                return Err(E_POINTER.into());
            };
            let allowed = navigation_address(&args).is_some_and(|text| super::engine_allows(&text));
            if !allowed {
                // SAFETY: SetCancel sets a flag of the event's arguments during the event.
                unsafe { args.SetCancel(true) }?;
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: the handler is a COM object that the engine holds for as long as it is
        // registered; the token is written once.
        unsafe { core.add_NavigationStarting(&navigations, &mut token) }?;
        let mut frame_token = 0i64;
        // SAFETY: the same handler for the navigations of the window's frames, which raise
        // FrameNavigationStarting and not NavigationStarting; the token is written once.
        unsafe { core.add_FrameNavigationStarting(&navigations, &mut frame_token) }?;

        let with_kinds: ICoreWebView2_22 = core.cast()?;
        // SAFETY: Environment reads a property of the web view that the runtime created.
        let environment = unsafe { core.cast::<ICoreWebView2_2>()?.Environment() }?;
        // SAFETY: a filter of every address for every context and every kind of source, added
        // on the thread of the window.
        unsafe {
            with_kinds.AddWebResourceRequestedFilterWithRequestSourceKinds(
                &HSTRING::from("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )
        }?;
        let requests = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else {
                return Err(E_POINTER.into());
            };
            // SAFETY: Request reads a property of the event's arguments during the event.
            let request = unsafe { args.Request() }?;
            let mut uri = PWSTR::null();
            // SAFETY: Uri writes a string that the engine allocated into `uri`, which
            // take_pwstr takes over and frees.
            let address = unsafe { request.Uri(&mut uri) }
                .ok()
                .map(|()| take_pwstr(uri));
            if !address.is_some_and(|text| super::request_allowed(&text)) {
                // SAFETY: a response with no body, made by the environment of this web view.
                let refusal = unsafe {
                    environment.CreateWebResourceResponse(
                        None,
                        403,
                        &HSTRING::from("Forbidden"),
                        &HSTRING::new(),
                    )
                }?;
                // SAFETY: SetResponse answers the request during the event.
                unsafe { args.SetResponse(&refusal) }?;
            }
            Ok(())
        }));
        let mut request_token = 0i64;
        // SAFETY: the handler is a COM object that the engine holds for as long as it is
        // registered; the token is written once.
        unsafe { core.add_WebResourceRequested(&requests, &mut request_token) }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::cell::Cell;
    use std::ffi::{c_char, CString};
    use std::sync::mpsc::Sender;

    use block2::{Block, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject, Sel};
    use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSArray, NSError, NSString, NSURL};
    use objc2_web_kit::{
        WKContentRuleList, WKContentRuleListStore, WKNavigationAction, WKNavigationActionPolicy,
        WKNavigationDelegate, WKWebView,
    };
    use tauri::webview::PlatformWebview;

    use super::{EngineSite, GuardAnswer, GuardInstalled};

    #[link(name = "Network", kind = "framework")]
    extern "C" {
        fn nw_endpoint_create_host(host: *const c_char, port: *const c_char) -> *mut NSObject;
        fn nw_proxy_config_create_socksv5(proxy_endpoint: *mut NSObject) -> *mut NSObject;
    }

    pub struct Ivars {
        inner: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "HalvoNavigationGuard"]
        #[ivars = Ivars]
        pub struct Guard;

        unsafe impl NSObjectProtocol for Guard {}

        unsafe impl WKNavigationDelegate for Guard {
            #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
            fn decide(
                &self,
                webview: &WKWebView,
                action: &WKNavigationAction,
                handler: &Block<dyn Fn(WKNavigationActionPolicy)>,
            ) {
                // SAFETY: the action and its request are the engine's for the duration of
                // this call.
                let address = unsafe { action.request() }
                    .URL()
                    .and_then(|url| url.absoluteString())
                    .map(|text| text.to_string());
                if !address.is_some_and(|text| super::engine_allows(&text)) {
                    handler.call((WKNavigationActionPolicy::Cancel,));
                    return;
                }
                // SAFETY: the inner delegate is the runtime's, which implements this method,
                // and gets the engine's own arguments.
                unsafe {
                    self.ivars()
                        .inner
                        .webView_decidePolicyForNavigationAction_decisionHandler(
                            webview, action, handler,
                        )
                }
            }
        }

        impl Guard {
            #[unsafe(method(respondsToSelector:))]
            fn responds(&self, selector: Sel) -> Bool {
                if selector == sel!(webView:decidePolicyForNavigationAction:decisionHandler:) {
                    return Bool::YES;
                }
                let inner: &ProtocolObject<dyn WKNavigationDelegate> = &self.ivars().inner;
                // SAFETY: respondsToSelector: takes a selector and answers a bool.
                unsafe { msg_send![inner, respondsToSelector: selector] }
            }

            #[unsafe(method(forwardingTargetForSelector:))]
            fn forward(&self, _selector: Sel) -> *mut AnyObject {
                Retained::as_ptr(&self.ivars().inner) as *mut AnyObject
            }
        }
    );

    fn put_delegate_in_front(view: &WKWebView, mtm: MainThreadMarker) -> Result<(), String> {
        // SAFETY: a property read of the web view on the main thread.
        let inner = unsafe { view.navigationDelegate() }.ok_or("no navigation delegate")?;
        let guard = Guard::alloc(mtm).set_ivars(Ivars { inner });
        // SAFETY: init of NSObject on an allocated instance of a subclass with its ivars set.
        let guard: Retained<Guard> = unsafe { msg_send![super(guard), init] };
        // SAFETY: a property write of the web view on the main thread; the delegate is weak,
        // so the guard is kept for the life of the process below.
        unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*guard))) };
        std::mem::forget(guard);
        Ok(())
    }

    fn set_the_store_proxy(view: &WKWebView) -> Result<(), String> {
        // SAFETY: property reads of the web view's configuration on the main thread.
        let store = unsafe { view.configuration().websiteDataStore() };
        // SAFETY: respondsToSelector: takes a selector and answers a bool.
        let has_setting: bool =
            unsafe { msg_send![&*store, respondsToSelector: sel!(setProxyConfigurations:)] };
        if !has_setting {
            return Err("the data store of this system has no proxy setting".to_string());
        }
        let host = CString::new(super::PROXY_HOST).map_err(|e| e.to_string())?;
        let port = CString::new(super::PROXY_PORT).map_err(|e| e.to_string())?;
        // SAFETY: nw_endpoint_create_host takes two C strings that live through the call and
        // answers a retained endpoint or null.
        let endpoint = unsafe { nw_endpoint_create_host(host.as_ptr(), port.as_ptr()) };
        // SAFETY: the endpoint is a retained object of the Network framework or null.
        let endpoint = unsafe { Retained::from_raw(endpoint) }.ok_or("no proxy endpoint")?;
        // SAFETY: nw_proxy_config_create_socksv5 takes a live endpoint and answers a retained
        // configuration or null.
        let config =
            unsafe { nw_proxy_config_create_socksv5(Retained::as_ptr(&endpoint).cast_mut()) };
        // SAFETY: the configuration is a retained object of the Network framework or null.
        let config = unsafe { Retained::from_raw(config) }.ok_or("no proxy configuration")?;
        let configs = NSArray::from_retained_slice(&[config]);
        // SAFETY: the store answers setProxyConfigurations: (read above), which takes an array
        // of proxy configurations.
        let _: () = unsafe { msg_send![&*store, setProxyConfigurations: &*configs] };
        Ok(())
    }

    pub fn install(webview: &PlatformWebview, site: &EngineSite, answer: Sender<GuardAnswer>) {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = answer.send(Err("not on the main thread".to_string()));
            return;
        };
        let pointer = webview.inner().cast::<WKWebView>();
        // SAFETY: the runtime hands the pointer of its WKWebView, alive while the closure of
        // with_webview runs on the main thread; retaining it keeps it for the list's answer.
        let Some(view) = (unsafe { Retained::retain(pointer) }) else {
            let _ = answer.send(Err("no web view".to_string()));
            return;
        };
        if let Err(e) = put_delegate_in_front(&view, mtm).and_then(|()| set_the_store_proxy(&view))
        {
            let _ = answer.send(Err(e));
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&site.rules_dir) {
            let _ = answer.send(Err(format!(
                "the directory of the rule list cannot be made: {e}"
            )));
            return;
        }
        let directory = NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(&site.rules_dir.to_string_lossy()),
            true,
        );
        // SAFETY: a store of compiled rule lists in a directory of the app, on the main thread.
        let Some(store) = (unsafe { WKContentRuleListStore::storeWithURL(Some(&directory), mtm) })
        else {
            let _ = answer.send(Err("no store of rule lists".to_string()));
            return;
        };
        let pending = Cell::new(Some((answer, view)));
        let compiled = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
            let Some((answer, view)) = pending.take() else {
                return;
            };
            // SAFETY: the engine hands a live rule list or null for the duration of the call.
            let Some(list) = (unsafe { list.as_ref() }) else {
                // SAFETY: the engine hands a live error or null for the duration of the call.
                let reason = unsafe { error.as_ref() }
                    .map(|error| error.localizedDescription().to_string())
                    .unwrap_or_else(|| "no rule list".to_string());
                let _ = answer.send(Err(format!("the rule list did not compile: {reason}")));
                return;
            };
            // SAFETY: the content controller of the live web view's configuration, on the main
            // thread, where the store answers.
            unsafe {
                view.configuration()
                    .userContentController()
                    .addContentRuleList(list)
            };
            let _ = answer.send(Ok(GuardInstalled(())));
        });
        // SAFETY: the store compiles the list and answers the block once, on the main thread.
        unsafe {
            store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
                Some(&NSString::from_str(super::RULE_LIST_ID)),
                Some(&NSString::from_str(super::RULE_LIST)),
                Some(&compiled),
            )
        };
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use std::sync::mpsc::Sender;

    use super::{EngineSite, GuardAnswer};

    pub fn install(
        _: &tauri::webview::PlatformWebview,
        _: &EngineSite,
        answer: Sender<GuardAnswer>,
    ) {
        let _ = answer.send(Err("no engine guard for this system".to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
            .replace("\r\n", "\n")
    }

    fn platform_module<'a>(source: &'a str, head: &str) -> &'a str {
        let start = source.find(head).expect("the module");
        let rest = &source[start + head.len()..];
        &rest[..rest.find("\n#[cfg(").expect("its end")]
    }

    #[test]
    fn the_engine_allows_the_app_origin_alone_and_refuses_a_string_that_the_reader_refuses() {
        let (scheme, host) = crate::app::APP_ORIGIN;
        assert!(engine_allows(&format!("{scheme}://{host}/")));
        assert!(engine_allows(&format!("{scheme}://{host}/index.html")));
        for refused in [
            "http://xn--zz.localhost:48123/leak?d=x",
            "http://xn--zz.localhost:1/",
            "http://example.com/",
            "",
            "not an address",
            "javascript:alert(1)",
            "data:text/html,x",
            "blob:tauri://localhost/x",
            "about:blank",
        ] {
            assert!(!engine_allows(refused), "{refused}");
        }
    }

    #[test]
    fn every_engine_string_that_the_url_reader_refuses_is_refused_at_the_engine() {
        let forms = source("engine-url-forms.txt");
        let mut refused_by_reader = 0;
        for form in forms.lines() {
            if tauri::Url::parse(form).is_err() {
                refused_by_reader += 1;
                assert!(!engine_allows(form), "{form}");
                assert!(!request_allowed(form), "{form}");
            }
        }
        assert_eq!(refused_by_reader, 70);
    }

    #[test]
    fn the_request_hook_lets_the_two_app_hosts_alone_through() {
        assert_eq!(REQUEST_HOSTS, ["tauri.localhost", "ipc.localhost"]);
        for allowed in [
            "http://tauri.localhost/",
            "http://tauri.localhost/index.html",
            "http://ipc.localhost/capture_status",
            "http://TAURI.localhost/views/status.js",
        ] {
            assert!(request_allowed(allowed), "{allowed}");
        }
        for refused in [
            "https://tauri.localhost/",
            "http://tauri.localhost:8080/",
            "http://tauri.localhost:1/",
            "http://user@tauri.localhost/",
            "http://:p@ipc.localhost/",
            "http://tauri.localhost.example.test/",
            "http://example.test/?h=tauri.localhost",
            "http://localhost/",
            "http://127.0.0.1/",
            "http://10.1.2.3:48123/leak?d=x",
            "ws://ipc.localhost/",
            "tauri://localhost/",
            "ipc://localhost/",
            "about:blank",
            "",
            "data:text/html,x",
        ] {
            assert!(!request_allowed(refused), "{refused}");
        }
    }

    #[test]
    fn the_rule_list_blocks_every_load_but_the_two_app_schemes_at_localhost() {
        assert_eq!(
            RULE_LIST,
            r#"[{"trigger":{"url-filter":".*"},"action":{"type":"block"}},{"trigger":{"url-filter":"^tauri://localhost/"},"action":{"type":"ignore-previous-rules"}},{"trigger":{"url-filter":"^ipc://localhost/"},"action":{"type":"ignore-previous-rules"}}]"#
        );
        assert_eq!(RULE_LIST_ID, "halvo-window-main");
        assert_eq!((PROXY_HOST, PROXY_PORT), ("255.255.255.255", "9"));
    }

    #[test]
    fn the_window_main_refuses_every_download_and_gets_the_guard_of_its_engine() {
        let app = source("app.rs");
        assert_eq!(app.matches(".on_download(|_, _| false)").count(), 1);
        assert_eq!(
            app.matches("(parts.guard)(&window, site, sender)").count(),
            1
        );
        assert_eq!(app.matches("::from_config(app, &config)?").count(), 1);
        let main = source("main.rs");
        assert_eq!(main.matches("guard: engine::guard,").count(), 1);
        let check = source("check_page.rs");
        assert_eq!(check.matches("guard: engine::guard,").count(), 1);
    }

    #[test]
    fn on_windows_the_handlers_cancel_alone_and_refuse_every_request_off_the_list() {
        let source = source("engine.rs");
        let windows = platform_module(&source, "#[cfg(windows)]\nmod platform");
        for (text, count) in [
            ("core.add_NavigationStarting(&navigations,", 1),
            ("core.add_FrameNavigationStarting(&navigations,", 1),
            ("SetCancel(", 1),
            ("SetCancel(true)", 1),
            ("return Err(E_POINTER.into());", 2),
            ("Ok(())\n        }));", 2),
            ("AddWebResourceRequestedFilterWithRequestSourceKinds(", 1),
            ("&HSTRING::from(\"*\"),", 1),
            ("COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,", 2),
            ("COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,", 2),
            ("super::request_allowed(", 1),
            ("super::engine_allows(", 1),
            ("SetResponse(", 1),
            ("core.add_WebResourceRequested(&requests,", 1),
            ("GuardInstalled(())", 1),
        ] {
            assert_eq!(windows.matches(text).count(), count, "{text}");
        }
    }

    #[test]
    fn on_macos_the_install_answers_installed_only_after_the_rule_list_is_added() {
        let source = source("engine.rs");
        let macos = platform_module(&source, "#[cfg(target_os = \"macos\")]\nmod platform");
        assert_eq!(macos.matches("GuardInstalled(())").count(), 1);
        let added = macos
            .find(".addContentRuleList(list)")
            .expect("the list is added");
        let answered = macos
            .find("let _ = answer.send(Ok(GuardInstalled(())));")
            .expect("the answer");
        assert!(added < answered);
        for (text, count) in [
            (
                "compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(",
                1,
            ),
            (
                "put_delegate_in_front(&view, mtm).and_then(|()| set_the_store_proxy(&view))",
                1,
            ),
            ("super::engine_allows(", 1),
            ("handler.call((WKNavigationActionPolicy::Cancel,));", 1),
        ] {
            assert_eq!(macos.matches(text).count(), count, "{text}");
        }
    }
}
