use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use halvo_control::paths::DataDir;
use serde::de::value::{Error as WordError, StrDeserializer};
use serde::de::IntoDeserializer;
use serde::Deserialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::app::{self, PageLoads};
use crate::commands::{capture::AppState, names};
use crate::engine::{self, GuardInstalled};
use crate::launch::Exit;
use crate::view_state::Service;

const PROBE: &str = "(function () { var ok = /^[A-Za-z0-9_:|.+-]+$/; var r = (window.Halvo && Halvo.ipc && Halvo.ipc.replied) ? Halvo.ipc.replied() : []; var v = (document.getElementById('version') || {}).textContent || ''; var s = (window.Halvo && Halvo.service) ? String(Halvo.service()) : ''; return 'version=' + (ok.test(v) ? v : '-') + ' service=' + (ok.test(s) ? s : '-') + ' replied=' + r.filter(function (n) { return ok.test(n); }).join(','); })()";

const POLL: Duration = Duration::from_millis(200);

pub fn reads() -> Vec<&'static str> {
    names::MODULES
        .iter()
        .flat_map(|module| module.reads.iter().copied())
        .collect()
}

pub fn read_service(word: &str) -> Option<Service> {
    let word: StrDeserializer<'_, WordError> = word.into_deserializer();
    Service::deserialize(word)
        .ok()
        .filter(|service: &Service| *service != Service::Connecting)
}

/// The line that the check prints for `seen`, the JSON text of the probe's
/// string.
pub fn line(seen: &str, version: &str, guard: Option<&GuardInstalled>) -> Option<String> {
    guard?;
    let text = seen.strip_prefix('"')?.strip_suffix('"')?;
    let (shown, rest) = text.strip_prefix("version=")?.split_once(" service=")?;
    let (service, replied) = rest.split_once(" replied=")?;
    let reads = reads();
    let ok = reads
        .iter()
        .filter(|name| replied.split(',').any(|r| r == **name))
        .count();
    (ok == reads.len() && shown == version && read_service(service).is_some()).then(|| {
        format!("page=loaded commands_ok={ok} version={shown} service={service} guard=installed")
    })
}

/// When the check mode gives up, as a time after its start: `within` after
/// its event loop was ready at `ready`, and never while the loop is not ready,
/// so that the start of the engine before Ready does not count in `within`.
pub fn gives_up_after(ready: Option<Duration>, within: Duration) -> Option<Duration> {
    ready.map(|ready| ready + within)
}

