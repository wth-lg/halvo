use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use halvo_control::paths::DataDir;
use tauri::{AppHandle, Manager, Runtime};

use crate::app::{self, PageLoads};
use crate::commands::{capture::AppState, names};
use crate::launch::Exit;

const PROBE: &str = "(function () { var ok = /^[A-Za-z0-9_:|.+-]+$/; var r = (window.Halvo && Halvo.ipc && Halvo.ipc.replied) ? Halvo.ipc.replied() : []; var v = (document.getElementById('version') || {}).textContent || ''; var s = (window.Halvo && Halvo.service) ? String(Halvo.service()) : ''; return 'version=' + (ok.test(v) ? v : '-') + ' service=' + (ok.test(s) ? s : '-') + ' replied=' + r.filter(function (n) { return ok.test(n); }).join(','); })()";

const POLL: Duration = Duration::from_millis(200);

pub fn reads() -> Vec<&'static str> {
    names::MODULES
        .iter()
        .flat_map(|module| module.reads.iter().copied())
        .collect()
}

/// The services that the page can show once a status has been read.
pub const SERVICES: [&str; 5] = [
    "running",
    "not_running",
    "held_unreachable",
    "status_refused",
    "unsupported",
];

/// The line that the check prints for `seen`, the JSON text of the probe's
/// string, or None while the page lacks the reply of a read command, shows
/// another version than `version`, or shows no service that a status was read for.
pub fn line(seen: &str, version: &str) -> Option<String> {
    let text = seen.strip_prefix('"')?.strip_suffix('"')?;
    let (shown, rest) = text.strip_prefix("version=")?.split_once(" service=")?;
    let (service, replied) = rest.split_once(" replied=")?;
    let reads = reads();
    let ok = reads
        .iter()
        .filter(|name| replied.split(',').any(|r| r == **name))
        .count();
    (ok == reads.len() && shown == version && SERVICES.contains(&service))
        .then(|| format!("page=loaded commands_ok={ok} version={shown} service={service}"))
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
    let parts = app::Parts {
        #[cfg(windows)]
        webview_data_dir: Some(data_dir.join("webview")),
        state: AppState::new(|| DataDir::from_flag(data_dir), crate::open_pane),
        tray: None,
        poll: true,
        window_visible: false,
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
    app.run_return(|_, _| {})
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
        if let Some(line) = line(&seen, &version) {
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

    fn seen(replied: &[&str], version: &str, service: &str) -> String {
        format!(
            "\"version={version} service={service} replied={}\"",
            replied.join(",")
        )
    }

    #[test]
    fn the_line_waits_for_every_read_command_the_version_and_a_read_service() {
        let all = ["capture_status", "inventory", "onboarding_state"];
        assert_eq!(
            line(&seen(&all, "0.0.0", "not_running"), "0.0.0").as_deref(),
            Some("page=loaded commands_ok=3 version=0.0.0 service=not_running")
        );
        for service in SERVICES {
            assert_eq!(
                line(&seen(&all, "0.0.0", service), "0.0.0"),
                Some(format!(
                    "page=loaded commands_ok=3 version=0.0.0 service={service}"
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
                        "onboarding_state"
                    ],
                    "0.0.0",
                    "unsupported"
                ),
                "0.0.0"
            )
            .as_deref(),
            Some("page=loaded commands_ok=3 version=0.0.0 service=unsupported")
        );
        assert_eq!(
            line(&seen(&all[..2], "0.0.0", "not_running"), "0.0.0"),
            None
        );
        assert_eq!(line(&seen(&all, "-", "not_running"), "0.0.0"), None);
        assert_eq!(line(&seen(&all, "0.0.1", "not_running"), "0.0.0"), None);
        assert_eq!(line(&seen(&all, "0.0.0", "connecting"), "0.0.0"), None);
        assert_eq!(line(&seen(&all, "0.0.0", "-"), "0.0.0"), None);
        assert_eq!(line(&seen(&all, "0.0.0", "capturing"), "0.0.0"), None);
        assert_eq!(line("", "0.0.0"), None);
        assert_eq!(line("\"not the probe\"", "0.0.0"), None);
        assert_eq!(
            line(
                "version=0.0.0 service=not_running replied=capture_status,inventory,onboarding_state",
                "0.0.0"
            ),
            None
        );
        assert_eq!(
            line(
                "\"version=0.0.0 replied=capture_status,inventory,onboarding_state\"",
                "0.0.0"
            ),
            None
        );
    }

    #[test]
    fn the_reads_are_those_of_the_capture_module() {
        assert_eq!(reads(), ["capture_status", "inventory", "onboarding_state"]);
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
