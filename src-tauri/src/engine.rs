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
pub const GUARD_WORLD: &str = "halvo-guard";

#[cfg(any(target_os = "macos", test))]
pub const PEER_READING: &str = "['RTCPeerConnection', 'webkitRTCPeerConnection'].filter(function (name) { return name in window; }).map(function (name) { return name + ':' + typeof window[name]; }).join(',') || 'absent'";

#[cfg(any(target_os = "macos", test))]
pub const PEER_ABSENT: &str = "absent";

#[cfg(any(target_os = "macos", test))]
pub const LINK_PRECONNECT_FEATURE: &str = "LinkPreconnect";

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

/// Installs every engine means of the window main and answers once on `answer`:
/// `GuardInstalled` only when each means answered Ok, an error otherwise, and no
/// answer when the runtime drops the closure of with_webview, which the receiver
/// reads as a closed channel. On Windows the means are a handler of every resource
/// request that refuses each address but the two of `REQUEST_HOSTS`, and the two
/// navigation handlers, beside `app::ENGINE_ARGS` at creation; on macOS the
/// navigation delegate in front of wry's, the data store's proxy at no address,
/// the engine's peer-connection and link-preconnect preferences turned off and
/// read back, and the content rule list `RULE_LIST`, compiled into
/// `site.rules_dir`.
///
/// Hosted run 43541710304, with these means in and the content policy, the
/// runtime's rule and the script of every frame out: on Windows (job
/// 132283525539) and on macOS (job 132283525260) the window opened 0
/// connections, sent 0 requests, made 0 lookups and sent 0 datagrams for every
/// step of rulings AP-9 and AP-10 whose control reached the job's listeners. That
/// holds for a peer connection whose remote description carries a marked fragment
/// (with no layer 230 datagrams that carried the marker on Windows and 247 on
/// macOS, log lines 5205 and 11003) and on macOS for a link clicked by script, a
/// link with a target and a download link to an address in numbers (with no
/// layer 1 connection each, log lines 10428, 10483 and 10703). Of frames on macOS
/// the run shows this alone: a frame of each of the three addresses opened 0
/// connections with these means in and the content policy and the runtime's rule
/// out (log lines 10636, 10647 and 10658; with no layer 1 each).
pub fn guard<R: Runtime>(window: &WebviewWindow<R>, site: EngineSite, answer: Sender<GuardAnswer>) {
    let unreached = answer.clone();
    let asked = window.with_webview(move |webview| platform::install(&webview, &site, answer));
    if let Err(e) = asked {
        let _ = unreached.send(Err(format!("the engine cannot be reached: {e}")));
    }
}

