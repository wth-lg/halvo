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

/// The decision of the engine's own navigation handlers (the Guard on macOS, the two
/// navigation handlers on Windows) for one navigation: only an address that the engine reported
/// and that `engine_allows` allows goes on; a navigation whose address cannot be read is refused.
#[cfg(any(windows, target_os = "macos", test))]
pub fn guard_allows(address: Option<&str>) -> bool {
    address.is_some_and(engine_allows)
}

#[cfg(any(windows, test))]
pub const REQUEST_HOSTS: [&str; 2] = ["tauri.localhost", "ipc.localhost"];

/// The decision of the Windows request hook for one request: only an address that the engine
/// reported and that `request_allowed` allows goes through; a request whose address cannot be
/// read gets the refusal.
#[cfg(any(windows, test))]
pub fn request_hook_allows(address: Option<&str>) -> bool {
    address.is_some_and(request_allowed)
}

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

/// The endpoint of the data store's proxy on macOS, as the two strings that Network takes: an
/// address that no program can listen on.
#[cfg(any(target_os = "macos", test))]
pub fn proxy_endpoint() -> Result<(std::ffi::CString, std::ffi::CString), NotInstalled> {
    let host = std::ffi::CString::new(PROXY_HOST).map_err(|e| e.to_string())?;
    let port = std::ffi::CString::new(PROXY_PORT).map_err(|e| e.to_string())?;
    Ok((host, port))
}

/// What the reading at start in a world of its own answers: the guard is installed only when the
/// script read the peer connection's constructors as absent.
#[cfg(any(target_os = "macos", test))]
pub fn peer_reading(text: Option<&str>, error: Option<String>) -> GuardAnswer {
    match text {
        Some(PEER_ABSENT) => Ok(GuardInstalled(())),
        Some(text) => Err(NotInstalled::on_this_system(format!(
            "the engine offers the peer connection's constructor ({text})"
        ))),
        None => Err(NotInstalled::from(format!(
            "the engine's peer connection could not be read at start: {}",
            error.unwrap_or_else(|| "no answer".to_string())
        ))),
    }
}

