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
mod instance;
mod launch;
#[cfg(target_os = "macos")]
mod login_item;
mod poller;
#[cfg(any(target_os = "macos", test))]
mod quit;
mod tray;
mod view_state;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
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
        Ok(launch::Launch::WriteLoginItem { label, data_dir }) => {
            std::process::exit(write_login_item(&label, &data_dir).code())
        }
        Err(usage) => {
            eprintln!("halvo: {usage}\n{}", launch::USAGE);
            std::process::exit(launch::Exit::Usage.code());
        }
    }
}

#[cfg(target_os = "macos")]
fn write_login_item(label: &str, data_dir: &Path) -> launch::Exit {
    login_item::write_scratch(
        label,
        data_dir,
        std::env::current_exe(),
        &halvo_control::paths::SystemAccount,
    )
}

#[cfg(not(target_os = "macos"))]
fn write_login_item(label: &str, data_dir: &Path) -> launch::Exit {
    eprintln!(
        "halvo: --write-login-item writes a macOS login item ({label} into {}), and this system has none",
        data_dir.display()
    );
    launch::Exit::LoginItemNotWritten
}

fn run(background: bool, given: Option<PathBuf>) {
    let resolved = match &given {
        Some(dir) => DataDir::from_flag(dir.clone()),
        None => DataDir::for_current_user(),
    };
    let lock = match resolved.as_ref().map(|dir| instance::take(dir, background)) {
        Ok(Ok(instance::Taken::First(lock))) => Some(lock),
        Ok(Ok(instance::Taken::Second)) => return,
        Ok(Err(e)) => {
            eprintln!("halvo: this Halvo runs without its lock, which could not be taken: {e}");
            None
        }
        Err(_) => {
            eprintln!("halvo: this Halvo runs without its lock, because it has no data directory");
            None
        }
    };
    let pid = Arc::<ChildPid>::default();
    let (control, data_dir) = control_of_child(|| resolved, Arc::clone(&pid));
    #[cfg(any(target_os = "macos", test))]
    let child_dir = data_dir.clone();
    #[cfg(target_os = "macos")]
    let login_dir = data_dir.clone();
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut state = commands::capture::AppState::with_control(control, data_dir, open_pane);
    #[cfg(target_os = "macos")]
    {
        state.login_item = login_item::at_the_click(given.is_some(), the_login_item);
    }
    #[cfg(any(target_os = "macos", test))]
    let views = Arc::clone(&state.views);
    #[cfg(target_os = "macos")]
    let login_views = Arc::clone(&state.views);
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
    if let Some(lock) = &lock {
        let handle = app.handle().clone();
        lock.show_on_ask(move || tray::show_main_window(&handle));
    }
    #[cfg(any(target_os = "macos", test))]
    tauri::Manager::manage(
        &app,
        quit::AppChildren(start_the_daemon(child_dir, given.is_some(), pid, views)),
    );
    #[cfg(target_os = "macos")]
    start_the_login_item(login_dir, given.is_some(), login_views);
    app.run(|_, _| {});
}

#[cfg(target_os = "macos")]
fn start_the_login_item(data_dir: Option<DataDir>, given: bool, views: Arc<poller::Views>) {
    let Some(data_dir) = data_dir else {
        return;
    };
    std::thread::spawn(move || {
        login_item::at_start(&views, &data_dir, given, the_login_item, &Launchctl);
    });
}

/// The manager of the app's own login item, for its own path and the account's LaunchAgents
/// directory; none where either cannot be read or the manager refuses the program.
#[cfg(target_os = "macos")]
fn the_login_item() -> Option<login_item::LoginItem> {
    login_item::LoginItem::new(
        halvo_control::service::LOGIN_ITEM_LABEL,
        &std::env::current_exe().ok()?,
        halvo_control::paths::LoginItemDir::for_current_user().ok()?,
        None,
    )
    .ok()
}

#[cfg(target_os = "macos")]
const LAUNCHCTL: &str = "/bin/launchctl";

#[cfg(target_os = "macos")]
const LAUNCHCTL_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(target_os = "macos")]
struct Launchctl;

#[cfg(target_os = "macos")]
impl login_item::Runner for Launchctl {
    fn status(&self, args: &[String]) -> Option<i32> {
        use daemon_child::Watched;
        let program = daemon_child::Program {
            path: PathBuf::from(LAUNCHCTL),
            leading: Vec::new(),
        };
        let args: Vec<std::ffi::OsString> = args.iter().map(std::ffi::OsString::from).collect();
        let err = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .ok()?;
        let mut child = daemon_child::start_child(&program, &args, err).ok()?;
        child.close_lifeline();
        let until = std::time::Instant::now() + LAUNCHCTL_WAIT;
        loop {
            match child.ended() {
                Ok(Some(daemon_child::Ending::Status(code))) => return Some(code),
                Ok(Some(_)) => return None,
                Ok(None) if std::time::Instant::now() < until => {
                    std::thread::sleep(daemon_child::STOP_POLL);
                }
                Ok(None) | Err(_) => {
                    let _ = child.kill();
                    return None;
                }
            }
        }
    }
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
        consented: false,
    };
    Some(Arc::new(daemon_child::Supervisor::start_holding(
        plan,
        pid,
        Arc::new(daemon_child::SystemClock),
        views,
    )))
}
