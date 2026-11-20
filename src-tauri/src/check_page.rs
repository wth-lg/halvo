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

pub fn run(data_dir: PathBuf, within: Duration) -> i32 {
    let started = Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(within);
        eprintln!(
            "halvo: --check-page: the page did not have the reply of its {} read commands within {} s",
            reads().len(),
            within.as_secs()
        );
        std::process::exit(Exit::PageNotLoaded.code());
    });
    let engine_rules_dir = Some(data_dir.join(app::ENGINE_RULES_DIR));
    let parts = app::Parts {
        #[cfg(windows)]
        webview_data_dir: Some(data_dir.join("webview")),
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
    eprintln!(
        "halvo: --check-page: at {} ms: the app is built",
        started.elapsed().as_millis()
    );
    let handle = app.handle().clone();
    std::thread::spawn(move || watch(handle, started));
    app.run_return(move |_, event| {
        if let tauri::RunEvent::Ready = event {
            eprintln!(
                "halvo: --check-page: at {} ms: the event loop is ready",
                started.elapsed().as_millis()
            );
        }
    })
}

fn tell(last: &mut String, turn: u32, started: Instant, place: String) {
    if *last != place || turn.is_multiple_of(50) {
        eprintln!(
            "halvo: --check-page: at {} ms, turn {}: {}",
            started.elapsed().as_millis(),
            turn,
            place
        );
        *last = place;
    }
}

fn watch<R: Runtime>(app: AppHandle<R>, started: Instant) {
    let version = app.package_info().version.to_string();
    eprintln!(
        "halvo: --check-page: at {} ms: the watch starts; version {}; reads {}",
        started.elapsed().as_millis(),
        version,
        reads().join(",")
    );
    let mut last = String::new();
    let mut turn = 0u32;
    loop {
        std::thread::sleep(POLL);
        turn += 1;
        let loads = match app.try_state::<PageLoads>() {
            Some(l) => l.count().to_string(),
            None => "none".to_string(),
        };
        let present = app.get_webview_window("main").is_some();
        let installed = app.try_state::<GuardInstalled>().is_some();
        let seen_state =
            format!("loads {loads}, window main present {present}, guard installed {installed}");
        let loaded = app.try_state::<PageLoads>().is_some_and(|l| l.count() > 0);
        let Some(window) = app.get_webview_window("main").filter(|_| loaded) else {
            tell(
                &mut last,
                turn,
                started,
                format!("place 1, no page loaded or no window main: {seen_state}"),
            );
            continue;
        };
        let (tx, rx) = mpsc::channel();
        if let Err(e) = window.eval_with_callback(PROBE, move |seen| {
            let _ = tx.send(seen);
        }) {
            tell(
                &mut last,
                turn,
                started,
                format!("place 2, the probe was not sent: {e}: {seen_state}"),
            );
            continue;
        }
        let Ok(seen) = rx.recv_timeout(Duration::from_secs(2)) else {
            tell(
                &mut last,
                turn,
                started,
                format!("place 3, no answer of the probe within 2 s: {seen_state}"),
            );
            continue;
        };
        tell(
            &mut last,
            turn,
            started,
            format!("place 4, an answer: {seen}: {seen_state}"),
        );
        let guard = app.try_state::<GuardInstalled>();
        if let Some(line) = line(&seen, &version, guard.as_deref()) {
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
}