#[cfg(target_os = "macos")]
pub use platform::configuration_at_creation;

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
        // SAFETY: `args` is the live COM object of an event that runs on the window's thread;
        // on success Uri writes a string allocated with CoTaskMemAlloc that this caller owns, and
        // take_pwstr frees it once with CoTaskMemFree; on an error `uri` stays null and is not read.
        unsafe { args.Uri(&mut uri) }.ok()?;
        Some(take_pwstr(uri))
    }

    fn install_every_means(webview: &PlatformWebview) -> windows_core::Result<()> {
        // SAFETY: wry keeps the controller alive as long as the web view, and with_webview hands
        // it to this closure on the thread that created it, the only thread WebView2 lets call it;
        // the returned interface holds its own reference.
        let core: ICoreWebView2 = unsafe { webview.controller().CoreWebView2() }?;
        let navigations = NavigationStartingEventHandler::create(Box::new(|_, args| {
            let Some(args) = args else {
                return Err(E_POINTER.into());
            };
            let allowed = navigation_address(&args).is_some_and(|text| super::engine_allows(&text));
            if !allowed {
                // SAFETY: WebView2 keeps `args` alive until this handler returns, on the window's
                // thread, and reads the flag only after the return.
                unsafe { args.SetCancel(true) }?;
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: on the window's thread; WebView2 takes its own reference to the handler and
        // keeps it while the registration stands, and `token` is a local that outlives the call.
        unsafe { core.add_NavigationStarting(&navigations, &mut token) }?;
        let mut frame_token = 0i64;
        // SAFETY: as for add_NavigationStarting: the window's thread, a handler that WebView2
        // references for itself, and a local token that outlives the call.
        unsafe { core.add_FrameNavigationStarting(&navigations, &mut frame_token) }?;

        let with_kinds: ICoreWebView2_22 = core.cast()?;
        // SAFETY: on the window's thread, on an interface that holds its reference to the web
        // view; the environment comes back with a reference of its own.
        let environment = unsafe { core.cast::<ICoreWebView2_2>()?.Environment() }?;
        // SAFETY: on the window's thread; the filter string is an HSTRING that lives through the
        // call, and WebView2 copies it.
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
            // SAFETY: WebView2 keeps `args` alive until this handler returns, on the window's
            // thread; the request comes back with a reference of its own.
            let request = unsafe { args.Request() }?;
            let mut uri = PWSTR::null();
            // SAFETY: `request` is a live COM object on the window's thread; on success Uri writes
            // a CoTaskMemAlloc string that take_pwstr frees once; on an error `uri` is not read.
            let address = unsafe { request.Uri(&mut uri) }
                .ok()
                .map(|()| take_pwstr(uri));
            if !address.is_some_and(|text| super::request_allowed(&text)) {
                // SAFETY: on the window's thread, the environment that created this web view as
                // WebView2 requires of a response; no stream, and two HSTRINGs that live through
                // the call.
                let refusal = unsafe {
                    environment.CreateWebResourceResponse(
                        None,
                        403,
                        &HSTRING::from("Forbidden"),
                        &HSTRING::new(),
                    )
                }?;
                // SAFETY: inside the handler, while WebView2 keeps `args` alive; it takes its own
                // reference to `refusal`.
                unsafe { args.SetResponse(&refusal) }?;
            }
            Ok(())
        }));
        let mut request_token = 0i64;
        // SAFETY: on the window's thread; WebView2 takes its own reference to the handler and
        // keeps it while the registration stands, and `request_token` is a local that outlives
        // the call.
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
    use objc2::{
        define_class, msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly,
    };
    use objc2_foundation::{NSArray, NSError, NSString, NSURL};
    use objc2_web_kit::{
        WKContentRuleList, WKContentRuleListStore, WKContentWorld, WKNavigationAction,
        WKNavigationActionPolicy, WKNavigationDelegate, WKPreferences, WKWebView,
        WKWebViewConfiguration, WKWebsiteDataStore,
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
                // SAFETY: WebKit calls this method on the main thread and keeps `action` alive for
                // the call; the request comes back retained.
                let address = unsafe { action.request() }
                    .URL()
                    .and_then(|url| url.absoluteString())
                    .map(|text| text.to_string());
                if !address.is_some_and(|text| super::engine_allows(&text)) {
                    handler.call((WKNavigationActionPolicy::Cancel,));
                    return;
                }
                // SAFETY: `inner` is wry's navigation delegate, retained by the ivars, whose class
                // implements this method; the arguments are WebKit's own, alive for this call on
                // the main thread, and `handler` is called once, by wry.
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
                // SAFETY: `inner` is an NSObject retained by the ivars; respondsToSelector: of
                // NSObject takes a SEL and returns BOOL, the types of this send.
                unsafe { msg_send![inner, respondsToSelector: selector] }
            }

            #[unsafe(method(forwardingTargetForSelector:))]
            fn forward(&self, _selector: Sel) -> *mut AnyObject {
                Retained::as_ptr(&self.ivars().inner) as *mut AnyObject
            }
        }
    );

    pub fn configuration_at_creation() -> Option<Retained<WKWebViewConfiguration>> {
        let mtm = MainThreadMarker::new()?;
        // SAFETY: on the main thread, which `mtm` proves; both come back retained.
        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        // SAFETY: on the main thread; the store that keeps nothing comes back retained and the
        // configuration retains it.
        unsafe { config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm)) };
        if answers(&config, sel!(_setAllowedNetworkHosts:)) {
            // SAFETY: +[NSSet set] takes nothing and returns an autoreleased empty set, which
            // Retained retains.
            let none: Option<Retained<AnyObject>> = unsafe { msg_send![objc2::class!(NSSet), set] };
            if let Some(none) = none {
                // SAFETY: the configuration answers _setAllowedNetworkHosts: (checked above),
                // which takes an NSSet of NSString and returns void; the configuration copies it.
                let _: () = unsafe { msg_send![&*config, _setAllowedNetworkHosts: &*none] };
            }
        }
        Some(config)
    }

    fn read_the_store_and_the_hosts(view: &WKWebView) -> Result<(), String> {
        // SAFETY: on the main thread with `view` retained; the copy of the configuration shares
        // the page's data store and carries its allowed hosts, and both come back retained.
        let config = unsafe { view.configuration() };
        // SAFETY: the same, for the data store of that configuration.
        let store = unsafe { config.websiteDataStore() };
        // SAFETY: a property read of the retained store on the main thread.
        if unsafe { store.isPersistent() } {
            return Err("the window's data store keeps data between starts".to_string());
        }
        if !answers(&config, sel!(_allowedNetworkHosts)) {
            return Err(
                "the configuration of this system has no allowed-hosts setting".to_string(),
            );
        }
        // SAFETY: the configuration answers _allowedNetworkHosts (checked above), which takes
        // nothing and returns an NSSet or nil, retained here.
        let hosts: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*config, _allowedNetworkHosts] };
        let hosts = hosts.ok_or("the window allows every network host")?;
        // SAFETY: an NSSet answers count, which takes nothing and returns NSUInteger.
        let count: usize = unsafe { msg_send![&*hosts, count] };
        if count != 0 {
            return Err(format!("the window allows {count} network hosts"));
        }
        Ok(())
    }

    fn put_delegate_in_front(view: &WKWebView, mtm: MainThreadMarker) -> Result<(), String> {
        // SAFETY: `view` is retained and this runs on the main thread, which `mtm` proves and
        // WebKit requires; the delegate comes back retained, so the ivars own it.
        let inner = unsafe { view.navigationDelegate() }.ok_or("no navigation delegate")?;
        let guard = Guard::alloc(mtm).set_ivars(Ivars { inner });
        // SAFETY: `guard` is an allocation of this class with its ivars set, and NSObject's init
        // is the designated initializer of its superclass; the result is +1, which Retained owns.
        let guard: Retained<Guard> = unsafe { msg_send![super(guard), init] };
        // SAFETY: on the main thread with `view` retained; WebKit holds the delegate weakly, so the
        // mem::forget below keeps `guard`, and the inner delegate it retains, alive for the
        // life of the process.
        unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*guard))) };
        std::mem::forget(guard);
        Ok(())
    }

    fn set_the_store_proxy(view: &WKWebView) -> Result<(), String> {
        // SAFETY: on the main thread with `view` retained; both reads come back retained.
        let store = unsafe { view.configuration().websiteDataStore() };
        // SAFETY: `store` is a retained NSObject; respondsToSelector: takes a SEL and returns
        // BOOL, the types of this send.
        let has_setting: bool =
            unsafe { msg_send![&*store, respondsToSelector: sel!(setProxyConfigurations:)] };
        if !has_setting {
            return Err("the data store of this system has no proxy setting".to_string());
        }
        let host = CString::new(super::PROXY_HOST).map_err(|e| e.to_string())?;
        let port = CString::new(super::PROXY_PORT).map_err(|e| e.to_string())?;
        // SAFETY: both arguments are NUL-terminated strings owned by `host` and `port`, alive
        // through the call; Network copies them and returns a +1 object or null.
        let endpoint = unsafe { nw_endpoint_create_host(host.as_ptr(), port.as_ptr()) };
        // SAFETY: the pointer is the +1 result of nw_endpoint_create_host, an Objective-C object
        // under OS_OBJECT_USE_OBJC, or null; Retained takes over that one reference.
        let endpoint = unsafe { Retained::from_raw(endpoint) }.ok_or("no proxy endpoint")?;
        // SAFETY: `endpoint` is retained through the call, and Network retains what it keeps;
        // the result is a +1 object or null.
        let config =
            unsafe { nw_proxy_config_create_socksv5(Retained::as_ptr(&endpoint).cast_mut()) };
        // SAFETY: the pointer is the +1 result of nw_proxy_config_create_socksv5 or null;
        // Retained takes over that one reference.
        let config = unsafe { Retained::from_raw(config) }.ok_or("no proxy configuration")?;
        let configs = NSArray::from_retained_slice(&[config]);
        // SAFETY: on the main thread; the store responds to setProxyConfigurations: (checked
        // above), whose argument is an NSArray of nw_proxy_config_t and which returns void; the
        // store copies the array.
        let _: () = unsafe { msg_send![&*store, setProxyConfigurations: &*configs] };
        Ok(())
    }

    fn answers(object: &AnyObject, selector: Sel) -> bool {
        // SAFETY: `object` is a live Objective-C object or class for the call; respondsToSelector:
        // of NSObject takes a SEL and returns BOOL.
        unsafe { msg_send![object, respondsToSelector: selector] }
    }

    fn feature_named(key: &str) -> Result<Retained<AnyObject>, String> {
        let class = WKPreferences::class();
        if !answers(class.as_ref(), sel!(_features)) {
            return Err("the preferences of this system list no features".to_string());
        }
        // SAFETY: +_features takes nothing and returns an autoreleased NSArray of _WKFeature,
        // which Retained retains.
        let features: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![class, _features] };
        let features = features.ok_or("no feature list")?;
        for i in 0..features.count() {
            let feature = features.objectAtIndex(i);
            if !answers(&feature, sel!(key)) {
                return Err("a feature of this system has no key".to_string());
            }
            // SAFETY: the feature answers key (checked above), which takes nothing and returns an
            // NSString owned by the feature.
            let name: Option<Retained<NSString>> = unsafe { msg_send![&*feature, key] };
            if name.is_some_and(|name| name.to_string() == key) {
                return Ok(feature);
            }
        }
        Err(format!(
            "the preferences of this system have no feature {key}"
        ))
    }

    fn set_feature(prefs: &WKPreferences, key: &str, on: bool) -> Result<(), String> {
        let feature = feature_named(key)?;
        if !answers(prefs, sel!(_setEnabled:forFeature:)) {
            return Err("the preferences of this system cannot set a feature".to_string());
        }
        if !answers(prefs, sel!(_isEnabledForFeature:)) {
            return Err("the preferences of this system cannot read a feature back".to_string());
        }
        // SAFETY: the preferences answer _setEnabled:forFeature: (checked above), which takes a
        // BOOL and a _WKFeature of the list and returns void, on the main thread.
        let _: () = unsafe { msg_send![prefs, _setEnabled: on, forFeature: &*feature] };
        // SAFETY: the preferences answer _isEnabledForFeature: (checked above), which takes the
        // same _WKFeature and returns BOOL.
        let now: bool = unsafe { msg_send![prefs, _isEnabledForFeature: &*feature] };
        if now != on {
            return Err(format!("the feature {key} does not read back as set"));
        }
        Ok(())
    }

    fn turn_off_engine_features(view: &WKWebView) -> Result<(), String> {
        // SAFETY: on the main thread with `view` retained; both reads come back retained, and the
        // preferences are the object that the page's settings follow (WebKit's configuration copy
        // shares it).
        let prefs = unsafe { view.configuration().preferences() };
        if !answers(&prefs, sel!(_setPeerConnectionEnabled:)) {
            return Err(
                "the preferences of this system have no peer-connection setting".to_string(),
            );
        }
        if !answers(&prefs, sel!(_peerConnectionEnabled)) {
            return Err(
                "the preferences of this system cannot read the peer-connection setting back"
                    .to_string(),
            );
        }
        // SAFETY: the preferences answer _setPeerConnectionEnabled: (checked above), which takes
        // a BOOL and returns void, on the main thread.
        let _: () = unsafe { msg_send![&*prefs, _setPeerConnectionEnabled: false] };
        // SAFETY: the preferences answer _peerConnectionEnabled (checked above), which takes
        // nothing and returns BOOL.
        let peer_on: bool = unsafe { msg_send![&*prefs, _peerConnectionEnabled] };
        if peer_on {
            return Err("peer connections stay on after the preference was set".to_string());
        }
        set_feature(&prefs, super::LINK_PRECONNECT_FEATURE, false)
    }

    fn read_the_peer_constructor(view: &WKWebView, answer: Sender<GuardAnswer>) {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = answer.send(Err("not on the main thread".to_string()));
            return;
        };
        // SAFETY: on the main thread, which `mtm` proves; the world comes back retained.
        let world =
            unsafe { WKContentWorld::worldWithName(&NSString::from_str(super::GUARD_WORLD), mtm) };
        let pending = Cell::new(Some(answer));
        let read = RcBlock::new(move |result: *mut AnyObject, error: *mut NSError| {
            let Some(answer) = pending.take() else {
                return;
            };
            // SAFETY: WebKit passes a result that it keeps alive for the call, or null.
            let text = unsafe { result.as_ref() }
                .and_then(|result| result.downcast_ref::<NSString>())
                .map(|text| text.to_string());
            let reading = match text {
                Some(text) if text == super::PEER_ABSENT => Ok(GuardInstalled(())),
                Some(text) => Err(format!(
                    "the engine offers the peer connection's constructor ({text})"
                )),
                None => {
                    // SAFETY: WebKit passes an error that it keeps alive for the call, or null.
                    let reason = unsafe { error.as_ref() }
                        .map(|error| error.localizedDescription().to_string())
                        .unwrap_or_else(|| "no answer".to_string());
                    Err(format!(
                        "the engine's peer connection could not be read at start: {reason}"
                    ))
                }
            };
            let _ = answer.send(reading);
        });
        // SAFETY: on the main thread with `view` retained; the script and the world live through
        // the call, and WebKit copies the block and calls it once on the main thread. The script
        // runs in a world of its own, where no script of the page or of the app runs.
        unsafe {
            view.evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(super::PEER_READING),
                None,
                &world,
                Some(&read),
            )
        };
    }

    pub fn install(webview: &PlatformWebview, site: &EngineSite, answer: Sender<GuardAnswer>) {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = answer.send(Err("not on the main thread".to_string()));
            return;
        };
        let pointer = webview.inner().cast::<WKWebView>();
        // SAFETY: wry hands with_webview's closure the pointer of the WKWebView that it owns,
        // alive while the closure runs on the main thread; retaining it here keeps the view alive
        // until the rule list's completion has used it.
        let Some(view) = (unsafe { Retained::retain(pointer) }) else {
            let _ = answer.send(Err("no web view".to_string()));
            return;
        };
        if let Err(e) = put_delegate_in_front(&view, mtm)
            .and_then(|()| set_the_store_proxy(&view))
            .and_then(|()| turn_off_engine_features(&view))
            .and_then(|()| read_the_store_and_the_hosts(&view))
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
        // SAFETY: on the main thread, which `mtm` proves; `directory` is a file URL that lives
        // through the call, and the store comes back retained.
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
            // SAFETY: WebKit passes a rule list that it keeps alive for the call, or null; the
            // reference does not leave the block.
            let Some(list) = (unsafe { list.as_ref() }) else {
                // SAFETY: WebKit passes an error that it keeps alive for the call, or null; only a
                // copy of its description leaves the block.
                let reason = unsafe { error.as_ref() }
                    .map(|error| error.localizedDescription().to_string())
                    .unwrap_or_else(|| "no rule list".to_string());
                let _ = answer.send(Err(format!("the rule list did not compile: {reason}")));
                return;
            };
            // SAFETY: WebKit runs the completion on the main thread; `view` is retained by the
            // block, and addContentRuleList retains `list`.
            unsafe {
                view.configuration()
                    .userContentController()
                    .addContentRuleList(list)
            };
            read_the_peer_constructor(&view, answer);
        });
        // SAFETY: on the main thread; the two strings live through the call, and WebKit copies
        // the completion block and calls it once on the main thread, which is the thread that
        // the block's non-Send contents require.
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

    fn src_file_with_lf_line_ends(name: &str) -> String {
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
        let forms = src_file_with_lf_line_ends("engine-url-forms.txt");
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
        assert_eq!(LINK_PRECONNECT_FEATURE, "LinkPreconnect");
        assert_eq!(GUARD_WORLD, "halvo-guard");
        assert_eq!(PEER_ABSENT, "absent");
        assert_eq!(
            PEER_READING,
            "['RTCPeerConnection', 'webkitRTCPeerConnection'].filter(function (name) { return name in window; }).map(function (name) { return name + ':' + typeof window[name]; }).join(',') || 'absent'"
        );
        assert_eq!((PROXY_HOST, PROXY_PORT), ("255.255.255.255", "9"));
    }

    #[test]
    fn the_window_main_refuses_every_download_and_gets_the_guard_of_its_engine() {
        let app = src_file_with_lf_line_ends("app.rs");
        assert_eq!(app.matches(".on_download(|_, _| false)").count(), 1);
        assert_eq!(
            app.matches("(parts.guard)(&window, site, sender)").count(),
            1
        );
        assert_eq!(app.matches("::from_config(app, &config)?").count(), 1);
        let main = src_file_with_lf_line_ends("main.rs");
        assert_eq!(main.matches("guard: engine::guard,").count(), 1);
        let check = src_file_with_lf_line_ends("check_page.rs");
        assert_eq!(check.matches("guard: engine::guard,").count(), 1);
    }

    #[test]
    fn on_windows_the_handlers_cancel_alone_and_refuse_every_request_off_the_list() {
        let source = src_file_with_lf_line_ends("engine.rs");
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
        let source = src_file_with_lf_line_ends("engine.rs");
        let macos = platform_module(&source, "#[cfg(target_os = \"macos\")]\nmod platform");
        assert_eq!(macos.matches("GuardInstalled(())").count(), 1);
        let added = macos
            .find(".addContentRuleList(list)")
            .expect("the list is added");
        let answered = macos
            .find("            read_the_peer_constructor(&view, answer);\n")
            .expect("the reading after the list");
        assert!(added < answered);
        for (text, count) in [
            (
                "compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(",
                1,
            ),
            (
                "put_delegate_in_front(&view, mtm)\n            .and_then(|()| set_the_store_proxy(&view))\n            .and_then(|()| turn_off_engine_features(&view))\n            .and_then(|()| read_the_store_and_the_hosts(&view))\n",
                1,
            ),
            ("answers(&config, sel!(_setAllowedNetworkHosts:))", 1),
            (
                "Some(text) if text == super::PEER_ABSENT => Ok(GuardInstalled(())),",
                1,
            ),
            ("let _ = answer.send(reading);", 1),
            ("evaluateJavaScript_inFrame_inContentWorld_completionHandler(", 1),
            ("WKContentWorld::worldWithName(&NSString::from_str(super::GUARD_WORLD), mtm)", 1),
            ("&NSString::from_str(super::PEER_READING),", 1),
            ("msg_send![&*config, _setAllowedNetworkHosts: &*none]", 1),
            ("WKWebsiteDataStore::nonPersistentDataStore(mtm)", 1),
            ("answers(&config, sel!(_allowedNetworkHosts))", 1),
            ("msg_send![&*config, _allowedNetworkHosts]", 1),
            ("if unsafe { store.isPersistent() } {", 1),
            ("if count != 0 {", 1),
            ("msg_send![&*prefs, _setPeerConnectionEnabled: false]", 1),
            ("msg_send![&*prefs, _peerConnectionEnabled]", 1),
            ("msg_send![class, _features]", 1),
            ("msg_send![&*feature, key]", 1),
            ("msg_send![prefs, _setEnabled: on, forFeature: &*feature]", 1),
            ("msg_send![prefs, _isEnabledForFeature: &*feature]", 1),
            ("answers(&prefs, sel!(_setPeerConnectionEnabled:))", 1),
            ("answers(&prefs, sel!(_peerConnectionEnabled))", 1),
            ("answers(class.as_ref(), sel!(_features))", 1),
            ("answers(&feature, sel!(key))", 1),
            ("answers(prefs, sel!(_setEnabled:forFeature:))", 1),
            ("answers(prefs, sel!(_isEnabledForFeature:))", 1),
            ("set_feature(&prefs, super::LINK_PRECONNECT_FEATURE, false)", 1),
            ("super::LINK_PRECONNECT_FEATURE", 1),
            ("super::engine_allows(", 1),
            ("handler.call((WKNavigationActionPolicy::Cancel,));", 1),
        ] {
            assert_eq!(macos.matches(text).count(), count, "{text}");
        }
    }
}
