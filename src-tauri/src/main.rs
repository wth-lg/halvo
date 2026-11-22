#![deny(clippy::undocumented_unsafe_blocks)]
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod check_page;
mod commands;
mod console;
#[cfg(any(target_os = "macos", windows, test))]
mod daemon_child;
mod engine;
mod instance;
mod launch;
#[cfg(target_os = "macos")]
mod login_item;
mod poller;
#[cfg(any(target_os = "macos", windows, test))]
mod quit;
#[cfg(target_os = "macos")]
mod remove;
mod stop_hook;
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
    stop_hook::install(stop_hook::system);
    let started = std::time::Instant::now();
    match launch::parse(std::env::args_os().skip(1)) {
        Ok(launch::Launch::Version) => console::print_version(app::VERSION),
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
        Ok(launch::Launch::DeleteAll { data_dir }) => {
            std::process::exit(delete_all_of_a_scratch_dir(&data_dir).code())
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

/// The launch form `--delete-all --data-dir DIR`, in the scratch form of `--write-login-item`:
/// it refuses with 64, before any read of DIR's entries, the default data directory, a directory
/// inside it and one that holds it; otherwise it runs the app's own entry of the offline delete
/// with no child and no module, prints its answer as JSON, and exits 0 on deleted or no_store
/// and 1 otherwise.
fn delete_all_of_a_scratch_dir(data_dir: &Path) -> launch::Exit {
    use commands::capture::{run_offline, Before, Offline, OFFLINE_WAIT};
    use view_state::DeleteView;
    let scratch =
        halvo_control::paths::ScratchDir::new(data_dir, &halvo_control::paths::SystemAccount)
            .and_then(|_| DataDir::from_flag(data_dir));
    let dir = match scratch {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!(
                "halvo: the scratch form does not take {}: {error}",
                data_dir.display()
            );
            return launch::Exit::Usage;
        }
    };
    let Some(program) = daemon_program() else {
        eprintln!("halvo: the app's own path cannot be read");
        return launch::Exit::NotDeleted;
    };
    let offline = Offline {
        program,
        data_dir: dir,
        children: None,
        run: Arc::new(|program, args| run_to_its_end(program, args, OFFLINE_WAIT)),
    };
    let before = Before {
        modules: Vec::new(),
        runs: Arc::default(),
        core: None,
    };
    let answer = run_offline(&offline, &before);
    let printed = match &answer {
        Ok(view) => serde_json::to_string(view),
        Err(error) => serde_json::to_string(error),
    };
    println!("{}", printed.unwrap_or_default());
    match answer {
        Ok(DeleteView::Deleted { .. } | DeleteView::NoStore { .. }) => launch::Exit::Deleted,
        Ok(_) | Err(_) => launch::Exit::NotDeleted,
    }
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
    #[cfg(target_os = "macos")]
    if let (None, Ok(dir)) = (&given, &resolved) {
        remove::old_consent_at_start(&halvo_control::paths::SystemAccount, dir);
    }
    let pid = Arc::<ChildPid>::default();
    let (control, data_dir) = control_of_child(|| resolved, Arc::clone(&pid));
    #[cfg(any(target_os = "macos", windows, test))]
    let child_dir = data_dir.clone();
    #[cfg(target_os = "macos")]
    let login_dir = data_dir.clone();
    #[cfg(target_os = "macos")]
    let offline_dir = data_dir.clone();
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut state = commands::capture::AppState::with_control(control, data_dir, open_pane);
    #[cfg(target_os = "macos")]
    {
        state.login_item = login_item::at_the_click(given.is_some(), the_login_item);
    }
    #[cfg(any(target_os = "macos", windows, test))]
    let views = Arc::clone(&state.views);
    #[cfg(target_os = "macos")]
    let login_views = Arc::clone(&state.views);
    #[cfg(target_os = "macos")]
    let (offline, offline_views) = (Arc::clone(&state.offline), Arc::clone(&state.views));
    #[cfg(target_os = "macos")]
    let (removal, removal_views) = (Arc::clone(&state.removal), Arc::clone(&state.views));
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
    #[cfg(any(target_os = "macos", windows, test))]
    let builder = quit::installed(builder);
    let app = match app::build(builder, parts).build(app::context()) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("halvo: the app did not start: {e}");
            stop_hook::stopped();
            std::process::exit(1);
        }
    };
    if let Some(lock) = &lock {
        let handle = app.handle().clone();
        lock.show_on_ask(move || tray::show_main_window(&handle));
    }
    #[cfg(any(target_os = "macos", windows, test))]
    let supervisor = start_the_daemon(child_dir, given.is_some(), pid, views);
    #[cfg(target_os = "macos")]
    set_the_offline_delete(&offline, &offline_views, supervisor.clone(), offline_dir);
    #[cfg(target_os = "macos")]
    {
        let wiring = remove::Wiring {
            account: Arc::new(halvo_control::paths::SystemAccount),
            given: given.is_some(),
            children: supervisor
                .clone()
                .map(|children| children as Arc<dyn remove::TheChild>),
            views: removal_views,
        };
        let _ = removal.set(Arc::new(move |data_dir, offline, before| {
            remove::run(&wiring, data_dir, offline, before)
        }));
    }
    #[cfg(any(target_os = "macos", windows, test))]
    tauri::Manager::manage(
        &app,
        quit::AppChildren(supervisor.map(|children| children as Arc<dyn quit::Children>)),
    );
    #[cfg(target_os = "macos")]
    start_the_login_item(login_dir, given.is_some(), login_views);
    #[cfg(target_os = "macos")]
    app.run(|app, event| {
        if let tauri::RunEvent::Reopen { .. } = event {
            tray::show_main_window(app);
        }
    });
    #[cfg(not(target_os = "macos"))]
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

