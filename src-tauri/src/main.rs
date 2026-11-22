#![deny(clippy::undocumented_unsafe_blocks)]
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod check_page;
mod commands;
#[cfg(any(target_os = "macos", test))]
mod daemon_child;
mod engine;
mod launch;
#[cfg(all(target_os = "macos", test))]
mod login_item;
mod poller;
#[cfg(any(target_os = "macos", test))]
mod quit;
mod tray;
mod view_state;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use commands::control::{control_of_child, ChildPid};
use halvo_control::onboarding::{open_settings_pane, Permission};
use halvo_control::paths::DataDir;

pub fn open_pane(permission: Permission) -> Result<(), String> {
    open_settings_pane(permission).map_err(|e| e.to_string())
}

fn main() {
    #[cfg(windows)]
    app::take_engine_arguments_out_of_the_environment();
    let started = std::time::Instant::now();
    match launch::parse(std::env::args_os().skip(1)) {
        Ok(launch::Launch::Version) => println!("{}", app::VERSION),
        Ok(launch::Launch::Run {
            background,
            data_dir,
        }) => run(background, data_dir),
        Ok(launch::Launch::CheckPage { data_dir, within }) => {
            std::process::exit(check_page::run(data_dir, within, started))
        }
        Err(usage) => {
            eprintln!("halvo: {usage}\n{}", launch::USAGE);
            std::process::exit(launch::Exit::Usage.code());
        }
    }
}

fn run(background: bool, given: Option<PathBuf>) {
    let pid = Arc::<ChildPid>::default();
    let (control, data_dir) = control_of_child(
        || match &given {
            Some(dir) => DataDir::from_flag(dir.clone()),
            None => DataDir::for_current_user(),
        },
        Arc::clone(&pid),
    );
    #[cfg(any(target_os = "macos", test))]
    let child_dir = data_dir.clone();
    let state = commands::capture::AppState::with_control(control, data_dir, open_pane);
    #[cfg(any(target_os = "macos", test))]
    let views = Arc::clone(&state.views);
    let parts = app::Parts {
        state,
        tray: Some(tray::install),
        guard: engine::guard,
        poll: true,
        window_visible: !background,
        engine_rules_dir: None,
        #[cfg(windows)]
        webview_data_dir: None,
    };
    let builder = tauri::Builder::default().manage(tray::QuitNotice::default());
    #[cfg(any(target_os = "macos", test))]
    let builder = quit::installed(builder);
    let app = match app::build(builder, parts).build(app::context()) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("halvo: the app did not start: {e}");
            std::process::exit(1);
        }
    };
    #[cfg(any(target_os = "macos", test))]
    tauri::Manager::manage(
        &app,
        quit::AppChildren(start_the_daemon(child_dir, given.is_some(), pid, views)),
    );
    app.run(|_, _| {});
}

/// The daemon's program: the file halvo-capture beside the app's own executable (contracts.md
/// C8 item 1).
#[cfg(any(target_os = "macos", test))]
fn daemon_program() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("halvo-capture"))
}

#[cfg(any(target_os = "macos", test))]
fn start_the_daemon(
    data_dir: Option<DataDir>,
    given: bool,
    pid: Arc<ChildPid>,
    views: Arc<poller::Views>,
) -> Option<Arc<dyn quit::Children>> {
    let plan = daemon_child::Plan {
        program: daemon_child::Program {
            path: daemon_program()?,
            leading: Vec::new(),
        },
        data_dir: data_dir?,
        given,
    };
    Some(Arc::new(daemon_child::Supervisor::start_holding(
        plan,
        pid,
        Arc::new(daemon_child::SystemClock),
        views,
    )))
}