pub struct GuardInstalled(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotInstalled {
    pub reason: String,
    pub restart_cures: bool,
}

impl NotInstalled {
    #[cfg(any(not(windows), test))]
    pub fn on_this_system(reason: impl Into<String>) -> Self {
        NotInstalled {
            reason: reason.into(),
            restart_cures: false,
        }
    }
}

#[cfg(any(target_os = "macos", test))]
pub const DELEGATE_READS_NONE: &str = "the window's navigation delegate reads back as none";

#[cfg(any(target_os = "macos", test))]
pub const DELEGATE_READS_ANOTHER: &str =
    "the window's navigation delegate reads back as another object than the guard";

#[cfg(any(target_os = "macos", test))]
pub const GUARD_ANSWERS_OTHERWISE: &str =
    "the guard does not answer for the navigation selector alone";

#[cfg(any(target_os = "macos", test))]
pub const PROXY_NOT_READ_BACK: &str =
    "the data store of this system cannot read back its proxy setting";

#[cfg(any(target_os = "macos", test))]
pub fn store_keeps_data() -> NotInstalled {
    NotInstalled::on_this_system("the window's data store keeps data between starts")
}

#[cfg(any(target_os = "macos", test))]
pub fn every_network_host_allowed() -> NotInstalled {
    NotInstalled::on_this_system("the window allows every network host")
}

#[cfg(any(target_os = "macos", test))]
pub fn network_hosts_allowed(count: usize) -> NotInstalled {
    NotInstalled::on_this_system(format!("the window allows {count} network hosts"))
}

impl From<String> for NotInstalled {
    fn from(reason: String) -> Self {
        NotInstalled {
            reason,
            restart_cures: true,
        }
    }
}

impl From<&str> for NotInstalled {
    fn from(reason: &str) -> Self {
        NotInstalled::from(reason.to_string())
    }
}

pub type GuardAnswer = Result<GuardInstalled, NotInstalled>;

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
/// `GuardInstalled` only when each means answered Ok, `NotInstalled` otherwise,
/// and no answer when the runtime drops the closure of with_webview, which the
/// receiver reads as a closed channel. On Windows the means are a handler of every
/// resource request that refuses each address but the two of `REQUEST_HOSTS`, and
/// the two navigation handlers, beside `app::ENGINE_ARGS` at creation. On macOS
/// they are the navigation delegate in front of wry's, read back by one read after
/// it is put in front and before the load (the view's delegate is the Guard by
/// pointer, it answers yes for the navigation selector and no for the form of that
/// selector with preferences, and the store reads back one proxy configuration),
/// the data store's proxy at no address, the engine's peer-connection and link-preconnect preferences turned off
/// and read back, the content rule list `RULE_LIST` compiled into
/// `site.rules_dir`, the store that keeps nothing and the empty set of allowed
/// network hosts of the configuration handed in at creation, read on the built web
/// view, and a reading in a world of its own that the engine offers no
/// peer-connection constructor. Every private selector is asked for before it is
/// sent; a system that lacks one answers not installed.
///
/// Among the start arguments, `--proxy-bypass-list=<-loopback>` takes away the
/// proxy's implicit exceptions, which Chromium's net/docs/proxy.md ("Proxy support
/// in Chrome", sections "Implicit bypass rules" and "Overriding the implicit bypass
/// rules") lists as localhost, *.localhost, [::1], 127.0.0.1/8 and the link-local
/// ranges 169.254/16 and [FE80::]/10.
///
/// Hosted run 43632388796, for scripted steps, with these means in and the content
/// policy, the runtime's rule and the script of every frame out, beside a control
/// above 0 in the same job and with every listener bound. On windows-2025-vs2026
/// 20261103.298.1 (21 launches of the head), to the runner's own IPv4 address or a
/// name for it, a peer connection whose remote description carries a marked
/// fragment sent 0 datagrams, where with no layer it sent 241 that carried the
/// marker (job 132561673817, log lines 6779 and 6783). To tauri.localhost and
/// ipc.localhost with a port of the job, a WebSocket and a turn: address over TCP
/// opened 0 connections in the head, without the proxy and without
/// `--proxy-bypass-list`, where with no layer they opened 1 with 1 request and 2
/// with 4 requests (log lines 6602 to 6615 and 6941 to 6958). On macos-15-arm64
/// 20261104.0514.1 (21 launches of the head) the marked peer connection sent 0
/// datagrams, where with no layer it sent 248 (job 132561673633, log lines 7146 and
/// 7152); a link clicked by script, a link with a target and a download link opened
/// 0 connections to the runner's own IPv4 address, where with no layer each opened
/// 1 (log lines 6779 to 6931), and 0 to an IPv6 address off the runner, where with
/// no layer they opened 2, 1 and 2 (log lines 6799 to 6951); to an IPv4 address off
/// the runner the head's three cells were not measurable in that run (the packet
/// filter did not see a probe of the launch), and the engine-alone copy read 0
/// beside the same control; a frame opened 0 connections, and 1 with no layer (log
/// lines 6886 and 6889). The empty set of allowed network hosts refuses web
/// addresses of other hosts for page loads; it does not cover a datagram or the
/// connection of a turn: address over TCP. Not measured on macOS 14 or on a macOS
/// newer than 15.
pub fn guard<R: Runtime>(window: &WebviewWindow<R>, site: EngineSite, answer: Sender<GuardAnswer>) {
    let unreached = answer.clone();
    let asked = window.with_webview(move |webview| platform::install(&webview, &site, answer));
    if let Err(e) = asked {
        let _ = unreached.send(Err(NotInstalled::from(format!(
            "the engine cannot be reached: {e}"
        ))));
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

    use super::{EngineSite, GuardAnswer, GuardInstalled, NotInstalled};

    const E_POINTER: HRESULT = HRESULT(0x8000_4003_u32 as i32);

    pub fn install(webview: &PlatformWebview, _site: &EngineSite, answer: Sender<GuardAnswer>) {
        let installed = install_every_means(webview)
            .map(|()| GuardInstalled(()))
            .map_err(|e| NotInstalled::from(format!("{e}")));
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
            let allowed = super::guard_allows(navigation_address(&args).as_deref());
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
        // SAFETY: on the window's thread, the environment that created this web view as WebView2
        // requires of a response; no stream, and two HSTRINGs that live through the call. The
        // refusal is made once, here, so that no call that can fail stands between a request and
        // its refusal (ruling AP-17 G9).
        let refusal = unsafe {
            environment.CreateWebResourceResponse(
                None,
                403,
                &HSTRING::from("Forbidden"),
                &HSTRING::new(),
            )
        }?;
        let requests = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else {
                return Err(E_POINTER.into());
            };
            // SAFETY: WebView2 keeps `args` alive until this handler returns, on the window's
            // thread; the request comes back with a reference of its own. An error here leaves
            // no address, and the request gets the refusal.
            let address = unsafe { args.Request() }.ok().and_then(|request| {
                let mut uri = PWSTR::null();
                // SAFETY: `request` is a live COM object on the window's thread; on success Uri
                // writes a CoTaskMemAlloc string that take_pwstr frees once; on an error `uri` is
                // not read.
                unsafe { request.Uri(&mut uri) }
                    .ok()
                    .map(|()| take_pwstr(uri))
            });
            if !super::request_hook_allows(address.as_deref()) {
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
    use std::ffi::{c_char, CStr};
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

    use super::{EngineSite, GuardAnswer, NotInstalled};

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
                if !super::guard_allows(address.as_deref()) {
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
                if selector
                    == sel!(webView:decidePolicyForNavigationAction:preferences:decisionHandler:)
                {
                    return Bool::NO;
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

    /// What the install reads from the engine and sends to it after the view is built: the real
    /// web view through `Real`, or a test's stand-in, so that a test can make each read fail
    /// (ruling AP-17 G3).
    pub(super) trait Side {
        fn store_is_persistent(&self) -> bool;
        fn config_answers(&self, selector: Sel) -> bool;
        /// The count of the allowed network hosts, or None for no set; asked only after the
        /// configuration answered `_allowedNetworkHosts`.
        fn allowed_hosts_count(&self) -> Option<usize>;
        fn delegate(&self) -> Option<*const AnyObject>;
        fn delegate_answers(&self, selector: Sel) -> bool;
        fn store_answers(&self, selector: Sel) -> bool;
        /// The count of the data store's proxy configurations, or None for no list; asked only
        /// after the store answered `proxyConfigurations`.
        fn proxy_configuration_count(&self) -> Option<usize>;
        /// Sends one SOCKS proxy at `host` and `port` to the data store; called only after the
        /// store answered `setProxyConfigurations:`.
        fn set_proxy(&self, host: &CStr, port: &CStr) -> Result<(), NotInstalled>;
    }

    pub(super) struct Real<'a>(pub &'a WKWebView);

    impl Side for Real<'_> {
        fn store_is_persistent(&self) -> bool {
            // SAFETY: on the main thread with the view retained; the copy of the configuration
            // shares the page's data store, both come back retained, and isPersistent is a
            // property read.
            unsafe { self.0.configuration().websiteDataStore().isPersistent() }
        }

        fn config_answers(&self, selector: Sel) -> bool {
            // SAFETY: on the main thread with the view retained; the configuration comes back
            // retained.
            let config = unsafe { self.0.configuration() };
            answers(&config, selector)
        }

        fn allowed_hosts_count(&self) -> Option<usize> {
            // SAFETY: on the main thread with the view retained; the configuration comes back
            // retained.
            let config = unsafe { self.0.configuration() };
            // SAFETY: the callers ask only after the configuration answered _allowedNetworkHosts,
            // which takes nothing and returns an NSSet or nil, retained here.
            let hosts: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*config, _allowedNetworkHosts] };
            // SAFETY: the set answers count, which takes nothing and returns NSUInteger.
            hosts.map(|hosts| unsafe { msg_send![&*hosts, count] })
        }

        fn delegate(&self) -> Option<*const AnyObject> {
            // SAFETY: on the main thread with the view retained; the weak property comes back
            // retained or nil, and only its address leaves this function.
            unsafe { self.0.navigationDelegate() }.map(|delegate| {
                let object: &AnyObject = (*delegate).as_ref();
                std::ptr::from_ref(object)
            })
        }

        fn delegate_answers(&self, selector: Sel) -> bool {
            // SAFETY: as in `delegate`; the delegate stays retained for the question.
            unsafe { self.0.navigationDelegate() }
                .is_some_and(|delegate| answers((*delegate).as_ref(), selector))
        }

        fn store_answers(&self, selector: Sel) -> bool {
            // SAFETY: on the main thread with the view retained; both reads come back retained.
            let store = unsafe { self.0.configuration().websiteDataStore() };
            answers(&store, selector)
        }

        fn proxy_configuration_count(&self) -> Option<usize> {
            // SAFETY: on the main thread with the view retained; both reads come back retained.
            let store = unsafe { self.0.configuration().websiteDataStore() };
            // SAFETY: the callers ask only after the store answered proxyConfigurations, which
            // takes nothing and returns an NSArray or nil, retained here.
            let configs: Option<Retained<NSArray<AnyObject>>> =
                unsafe { msg_send![&*store, proxyConfigurations] };
            configs.map(|configs| configs.count())
        }

        fn set_proxy(&self, host: &CStr, port: &CStr) -> Result<(), NotInstalled> {
            // SAFETY: on the main thread with the view retained; both reads come back retained.
            let store = unsafe { self.0.configuration().websiteDataStore() };
            // SAFETY: both arguments are NUL-terminated strings that the caller owns through the
            // call; Network copies them and returns a +1 object or null.
            let endpoint = unsafe { nw_endpoint_create_host(host.as_ptr(), port.as_ptr()) };
            // SAFETY: the pointer is the +1 result of nw_endpoint_create_host, an Objective-C
            // object under OS_OBJECT_USE_OBJC, or null; Retained takes over that one reference.
            let endpoint = unsafe { Retained::from_raw(endpoint) }.ok_or("no proxy endpoint")?;
            // SAFETY: `endpoint` is retained through the call, and Network retains what it keeps;
            // the result is a +1 object or null.
            let config =
                unsafe { nw_proxy_config_create_socksv5(Retained::as_ptr(&endpoint).cast_mut()) };
            // SAFETY: the pointer is the +1 result of nw_proxy_config_create_socksv5 or null;
            // Retained takes over that one reference.
            let config = unsafe { Retained::from_raw(config) }.ok_or("no proxy configuration")?;
            let configs = NSArray::from_retained_slice(&[config]);
            // SAFETY: on the main thread; the callers send only after the store answered
            // setProxyConfigurations:, whose argument is an NSArray of nw_proxy_config_t and
            // which returns void; the store copies the array.
            let _: () = unsafe { msg_send![&*store, setProxyConfigurations: &*configs] };
            Ok(())
        }
    }

    pub(super) fn read_the_store_and_the_hosts(side: &impl Side) -> Result<(), NotInstalled> {
        if side.store_is_persistent() {
            return Err(super::store_keeps_data());
        }
        if !side.config_answers(sel!(_allowedNetworkHosts)) {
            return Err(NotInstalled::on_this_system(
                "the configuration of this system has no allowed-hosts setting".to_string(),
            ));
        }
        let count = side
            .allowed_hosts_count()
            .ok_or_else(super::every_network_host_allowed)?;
        if count != 0 {
            return Err(super::network_hosts_allowed(count));
        }
        Ok(())
    }

    fn put_delegate_in_front(
        view: &WKWebView,
        mtm: MainThreadMarker,
    ) -> Result<*const AnyObject, NotInstalled> {
        // SAFETY: `view` is retained and this runs on the main thread, which `mtm` proves and
        // WebKit requires; the delegate comes back retained, so the ivars own it.
        let inner = unsafe { view.navigationDelegate() }.ok_or("no navigation delegate")?;
        let guard = Guard::alloc(mtm).set_ivars(Ivars { inner });
        // SAFETY: `guard` is an allocation of this class with its ivars set, and NSObject's init
        // is the designated initializer of its superclass; the result is +1, which Retained owns.
        let guard: Retained<Guard> = unsafe { msg_send![super(guard), init] };
        let pointer = Retained::as_ptr(&guard).cast::<AnyObject>();
        // SAFETY: on the main thread with `view` retained; WebKit holds the delegate weakly, so the
        // mem::forget below keeps `guard`, and the inner delegate it retains, alive for the
        // life of the process.
        unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*guard))) };
        std::mem::forget(guard);
        Ok(pointer)
    }

    /// One read, after the delegate was put in front and its function has returned: the view's
    /// navigation delegate is the Guard by pointer, it answers yes for the navigation selector
    /// and no for its form with preferences, and the data store reads back one proxy
    /// configuration. Anything else is not installed.
    pub(super) fn read_back_the_delegate_and_the_proxy(
        side: &impl Side,
        guard: *const AnyObject,
    ) -> Result<(), NotInstalled> {
        let delegate = side
            .delegate()
            .ok_or_else(|| NotInstalled::on_this_system(super::DELEGATE_READS_NONE))?;
        if !std::ptr::eq(delegate, guard) {
            return Err(NotInstalled::on_this_system(super::DELEGATE_READS_ANOTHER));
        }
        if !side.delegate_answers(sel!(webView:decidePolicyForNavigationAction:decisionHandler:))
            || side.delegate_answers(sel!(
                webView:decidePolicyForNavigationAction:preferences:decisionHandler:
            ))
        {
            return Err(NotInstalled::on_this_system(super::GUARD_ANSWERS_OTHERWISE));
        }
        if !side.store_answers(sel!(proxyConfigurations)) {
            return Err(NotInstalled::on_this_system(super::PROXY_NOT_READ_BACK));
        }
        let count = side.proxy_configuration_count().unwrap_or(0);
        if count != 1 {
            return Err(NotInstalled::on_this_system(format!(
                "the data store reads back {count} proxy configurations"
            )));
        }
        Ok(())
    }

    pub(super) fn set_the_store_proxy(side: &impl Side) -> Result<(), NotInstalled> {
        if !side.store_answers(sel!(setProxyConfigurations:)) {
            return Err(NotInstalled::on_this_system(
                "the data store of this system has no proxy setting".to_string(),
            ));
        }
        let (host, port) = super::proxy_endpoint()?;
        side.set_proxy(&host, &port)
    }

    fn answers(object: &AnyObject, selector: Sel) -> bool {
        // SAFETY: `object` is a live Objective-C object or class for the call; respondsToSelector:
        // of NSObject takes a SEL and returns BOOL.
        unsafe { msg_send![object, respondsToSelector: selector] }
    }

    fn feature_named(key: &str) -> Result<Retained<AnyObject>, NotInstalled> {
        let class = WKPreferences::class();
        if !answers(class.as_ref(), sel!(_features)) {
            return Err(NotInstalled::on_this_system(
                "the preferences of this system list no features".to_string(),
            ));
        }
        // SAFETY: +_features takes nothing and returns an autoreleased NSArray of _WKFeature,
        // which Retained retains.
        let features: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![class, _features] };
        let features = features.ok_or("no feature list")?;
        for i in 0..features.count() {
            let feature = features.objectAtIndex(i);
            if !answers(&feature, sel!(key)) {
                return Err(NotInstalled::on_this_system(
                    "a feature of this system has no key".to_string(),
                ));
            }
            // SAFETY: the feature answers key (checked above), which takes nothing and returns an
            // NSString owned by the feature.
            let name: Option<Retained<NSString>> = unsafe { msg_send![&*feature, key] };
            if name.is_some_and(|name| name.to_string() == key) {
                return Ok(feature);
            }
        }
        Err(NotInstalled::on_this_system(format!(
            "the preferences of this system have no feature {key}"
        )))
    }

    fn set_feature(prefs: &WKPreferences, key: &str, on: bool) -> Result<(), NotInstalled> {
        let feature = feature_named(key)?;
        if !answers(prefs, sel!(_setEnabled:forFeature:)) {
            return Err(NotInstalled::on_this_system(
                "the preferences of this system cannot set a feature".to_string(),
            ));
        }
        if !answers(prefs, sel!(_isEnabledForFeature:)) {
            return Err(NotInstalled::on_this_system(
                "the preferences of this system cannot read a feature back".to_string(),
            ));
        }
        // SAFETY: the preferences answer _setEnabled:forFeature: (checked above), which takes a
        // BOOL and a _WKFeature of the list and returns void, on the main thread.
        let _: () = unsafe { msg_send![prefs, _setEnabled: on, forFeature: &*feature] };
        // SAFETY: the preferences answer _isEnabledForFeature: (checked above), which takes the
        // same _WKFeature and returns BOOL.
        let now: bool = unsafe { msg_send![prefs, _isEnabledForFeature: &*feature] };
        if now != on {
            return Err(NotInstalled::from(format!(
                "the feature {key} does not read back as set"
            )));
        }
        Ok(())
    }

    fn turn_off_engine_features(view: &WKWebView) -> Result<(), NotInstalled> {
        // SAFETY: on the main thread with `view` retained; both reads come back retained, and the
        // preferences are the object that the page's settings follow (WebKit's configuration copy
        // shares it).
        let prefs = unsafe { view.configuration().preferences() };
        if !answers(&prefs, sel!(_setPeerConnectionEnabled:)) {
            return Err(NotInstalled::on_this_system(
                "the preferences of this system have no peer-connection setting".to_string(),
            ));
        }
        if !answers(&prefs, sel!(_peerConnectionEnabled)) {
            return Err(NotInstalled::on_this_system(
                "the preferences of this system cannot read the peer-connection setting back"
                    .to_string(),
            ));
        }
        // SAFETY: the preferences answer _setPeerConnectionEnabled: (checked above), which takes
        // a BOOL and returns void, on the main thread.
        let _: () = unsafe { msg_send![&*prefs, _setPeerConnectionEnabled: false] };
        // SAFETY: the preferences answer _peerConnectionEnabled (checked above), which takes
        // nothing and returns BOOL.
        let peer_on: bool = unsafe { msg_send![&*prefs, _peerConnectionEnabled] };
        if peer_on {
            return Err(NotInstalled::from(
                "peer connections stay on after the preference was set".to_string(),
            ));
        }
        set_feature(&prefs, super::LINK_PRECONNECT_FEATURE, false)
    }

    fn read_the_peer_constructor(view: &WKWebView, answer: Sender<GuardAnswer>) {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = answer.send(Err(NotInstalled::from(
                "not on the main thread".to_string(),
            )));
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
            // SAFETY: WebKit passes an error that it keeps alive for the call, or null.
            let reason =
                unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string());
            let _ = answer.send(super::peer_reading(text.as_deref(), reason));
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
            let _ = answer.send(Err(NotInstalled::from(
                "not on the main thread".to_string(),
            )));
            return;
        };
        let pointer = webview.inner().cast::<WKWebView>();
        // SAFETY: wry hands with_webview's closure the pointer of the WKWebView that it owns,
        // alive while the closure runs on the main thread; retaining it here keeps the view alive
        // until the rule list's completion has used it.
        let Some(view) = (unsafe { Retained::retain(pointer) }) else {
            let _ = answer.send(Err(NotInstalled::from("no web view".to_string())));
            return;
        };
        let side = Real(&view);
        if let Err(e) = put_delegate_in_front(&view, mtm)
            .and_then(|guard| set_the_store_proxy(&side).map(|()| guard))
            .and_then(|guard| turn_off_engine_features(&view).map(|()| guard))
            .and_then(|guard| read_the_store_and_the_hosts(&side).map(|()| guard))
            .and_then(|guard| read_back_the_delegate_and_the_proxy(&side, guard))
        {
            let _ = answer.send(Err(e));
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&site.rules_dir) {
            let _ = answer.send(Err(NotInstalled::from(format!(
                "the directory of the rule list cannot be made: {e}"
            ))));
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
            let _ = answer.send(Err(NotInstalled::from(
                "no store of rule lists".to_string(),
            )));
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
                let _ = answer.send(Err(NotInstalled::from(format!(
                    "the rule list did not compile: {reason}"
                ))));
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

    /// The install's reads through a stand-in of the engine (ruling AP-17 G3): each read that
    /// the install makes can be made to fail, and each failure answers not installed.
    #[cfg(test)]
    mod side_tests {
        use std::cell::RefCell;
        use std::ffi::CStr;

        use objc2::runtime::{AnyObject, Sel};
        use objc2::sel;

        use super::{
            read_back_the_delegate_and_the_proxy, read_the_store_and_the_hosts,
            set_the_store_proxy, Side,
        };
        use crate::engine::NotInstalled;

        struct Engine {
            persistent: bool,
            answers_hosts: bool,
            hosts: Option<usize>,
            delegate: Option<*const AnyObject>,
            answers_plain: bool,
            answers_preferences: bool,
            reads_proxies: bool,
            sets_proxies: bool,
            proxies: Option<usize>,
            sent: RefCell<Vec<(String, String)>>,
        }

        type Change = fn(&mut Engine);

        const GUARD: u8 = 1;
        const OTHER: u8 = 2;

        fn guard() -> *const AnyObject {
            std::ptr::from_ref(&GUARD).cast()
        }

        fn engine() -> Engine {
            Engine {
                persistent: false,
                answers_hosts: true,
                hosts: Some(0),
                delegate: Some(guard()),
                answers_plain: true,
                answers_preferences: false,
                reads_proxies: true,
                sets_proxies: true,
                proxies: Some(1),
                sent: RefCell::new(Vec::new()),
            }
        }

        impl Side for Engine {
            fn store_is_persistent(&self) -> bool {
                self.persistent
            }
            fn config_answers(&self, selector: Sel) -> bool {
                selector == sel!(_allowedNetworkHosts) && self.answers_hosts
            }
            fn allowed_hosts_count(&self) -> Option<usize> {
                assert!(self.answers_hosts, "the count is asked before the question");
                self.hosts
            }
            fn delegate(&self) -> Option<*const AnyObject> {
                self.delegate
            }
            fn delegate_answers(&self, selector: Sel) -> bool {
                if selector == sel!(webView:decidePolicyForNavigationAction:decisionHandler:) {
                    self.answers_plain
                } else if selector
                    == sel!(webView:decidePolicyForNavigationAction:preferences:decisionHandler:)
                {
                    self.answers_preferences
                } else {
                    panic!("no other selector is asked")
                }
            }
            fn store_answers(&self, selector: Sel) -> bool {
                if selector == sel!(proxyConfigurations) {
                    self.reads_proxies
                } else if selector == sel!(setProxyConfigurations:) {
                    self.sets_proxies
                } else {
                    panic!("no other selector is asked")
                }
            }
            fn proxy_configuration_count(&self) -> Option<usize> {
                assert!(self.reads_proxies, "the list is read before the question");
                self.proxies
            }
            fn set_proxy(&self, host: &CStr, port: &CStr) -> Result<(), NotInstalled> {
                assert!(self.sets_proxies, "setProxyConfigurations: is sent unasked");
                self.sent.borrow_mut().push((
                    host.to_string_lossy().into_owned(),
                    port.to_string_lossy().into_owned(),
                ));
                Ok(())
            }
        }

        #[test]
        fn the_reading_at_start_answers_not_installed_for_a_store_that_keeps_data_and_for_allowed_hosts(
        ) {
            assert!(read_the_store_and_the_hosts(&engine()).is_ok());
            let rows: [(Change, NotInstalled); 4] = [
                (|e| e.persistent = true, crate::engine::store_keeps_data()),
                (
                    |e| e.answers_hosts = false,
                    NotInstalled::on_this_system(
                        "the configuration of this system has no allowed-hosts setting",
                    ),
                ),
                (
                    |e| e.hosts = None,
                    crate::engine::every_network_host_allowed(),
                ),
                (
                    |e| e.hosts = Some(2),
                    crate::engine::network_hosts_allowed(2),
                ),
            ];
            for (change, want) in rows {
                let mut e = engine();
                change(&mut e);
                assert_eq!(read_the_store_and_the_hosts(&e), Err(want));
            }
        }

        #[test]
        fn the_read_back_answers_not_installed_for_every_delegate_and_proxy_that_is_not_the_guards()
        {
            assert!(read_back_the_delegate_and_the_proxy(&engine(), guard()).is_ok());
            let rows: [(Change, &str); 8] = [
                (|e| e.delegate = None, crate::engine::DELEGATE_READS_NONE),
                (
                    |e| e.delegate = Some(std::ptr::from_ref(&OTHER).cast()),
                    crate::engine::DELEGATE_READS_ANOTHER,
                ),
                (
                    |e| e.answers_plain = false,
                    crate::engine::GUARD_ANSWERS_OTHERWISE,
                ),
                (
                    |e| e.answers_preferences = true,
                    crate::engine::GUARD_ANSWERS_OTHERWISE,
                ),
                (
                    |e| e.reads_proxies = false,
                    crate::engine::PROXY_NOT_READ_BACK,
                ),
                (
                    |e| e.proxies = None,
                    "the data store reads back 0 proxy configurations",
                ),
                (
                    |e| e.proxies = Some(0),
                    "the data store reads back 0 proxy configurations",
                ),
                (
                    |e| e.proxies = Some(2),
                    "the data store reads back 2 proxy configurations",
                ),
            ];
            for (change, want) in rows {
                let mut e = engine();
                change(&mut e);
                assert_eq!(
                    read_back_the_delegate_and_the_proxy(&e, guard()),
                    Err(NotInstalled::on_this_system(want)),
                    "{want}"
                );
            }
        }

        #[test]
        fn the_proxy_is_sent_only_after_the_question_and_only_to_the_address_of_no_program() {
            let e = engine();
            assert!(set_the_store_proxy(&e).is_ok());
            assert_eq!(
                *e.sent.borrow(),
                vec![("255.255.255.255".to_string(), "9".to_string())]
            );
            let mut e = engine();
            e.sets_proxies = false;
            assert_eq!(
                set_the_store_proxy(&e),
                Err(NotInstalled::on_this_system(
                    "the data store of this system has no proxy setting"
                ))
            );
            assert!(e.sent.borrow().is_empty());
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use std::sync::mpsc::Sender;

    use super::{EngineSite, GuardAnswer, NotInstalled};

    pub fn install(
        _: &tauri::webview::PlatformWebview,
        _: &EngineSite,
        answer: Sender<GuardAnswer>,
    ) {
        let _ = answer.send(Err(NotInstalled::on_this_system(
            "no engine guard for this system".to_string(),
        )));
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
            "http://xn--zz.com:47026/p?q#f",
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
            ("super::request_hook_allows(", 1),
            ("super::guard_allows(", 1),
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
        assert_eq!(macos.matches("GuardInstalled(())").count(), 0);
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
                "let side = Real(&view);\n        if let Err(e) = put_delegate_in_front(&view, mtm)\n            .and_then(|guard| set_the_store_proxy(&side).map(|()| guard))\n            .and_then(|guard| turn_off_engine_features(&view).map(|()| guard))\n            .and_then(|guard| read_the_store_and_the_hosts(&side).map(|()| guard))\n            .and_then(|guard| read_back_the_delegate_and_the_proxy(&side, guard))\n",
                1,
            ),
            (
                "if selector == sel!(webView:decidePolicyForNavigationAction:decisionHandler:) {\n                    return Bool::YES;\n                }",
                1,
            ),
            (
                "== sel!(webView:decidePolicyForNavigationAction:preferences:decisionHandler:)\n                {\n                    return Bool::NO;\n                }",
                1,
            ),
            (
                "unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*guard))) };",
                1,
            ),
            ("std::mem::forget(guard);\n        Ok(pointer)", 1),
            (
                "let _: () = unsafe { msg_send![&*store, setProxyConfigurations: &*configs] };",
                1,
            ),
            ("unsafe { view.navigationDelegate() }", 1),
            ("unsafe { self.0.navigationDelegate() }", 2),
            ("if !std::ptr::eq(delegate, guard) {", 1),
            (
                "if !side.delegate_answers(sel!(webView:decidePolicyForNavigationAction:decisionHandler:))\n            || side.delegate_answers(sel!(\n                webView:decidePolicyForNavigationAction:preferences:decisionHandler:\n            ))\n        {",
                1,
            ),
            ("side.store_answers(sel!(proxyConfigurations))", 1),
            ("side.store_answers(sel!(setProxyConfigurations:))", 1),
            ("msg_send![&*store, proxyConfigurations]", 1),
            ("if count != 1 {", 1),
            ("answers(&config, sel!(_setAllowedNetworkHosts:))", 1),
            (
                "let _ = answer.send(super::peer_reading(text.as_deref(), reason));",
                1,
            ),
            ("evaluateJavaScript_inFrame_inContentWorld_completionHandler(", 1),
            ("WKContentWorld::worldWithName(&NSString::from_str(super::GUARD_WORLD), mtm)", 1),
            ("&NSString::from_str(super::PEER_READING),", 1),
            ("msg_send![&*config, _setAllowedNetworkHosts: &*none]", 1),
            ("WKWebsiteDataStore::nonPersistentDataStore(mtm)", 1),
            ("side.config_answers(sel!(_allowedNetworkHosts))", 1),
            ("msg_send![&*config, _allowedNetworkHosts]", 1),
            ("msg_send![&*hosts, count]", 1),
            ("if side.store_is_persistent() {", 1),
            ("unsafe { self.0.configuration().websiteDataStore().isPersistent() }", 1),
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
            ("super::guard_allows(", 1),
            ("handler.call((WKNavigationActionPolicy::Cancel,));", 1),
        ] {
            assert_eq!(macos.matches(text).count(), count, "{text}");
        }
    }

    #[test]
    fn the_navigation_handlers_go_on_only_for_an_address_that_the_engine_allows() {
        let (scheme, host) = crate::app::APP_ORIGIN;
        assert!(guard_allows(Some(&format!("{scheme}://{host}/"))));
        for refused in [
            None,
            Some(""),
            Some("https://example.com/"),
            Some("http://xn--zz.localhost:1/"),
        ] {
            assert!(!guard_allows(refused), "{refused:?}");
        }
    }

    #[test]
    fn the_request_hook_lets_through_only_an_address_that_it_reads_and_allows() {
        assert!(request_hook_allows(Some(
            "http://tauri.localhost/index.html"
        )));
        assert!(request_hook_allows(Some("http://ipc.localhost/plugin")));
        for refused in [
            None,
            Some(""),
            Some("http://127.0.0.1/"),
            Some("https://tauri.localhost/"),
        ] {
            assert!(!request_hook_allows(refused), "{refused:?}");
        }
    }

    #[test]
    fn the_proxy_endpoint_is_the_address_of_no_program_and_link_preconnect_is_the_feature_turned_off(
    ) {
        let (host, port) = proxy_endpoint().expect("two strings");
        assert_eq!(host.to_str(), Ok("255.255.255.255"));
        assert_eq!(port.to_str(), Ok("9"));
        assert_eq!(LINK_PRECONNECT_FEATURE, "LinkPreconnect");
    }

    #[test]
    fn the_reading_at_start_installs_only_when_the_constructors_read_absent() {
        assert!(peer_reading(Some(PEER_ABSENT), None).is_ok());
        assert_eq!(
            peer_reading(Some("RTCPeerConnection"), None).err(),
            Some(NotInstalled::on_this_system(
                "the engine offers the peer connection's constructor (RTCPeerConnection)"
            ))
        );
        assert!(peer_reading(None, Some("timeout".to_string())).is_err());
        assert!(peer_reading(None, None).is_err());
    }

    /// Ruling AP-17 G3: each means stands at its one call, in the exact text that a test or the
    /// functions above hold; a change that lets a handler allow everything, compiles another
    /// rule list, sends another endpoint or turns LinkPreconnect on changes one of these texts.
    #[test]
    fn each_means_of_the_engine_guard_stands_at_its_one_call_in_its_exact_text() {
        let source = src_file_with_lf_line_ends("engine.rs");
        let windows = platform_module(&source, "#[cfg(windows)]\nmod platform {");
        let macos = platform_module(&source, "#[cfg(target_os = \"macos\")]\nmod platform {");
        for (module, text) in [
            (
                windows,
                "            let allowed = super::guard_allows(navigation_address(&args).as_deref());\n            if !allowed {\n                // SAFETY: WebView2 keeps `args` alive until this handler returns, on the window's\n                // thread, and reads the flag only after the return.\n                unsafe { args.SetCancel(true) }?;\n            }\n            Ok(())\n",
            ),
            (
                windows,
                "            if !super::request_hook_allows(address.as_deref()) {\n                // SAFETY: inside the handler, while WebView2 keeps `args` alive; it takes its own\n                // reference to `refusal`.\n                unsafe { args.SetResponse(&refusal) }?;\n            }\n            Ok(())\n",
            ),
            (
                macos,
                "                if !super::guard_allows(address.as_deref()) {\n                    handler.call((WKNavigationActionPolicy::Cancel,));\n                    return;\n                }\n",
            ),
            (
                macos,
                "                Some(&NSString::from_str(super::RULE_LIST_ID)),\n                Some(&NSString::from_str(super::RULE_LIST)),\n                Some(&compiled),\n",
            ),
            (
                macos,
                "        let (host, port) = super::proxy_endpoint()?;\n        side.set_proxy(&host, &port)\n",
            ),
            (
                macos,
                "            let endpoint = unsafe { nw_endpoint_create_host(host.as_ptr(), port.as_ptr()) };\n",
            ),
            (macos, "        set_feature(&prefs, super::LINK_PRECONNECT_FEATURE, false)\n"),
            (macos, "            let _ = answer.send(super::peer_reading(text.as_deref(), reason));\n"),
        ] {
            assert_eq!(module.matches(text).count(), 1, "{text}");
        }
        let made = windows
            .find("let refusal = unsafe {\n            environment.CreateWebResourceResponse(")
            .expect("the refusal is made at install");
        let start = windows
            .find("WebResourceRequestedEventHandler::create(Box::new(move |_, args| {")
            .expect("the request hook");
        assert!(made < start, "the refusal is made before the hook");
        let hook = &windows[start..start + windows[start..].find("        }));").expect("its end")];
        assert_eq!(
            hook.matches('?').count(),
            1,
            "no call in the hook but the refusal can fail out: {hook}"
        );
        assert_eq!(hook.matches("CreateWebResourceResponse").count(), 0);
        for (module, name, count) in [
            (windows, "guard_allows(", 1),
            (windows, "request_hook_allows(", 1),
            (windows, "SetResponse(", 1),
            (windows, "SetCancel(", 1),
            (macos, "guard_allows(", 1),
            (macos, "compileContentRuleListForIdentifier", 1),
            (macos, "nw_endpoint_create_host(", 2),
            (macos, "proxy_endpoint(", 1),
            (macos, "LINK_PRECONNECT_FEATURE", 1),
            (macos, "peer_reading(", 1),
        ] {
            assert_eq!(module.matches(name).count(), count, "{name}");
        }
    }
}
