use tauri::{Runtime, WebviewWindow};

/// Whether the engine may start a navigation to `address`, the string as the engine itself
/// holds it, before the runtime reads it: the navigation rule of the window main
/// (`app::may_navigate`), which refuses a string that the URL reader refuses. The runtime
/// alone allows such a string without asking the rule (tauri-runtime-wry 2.12.3 src/lib.rs
/// lines 4820 to 4826); the handlers of this module ask this function first.
pub fn engine_allows(address: &str) -> bool {
    crate::app::may_navigate(address)
}

/// Puts `engine_allows` in front of every navigation of the window's engine: a handler of
/// the start of a navigation on Windows, and on macOS a navigation delegate that decides first
/// and hands every other call to the delegate of the runtime. A window whose engine cannot be
/// reached is reported on stderr and keeps the runtime's rule.
pub fn guard<R: Runtime>(window: &WebviewWindow<R>) {
    let installed = window.with_webview(|webview| {
        if let Err(e) = platform::install(&webview) {
            eprintln!("halvo: the navigation guard of the engine is not installed: {e}");
        }
    });
    if let Err(e) = installed {
        eprintln!("halvo: the engine of the window main cannot be reached: {e}");
    }
}

#[cfg(windows)]
mod platform {
    use tauri::webview::PlatformWebview;
    use webview2_com::{take_pwstr, NavigationStartingEventHandler};
    use windows_core::PWSTR;

    pub fn install(webview: &PlatformWebview) -> windows_core::Result<()> {
        // SAFETY: CoreWebView2 reads a property of the controller that the runtime created and
        // still holds; the call runs on the thread of the window, where with_webview runs it.
        let core = unsafe { webview.controller().CoreWebView2() }?;
        let handler = NavigationStartingEventHandler::create(Box::new(|_, args| {
            let Some(args) = args else {
                return Ok(());
            };
            let mut uri = PWSTR::null();
            // SAFETY: Uri writes a string that the engine allocated into `uri`, which
            // take_pwstr takes over and frees.
            unsafe { args.Uri(&mut uri) }?;
            if !super::engine_allows(&take_pwstr(uri)) {
                // SAFETY: SetCancel sets a flag of the event's arguments during the event.
                unsafe { args.SetCancel(true) }?;
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: the handler is a COM object that the engine holds for as long as it is
        // registered; the token is written once.
        unsafe { core.add_NavigationStarting(&handler, &mut token) }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use block2::Block;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject, Sel};
    use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_web_kit::{
        WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKWebView,
    };
    use tauri::webview::PlatformWebview;

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

    pub fn install(webview: &PlatformWebview) -> Result<(), &'static str> {
        let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
        let pointer = webview.inner() as *const WKWebView;
        // SAFETY: the runtime hands the pointer of its WKWebView, alive while the closure of
        // with_webview runs on the main thread.
        let view = unsafe { pointer.as_ref() }.ok_or("no web view")?;
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
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    pub fn install(_: &tauri::webview::PlatformWebview) -> Result<(), &'static str> {
        Err("no engine guard for this system")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let forms = include_str!("engine-url-forms.txt");
        let mut refused_by_reader = 0;
        for form in forms.lines() {
            if tauri::Url::parse(form).is_err() {
                refused_by_reader += 1;
                assert!(!engine_allows(form), "{form}");
            }
        }
        assert_eq!(refused_by_reader, 70);
    }

    #[test]
    fn the_window_main_refuses_every_download_and_gets_the_guard_of_its_engine() {
        let app = include_str!("app.rs");
        assert_eq!(app.matches(".on_download(|_, _| false)").count(), 1);
        assert_eq!(app.matches("crate::engine::guard(&window);").count(), 1);
        assert_eq!(app.matches("WebviewWindowBuilder::from_config").count(), 1);
    }
}