fn give_up(started: Instant, within: Duration, ready: &mpsc::Receiver<Duration>) {
    let mut ready_at = None;
    loop {
        let heard = match gives_up_after(ready_at, within) {
            Some(at) => ready.recv_timeout(at.saturating_sub(started.elapsed())),
            None => ready
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match heard {
            Ok(at) => ready_at = Some(at),
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
    eprintln!(
        "halvo: --check-page: the page did not have the reply of its {} read commands within {} s after the event loop was ready",
        reads().len(),
        within.as_secs()
    );
    std::process::exit(Exit::PageNotLoaded.code());
}

pub fn run(data_dir: PathBuf, within: Duration, started: Instant) -> i32 {
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || give_up(started, within, &ready_rx));
    let engine_rules_dir = Some(data_dir.join(app::ENGINE_RULES_DIR));
    let parts = app::Parts {
        #[cfg(windows)]
        webview_data_dir: Some(data_dir.join("webview")),
        #[cfg(windows)]
        webview_environment: app::system_environment,
        state: AppState::new(|| DataDir::from_flag(data_dir), crate::open_pane),
        tray: None,
        guard: engine::guard,
        poll: true,
        window_visible: false,
        engine_rules_dir,
    };
    let built = app::build(tauri::Builder::default(), parts).build(app::context());
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut app = match built {
        Ok(app) => app,
        Err(e) => {
            eprintln!("halvo: --check-page: the app did not start: {e}");
            return Exit::PageNotLoaded.code();
        }
    };
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    let handle = app.handle().clone();
    std::thread::spawn(move || watch(handle, started));
    app.run_return(move |_, event| {
        if let tauri::RunEvent::Ready = event {
            let ready = started.elapsed();
            eprintln!(
                "halvo: --check-page: the event loop was ready at {} ms",
                ready.as_millis()
            );
            let _ = ready_tx.send(ready);
        }
    })
}

fn watch<R: Runtime>(app: AppHandle<R>, started: Instant) {
    let version = app.package_info().version.to_string();
    loop {
        std::thread::sleep(POLL);
        let loaded = app.try_state::<PageLoads>().is_some_and(|l| l.count() > 0);
        let Some(window) = app.get_webview_window("main").filter(|_| loaded) else {
            continue;
        };
        let (tx, rx) = mpsc::channel();
        if window
            .eval_with_callback(PROBE, move |seen| {
                let _ = tx.send(seen);
            })
            .is_err()
        {
            continue;
        }
        let Ok(seen) = rx.recv_timeout(Duration::from_secs(2)) else {
            continue;
        };
        let guard = app.try_state::<GuardInstalled>();
        if let Some(line) = line(&seen, &version, guard.as_deref()) {
            #[cfg(windows)]
            let line = format!(
                "{line} display_affinity={} general_autofill={}",
                display_affinity(&window),
                general_autofill(&window)
            );
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
            eprintln!(
                "halvo: --check-page: done in {} ms",
                started.elapsed().as_millis()
            );
            app.exit(Exit::PageLoaded.code());
            return;
        }
    }
}

/// The display affinity of the window main as Windows reads it (GetWindowDisplayAffinity): 17,
/// WDA_EXCLUDEFROMCAPTURE, where the system keeps the window out of screenshots, screen shares
/// and screen recordings that are made through Windows' own capture features, Halvo's own screen
/// record among them; `-` where it cannot be read.
#[cfg(windows)]
fn display_affinity<R: Runtime>(window: &tauri::WebviewWindow<R>) -> String {
    #[link(name = "user32")]
    extern "system" {
        fn GetWindowDisplayAffinity(window: *mut std::ffi::c_void, affinity: *mut u32) -> i32;
    }
    let Ok(handle) = window.hwnd() else {
        return "-".to_owned();
    };
    let mut affinity = 0u32;
    // SAFETY: the handle names the window main, which the app holds open while it runs, and
    // `affinity` is a live local that the call writes.
    let read = unsafe { GetWindowDisplayAffinity(handle.0, &mut affinity) };
    if read == 0 {
        return "-".to_owned();
    }
    affinity.to_string()
}

/// Whether the web view of the window main has its general autofill on, as WebView2 reads it
/// (ICoreWebView2Settings4::IsGeneralAutofillEnabled): `on`, `off`, or `-` where it cannot be read
/// within 2 s.
#[cfg(windows)]
fn general_autofill<R: Runtime>(window: &tauri::WebviewWindow<R>) -> &'static str {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings4;
    use windows_core::Interface;
    let (tx, rx) = mpsc::channel();
    let asked = window.with_webview(move |webview| {
        let read = || -> windows_core::Result<bool> {
            // SAFETY: wry keeps the controller alive as long as the web view, and with_webview
            // hands it to this closure on the thread that created it, the one thread WebView2 lets
            // call it; each returned interface holds its own reference.
            let settings = unsafe { webview.controller().CoreWebView2()?.Settings()? };
            let settings: ICoreWebView2Settings4 = settings.cast()?;
            let mut enabled = windows_core::BOOL::default();
            // SAFETY: on the window's thread, on an interface that holds its reference to the
            // settings; `enabled` is a live local that the call writes.
            unsafe { settings.IsGeneralAutofillEnabled(&mut enabled) }?;
            Ok(enabled.as_bool())
        };
        let _ = tx.send(read().ok());
    });
    if asked.is_err() {
        return "-";
    }
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Some(true)) => "on",
        Ok(Some(false)) => "off",
        Ok(None) | Err(_) => "-",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::installed_for_tests as installed;

    fn seen(replied: &[&str], version: &str, service: &str) -> String {
        format!(
            "\"version={version} service={service} replied={}\"",
            replied.join(",")
        )
    }

    #[test]
    fn the_line_waits_for_every_read_command_the_version_and_a_read_service() {
        let all = [
            "capture_status",
            "inventory",
            "onboarding_state",
            "last_delete",
        ];
        assert_eq!(
            line(
                &seen(&all, "0.0.0", "not_running"),
                "0.0.0",
                Some(&installed())
            )
            .as_deref(),
            Some("page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed")
        );
        for service in [
            "running",
            "not_running",
            "held_unreachable",
            "status_refused",
            "unsupported",
            "another_version",
        ] {
            assert_eq!(
                line(&seen(&all, "0.0.0", service), "0.0.0", Some(&installed())),
                Some(format!(
                    "page=loaded commands_ok=4 version=0.0.0 service={service} guard=installed"
                ))
            );
        }
        assert_eq!(
            line(
                &seen(
                    &[
                        "inventory",
                        "plugin:app|version",
                        "capture_status",
                        "onboarding_state",
                        "last_delete"
                    ],
                    "0.0.0",
                    "unsupported"
                ),
                "0.0.0",
                Some(&installed())
            )
            .as_deref(),
            Some("page=loaded commands_ok=4 version=0.0.0 service=unsupported guard=installed")
        );
        assert_eq!(
            line(
                &seen(&all[..2], "0.0.0", "not_running"),
                "0.0.0",
                Some(&installed())
            ),
            None
        );
        assert_eq!(
            line(&seen(&all, "-", "not_running"), "0.0.0", Some(&installed())),
            None
        );
        assert_eq!(
            line(
                &seen(&all, "0.0.1", "not_running"),
                "0.0.0",
                Some(&installed())
            ),
            None
        );
        assert_eq!(
            line(
                &seen(&all, "0.0.0", "connecting"),
                "0.0.0",
                Some(&installed())
            ),
            None
        );
        assert_eq!(
            line(&seen(&all, "0.0.0", "-"), "0.0.0", Some(&installed())),
            None
        );
        assert_eq!(
            line(
                &seen(&all, "0.0.0", "capturing"),
                "0.0.0",
                Some(&installed())
            ),
            None
        );
        assert_eq!(line("", "0.0.0", Some(&installed())), None);
        assert_eq!(line("\"not the probe\"", "0.0.0", Some(&installed())), None);
        assert_eq!(
            line(
                "version=0.0.0 service=not_running replied=capture_status,inventory,onboarding_state",
                "0.0.0",
                Some(&installed())
            ),
            None
        );
        assert_eq!(
            line(
                "\"version=0.0.0 replied=capture_status,inventory,onboarding_state\"",
                "0.0.0",
                Some(&installed())
            ),
            None
        );
    }

    #[test]
    fn without_the_proof_of_the_guard_the_check_prints_no_line() {
        let all = [
            "capture_status",
            "inventory",
            "onboarding_state",
            "last_delete",
        ];
        for service in [
            "running",
            "not_running",
            "held_unreachable",
            "status_refused",
            "unsupported",
            "another_version",
        ] {
            assert_eq!(line(&seen(&all, "0.0.0", service), "0.0.0", None), None);
        }
    }

    #[test]
    fn the_reads_are_those_of_the_capture_module() {
        assert_eq!(
            reads(),
            [
                "capture_status",
                "inventory",
                "onboarding_state",
                "last_delete"
            ]
        );
        for module in names::MODULES {
            for read in module.reads {
                assert!(
                    module.commands.contains(read),
                    "{read} of {}",
                    module.module
                );
            }
        }
    }

    #[test]
    fn the_timer_of_the_check_mode_runs_from_the_ready_of_its_event_loop() {
        let within = Duration::from_secs(60);
        assert_eq!(gives_up_after(None, within), None);
        assert_eq!(
            gives_up_after(Some(Duration::from_millis(8_086)), within),
            Some(Duration::from_millis(68_086))
        );
        assert_eq!(
            gives_up_after(Some(Duration::from_secs(75)), within),
            Some(Duration::from_secs(135))
        );
        assert_eq!(gives_up_after(Some(Duration::ZERO), within), Some(within));
    }

    const GIVE_UP_CHILD: &str = "HALVO_CHECK_PAGE_GIVE_UP_CHILD";
    const WITHIN: Duration = Duration::from_millis(50);
    const SENTENCE: &str = "the page did not have the reply of its";

    fn give_up_child(test: &str) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().expect("the test binary has a path"))
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(GIVE_UP_CHILD, "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the test binary starts again")
    }

    fn ended_within(
        child: &mut std::process::Child,
        bound: Duration,
    ) -> Option<std::process::ExitStatus> {
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().expect("the child can be read") {
                return Some(status);
            }
            if start.elapsed() >= bound {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stderr_of(child: &mut std::process::Child) -> String {
        let mut text = String::new();
        if let Some(mut err) = child.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut err, &mut text);
        }
        text
    }

    fn give_up_in_this_child(started: Instant, send_ready: bool, close_first: bool) {
        let (ready, heard) = mpsc::channel();
        if send_ready {
            ready.send(Duration::ZERO).expect("the Ready is sent");
        }
        if close_first {
            drop(ready);
            give_up(started, WITHIN, &heard);
            return;
        }
        give_up(started, WITHIN, &heard);
        drop(ready);
    }

    #[test]
    fn give_up_ends_the_process_with_its_sentence_when_ready_has_no_answer() {
        if std::env::var_os(GIVE_UP_CHILD).is_some() {
            give_up_in_this_child(Instant::now(), true, false);
            return;
        }
        let mut child = give_up_child(
            "check_page::tests::give_up_ends_the_process_with_its_sentence_when_ready_has_no_answer",
        );
        let status = ended_within(&mut child, Duration::from_secs(20));
        let err = stderr_of(&mut child);
        assert_eq!(
            status.and_then(|s| s.code()),
            Some(Exit::PageNotLoaded.code()),
            "{err}"
        );
        assert!(err.contains(SENTENCE), "{err}");
    }

    #[test]
    fn give_up_returns_with_no_sentence_when_the_channel_closes_before_ready() {
        if std::env::var_os(GIVE_UP_CHILD).is_some() {
            give_up_in_this_child(Instant::now(), false, true);
            return;
        }
        let mut child = give_up_child(
            "check_page::tests::give_up_returns_with_no_sentence_when_the_channel_closes_before_ready",
        );
        let status = ended_within(&mut child, Duration::from_secs(20));
        let err = stderr_of(&mut child);
        assert_eq!(status.and_then(|s| s.code()), Some(0), "{err}");
        assert!(!err.contains(SENTENCE), "{err}");
    }

    #[test]
    fn give_up_ends_the_process_when_a_ready_is_heard_after_its_deadline() {
        if std::env::var_os(GIVE_UP_CHILD).is_some() {
            let started = Instant::now()
                .checked_sub(Duration::from_secs(10))
                .expect("an instant 10 s back");
            give_up_in_this_child(started, true, false);
            return;
        }
        let mut child = give_up_child(
            "check_page::tests::give_up_ends_the_process_when_a_ready_is_heard_after_its_deadline",
        );
        let status = ended_within(&mut child, Duration::from_secs(20));
        let err = stderr_of(&mut child);
        assert_eq!(
            status.and_then(|s| s.code()),
            Some(Exit::PageNotLoaded.code()),
            "{err}"
        );
        assert!(err.contains(SENTENCE), "{err}");
        assert!(!err.contains("panicked"), "{err}");
    }

    #[test]
    fn give_up_never_ends_the_process_before_ready() {
        if std::env::var_os(GIVE_UP_CHILD).is_some() {
            give_up_in_this_child(Instant::now(), false, false);
            return;
        }
        let mut child =
            give_up_child("check_page::tests::give_up_never_ends_the_process_before_ready");
        let status = ended_within(&mut child, WITHIN * 10);
        let err = stderr_of(&mut child);
        assert!(status.is_none(), "{err}");
        assert!(!err.contains(SENTENCE), "{err}");
    }
}
