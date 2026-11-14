use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use halvo_capture::consent::onboarding::platform_probe;
use tauri::{AppHandle, Manager, Runtime, Wry};

use crate::app::{self, PageLoads};
use crate::commands::names;
use crate::launch::Exit;
use crate::unwired_store;

const PROBE: &str = "(function () { var ok = /^[A-Za-z0-9_:|.+-]+$/; var r = (window.Halvo && Halvo.ipc && Halvo.ipc.replied) ? Halvo.ipc.replied() : []; var v = (document.getElementById('version') || {}).textContent || ''; return 'version=' + (ok.test(v) ? v : '-') + ' replied=' + r.filter(function (n) { return ok.test(n); }).join(','); })()";

const POLL: Duration = Duration::from_millis(200);

pub fn reads() -> Vec<&'static str> {
    names::MODULES
        .iter()
        .flat_map(|module| module.reads.iter().copied())
        .collect()
}

/// The line that the check prints for `seen`, the JSON text of the probe's
/// string, or None while the page lacks the reply of a read command or shows
/// another version than `version`.
pub fn line(seen: &str, version: &str) -> Option<String> {
    let text = seen.strip_prefix('"')?.strip_suffix('"')?;
    let (shown, replied) = text.strip_prefix("version=")?.split_once(" replied=")?;
    let reads = reads();
    let ok = reads
        .iter()
        .filter(|name| replied.split(',').any(|r| r == **name))
        .count();
    (ok == reads.len() && shown == version)
        .then(|| format!("page=loaded commands_ok={ok} version={shown}"))
}

fn no_tray(_app: &AppHandle<Wry>) -> tauri::Result<()> {
    Ok(())
}

pub fn run(config_dir: PathBuf, within: Duration) -> i32 {
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
        config_dir: Some(config_dir.clone()),
        store: Box::new(unwired_store::UnwiredStore),
        store_backend: unwired_store::DESCRIPTION,
        probe: platform_probe(),
        open_pane: crate::open_pane,
        tray: no_tray,
        window_visible: false,
        #[cfg(windows)]
        webview_data_dir: Some(config_dir.join("webview")),
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

    fn seen(replied: &[&str], version: &str) -> String {
        format!("\"version={version} replied={}\"", replied.join(","))
    }

    #[test]
    fn the_line_waits_for_every_read_command_and_the_version() {
        let all = ["capture_status", "inventory", "onboarding_state"];
        assert_eq!(
            line(&seen(&all, "0.0.0"), "0.0.0").as_deref(),
            Some("page=loaded commands_ok=3 version=0.0.0")
        );
        assert_eq!(
            line(
                &seen(
                    &[
                        "inventory",
                        "plugin:app|version",
                        "capture_status",
                        "onboarding_state"
                    ],
                    "0.0.0"
                ),
                "0.0.0"
            )
            .as_deref(),
            Some("page=loaded commands_ok=3 version=0.0.0")
        );
        assert_eq!(line(&seen(&all[..2], "0.0.0"), "0.0.0"), None);
        assert_eq!(line(&seen(&all, "-"), "0.0.0"), None);
        assert_eq!(line(&seen(&all, "0.0.1"), "0.0.0"), None);
        assert_eq!(line("", "0.0.0"), None);
        assert_eq!(line("\"not the probe\"", "0.0.0"), None);
        assert_eq!(
            line(
                "version=0.0.0 replied=capture_status,inventory,onboarding_state",
                "0.0.0"
            ),
            None
        );
        assert_eq!(
            line(
                "\"version=0.0.0 replied=capture_status,inventory,onboarding_state\\\"\"",
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