/// How often the app reads whether the offline delete has ended.
const OFFLINE_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// Runs the offline delete's `program` with `args` to its end (contracts.md C3.3): standard
/// input from the null device, standard output read whole, standard error the app's own, and
/// every other descriptor closed at its exec; after `within` it kills the program and answers
/// that it had no end.
fn run_to_its_end(
    program: &Path,
    args: &[std::ffi::OsString],
    within: std::time::Duration,
) -> view_state::OfflineEnd {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;
    use view_state::OfflineEnd;
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(target_os = "macos")]
    daemon_child::only_the_standard_streams(&mut command);
    let Ok(mut child) = command.spawn() else {
        return OfflineEnd::NotStarted;
    };
    let reader = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = out.read_to_end(&mut text);
            String::from_utf8_lossy(&text).into_owned()
        })
    });
    let until = Instant::now() + within;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < until => std::thread::sleep(OFFLINE_POLL),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    match status.map(|status| status.code()) {
        None => OfflineEnd::NoEnd,
        Some(Some(code)) => OfflineEnd::Exited { code, out },
        Some(None) => OfflineEnd::Signal { out },
    }
}

/// The daemon's program: the file halvo-capture beside the app's own executable (contracts.md
/// C8 item 1), with the system's suffix of an executable.
fn daemon_program() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name(format!("halvo-capture{}", std::env::consts::EXE_SUFFIX)))
}

/// The app's own supervisor of its daemon child.
#[cfg(any(target_os = "macos", windows, test))]
type AppSupervisor = daemon_child::Supervisor<daemon_child::Running, daemon_child::SystemClock>;

/// The offline delete of the app (contracts.md C10.5): halvo-capture beside the app's own
/// executable on the app's data directory, held by the app's supervisor; every view names that
/// folder as resolved. It takes the state's cell and views from before the state moves into the
/// app, and reads no managed state, which the setup hook manages only inside `run`.
#[cfg(target_os = "macos")]
fn set_the_offline_delete(
    offline: &std::sync::OnceLock<commands::capture::Offline>,
    views: &poller::Views,
    supervisor: Option<Arc<AppSupervisor>>,
    data_dir: Option<DataDir>,
) {
    let Some(data_dir) = data_dir else {
        return;
    };
    let resolved =
        std::fs::canonicalize(data_dir.path()).unwrap_or_else(|_| data_dir.path().to_path_buf());
    views
        .hold()
        .set_folder(halvo_control::paths::one_line(&resolved.to_string_lossy()));
    let Some(program) = daemon_program() else {
        return;
    };
    let _ = offline.set(commands::capture::Offline {
        program,
        data_dir,
        children: supervisor
            .map(|children| children as Arc<dyn commands::capture::WithoutTheChild>),
        run: Arc::new(|program, args| {
            run_to_its_end(program, args, commands::capture::OFFLINE_WAIT)
        }),
    });
}

#[cfg(any(target_os = "macos", windows, test))]
fn start_the_daemon(
    data_dir: Option<DataDir>,
    given: bool,
    pid: Arc<ChildPid>,
    views: Arc<poller::Views>,
) -> Option<Arc<AppSupervisor>> {
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
