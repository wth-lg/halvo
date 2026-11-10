use super::macos_ax as ax;
use super::macos_screen as screen;
use super::{
    marked, AddressRead, Browser, Focused, Own, Usage, WindowFacts, WALK_LEVELS, WALK_NODES,
};
use crate::frame::Frame;
use crate::tree::{line, walk, Read, Tree};
use std::collections::VecDeque;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

pub const RUNNER: &str = "macos-15";
pub const OS_ENGINES: [&str; 2] = ["apple-vision-accurate", "apple-vision-fast"];
pub const MEM_KIND: &str = "phys_footprint";
pub const PEAK_KIND: &str = "lifetime_max_phys_footprint";
pub const NAME_ATTR: &str = "AXTitle|AXDescription";
pub const VALUE_ATTR: &str = "AXValue";
pub const TREE_VIEW: &str = "AXChildren";

fn output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn system_version() -> String {
    format!(
        "{}-{}",
        output("sw_vers", &["-productVersion"]),
        output("sw_vers", &["-buildVersion"])
    )
}

pub fn display_size() -> (usize, usize) {
    ax::display_size()
}

pub fn set_display_size(width: usize, height: usize) -> Result<(usize, usize), String> {
    ax::set_display_size(width, height)
}

pub fn browsers() -> Vec<Browser> {
    let mut found = Vec::new();
    for (name, path) in [
        (
            "chrome",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ),
        (
            "edge",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ),
        (
            "firefox",
            "/Applications/Firefox.app/Contents/MacOS/firefox",
        ),
    ] {
        if Path::new(path).exists() {
            let version = output(path, &["--version"])
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_string();
            found.push(Browser {
                name: name.to_string(),
                path: path.to_string(),
                version,
            });
        }
    }
    if Path::new("/Applications/Safari.app").exists() {
        found.push(Browser {
            name: "safari".to_string(),
            path: "/Applications/Safari.app".to_string(),
            version: output(
                "defaults",
                &[
                    "read",
                    "/Applications/Safari.app/Contents/Info",
                    "CFBundleShortVersionString",
                ],
            ),
        });
    }
    found
}

pub fn granted() -> Result<(), String> {
    if ax::ax_trusted() {
        Ok(())
    } else {
        Err("the accessibility grant is missing for this process".to_string())
    }
}

pub fn resolve_pid(launched: u32, _title: &str) -> u32 {
    launched
}

pub fn pid_named(name: &str) -> Option<u32> {
    output("pgrep", &["-x", name])
        .lines()
        .next()
        .and_then(|line| line.trim().parse().ok())
}

pub fn tree_usage(pid: u32, webkit: bool) -> Usage {
    let mut pids = vec![pid as i32];
    pids.extend(ax::descendants(pid as i32));
    if webkit {
        for other in ax::all_pids() {
            if !pids.contains(&other) && ax::path_of(other).contains("/WebKit.framework/") {
                pids.push(other);
            }
        }
    }
    let mut total = Usage::default();
    for pid in pids {
        if let Some(usage) = ax::usage(pid) {
            total.cpu_ms += usage.cpu_ns as f64 / 1e6;
            total.mem_bytes += usage.footprint;
        }
    }
    total
}

pub fn own_usage() -> Own {
    ax::usage(std::process::id() as i32)
        .map(|usage| Own {
            cpu_ms: usage.cpu_ns as f64 / 1e6,
            peak_bytes: usage.peak_footprint,
        })
        .unwrap_or_default()
}

pub fn kill_tree(pid: u32) {
    if pid <= 1 {
        return;
    }
    let mut pids = ax::descendants(pid as i32);
    pids.push(pid as i32);
    for pid in pids {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

fn front_window(app: &ax::Element) -> Option<ax::Element> {
    ax::attr_element(app, "AXFocusedWindow")
        .or_else(|| ax::attr_element(app, "AXMainWindow"))
        .or_else(|| ax::windows(app).into_iter().next())
}

pub fn address_read(pid: u32, needle: &str, _title: &str) -> AddressRead {
    let app = ax::app(pid as i32);
    let set = ["AXEnhancedUserInterface", "AXManualAccessibility"]
        .iter()
        .any(|name| ax::attr_bool(&app, name) == Some(true));
    let mut read = AddressRead {
        activation: if set { "set" } else { "unset" }.to_string(),
        ..AddressRead::default()
    };
    let start = Instant::now();
    loop {
        read.attempts += 1;
        if !read.role_query && start.elapsed() > Duration::from_secs(2) {
            let _ = ax::attr_string(&app, "AXRole");
            read.role_query = true;
        }
        if let Some(window) = front_window(&app) {
            let mut queue = VecDeque::from([(window, 1usize)]);
            let mut seen = 0u32;
            while let Some((node, level)) = queue.pop_front() {
                seen += 1;
                let role = ax::attr_string(&node, "AXRole").unwrap_or_default();
                if role == "AXWebArea" {
                    continue;
                }
                if let Some(value) = ax::attr_string(&node, "AXValue") {
                    if value.contains(needle) {
                        read.value = Some(value);
                        read.role = role;
                        break;
                    }
                }
                if level < 12 && seen < 1500 {
                    for child in ax::children(&node) {
                        queue.push_back((child, level + 1));
                    }
                }
            }
            read.nodes = read.nodes.max(seen);
        }
        if read.value.is_some() || start.elapsed() > Duration::from_secs(7) {
            return read;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

pub fn activate(pid: u32) -> Result<String, String> {
    let app = ax::app(pid as i32);
    let enhanced = ax::set_attr_bool(&app, "AXEnhancedUserInterface", true);
    let manual = ax::set_attr_bool(&app, "AXManualAccessibility", true);
    Ok(format!(
        "AXEnhancedUserInterface({enhanced})+AXManualAccessibility({manual})"
    ))
}

struct AxTree;

impl Tree for AxTree {
    type Node = ax::Element;
    fn name(&self, node: &ax::Element) -> String {
        let _ = ax::attr_string(node, "AXRole");
        ax::attr_string(node, "AXTitle")
            .filter(|title| !title.is_empty())
            .or_else(|| ax::attr_string(node, "AXDescription"))
            .unwrap_or_default()
    }
    fn value(&self, node: &ax::Element) -> String {
        ax::attr_string(node, "AXValue").unwrap_or_default()
    }
    fn children(&self, node: &ax::Element) -> Vec<ax::Element> {
        ax::children(node)
    }
}

pub fn focused_read(pid: u32) -> Focused {
    let app = ax::app(pid as i32);
    match ax::attr_element(&app, "AXFocusedUIElement") {
        Some(element) => Focused {
            found: true,
            bytes: line(&AxTree.name(&element), &AxTree.value(&element))
                .map(|line| line.len())
                .unwrap_or(0),
        },
        None => Focused::default(),
    }
}

pub fn subtree_read(pid: u32, _title: &str) -> Result<Read, String> {
    let app = ax::app(pid as i32);
    let window = front_window(&app).ok_or("the application shows no window to this process")?;
    Ok(walk(&AxTree, &window))
}

fn facts(window: &ax::Element) -> WindowFacts {
    let mut facts = WindowFacts {
        title: ax::attr_string(window, "AXTitle").unwrap_or_default(),
        ..WindowFacts::default()
    };
    let role = ax::attr_string(window, "AXRole").unwrap_or_default();
    for name in ax::attr_names(window) {
        if name == "AXTitle" {
            continue;
        }
        if let Some(value) = ax::attr_string(window, &name).filter(|value| marked(value)) {
            facts
                .hits
                .push(format!("window|{role}|{name}|{}", value.to_lowercase()));
        }
    }
    let mut queue: VecDeque<(ax::Element, usize)> = ax::children(window)
        .into_iter()
        .map(|child| (child, 2))
        .collect();
    while let Some((node, level)) = queue.pop_front() {
        if facts.nodes >= WALK_NODES {
            break;
        }
        facts.nodes += 1;
        let role = ax::attr_string(&node, "AXRole").unwrap_or_default();
        if role == "AXWebArea" {
            continue;
        }
        for name in [
            "AXTitle",
            "AXDescription",
            "AXValue",
            "AXHelp",
            "AXIdentifier",
            "AXRoleDescription",
            "AXSubrole",
        ] {
            if let Some(value) = ax::attr_string(&node, name).filter(|value| marked(value)) {
                facts
                    .hits
                    .push(format!("descendant|{role}|{name}|{}", value.to_lowercase()));
            }
        }
        if level < WALK_LEVELS {
            for child in ax::children(&node) {
                queue.push_back((child, level + 1));
            }
        }
    }
    facts.hits.sort();
    facts.hits.dedup();
    facts
}

pub fn private_read(
    pid: u32,
    normal: &str,
    private: &str,
) -> Result<(WindowFacts, WindowFacts, bool), String> {
    let app = ax::app(pid as i32);
    let mut result = Err("no read was made".to_string());
    for pass in 0..2 {
        if pass == 1 {
            std::thread::sleep(Duration::from_millis(1500));
        }
        let windows = ax::windows(&app);
        let title = |window: &ax::Element| ax::attr_string(window, "AXTitle").unwrap_or_default();
        let Some(normal_window) = windows.iter().find(|w| title(w).contains(normal)) else {
            result = Err(format!(
                "no window shows the normal page among {} windows",
                windows.len()
            ));
            continue;
        };
        let (private_window, loaded) = match windows.iter().find(|w| title(w).contains(private)) {
            Some(window) => (window, true),
            None => match windows.iter().find(|w| !title(w).contains(normal)) {
                Some(window) => (window, false),
                None => {
                    result = Err(format!("no second window among {} windows", windows.len()));
                    continue;
                }
            },
        };
        result = Ok((facts(normal_window), facts(private_window), loaded));
    }
    result
}

pub fn press_menu(pid: u32, path: &[String]) -> Result<(), String> {
    let app = ax::app(pid as i32);
    let mut node =
        ax::attr_element(&app, "AXMenuBar").ok_or("the application shows no menu bar")?;
    for (step, title) in path.iter().enumerate() {
        let mut queue: VecDeque<(ax::Element, usize)> = ax::children(&node)
            .into_iter()
            .map(|child| (child, 1))
            .collect();
        let mut found = None;
        while let Some((child, level)) = queue.pop_front() {
            if ax::attr_string(&child, "AXTitle").as_deref() == Some(title.as_str()) {
                found = Some(child);
                break;
            }
            if level < 2 {
                for grandchild in ax::children(&child) {
                    queue.push_back((grandchild, level + 1));
                }
            }
        }
        node = found.ok_or_else(|| format!("no menu item at step {} of the path", step + 1))?;
    }
    match ax::perform(&node, "AXPress") {
        0 => Ok(()),
        error => Err(format!("AXPress: AXError {error}")),
    }
}

pub fn capture(cursor: bool) -> Result<(Frame, String), String> {
    let frame = screen::capture_main_display(15_000, cursor)?;
    Ok((
        frame,
        format!(
            "screencapturekit-preflight-{}",
            screen::screen_capture_preflight()
        ),
    ))
}

fn own_cpu_ms() -> f64 {
    own_usage().cpu_ms
}

pub fn recognize(engine: &str, frame: &Frame) -> Result<(Vec<String>, f64, f64), String> {
    screen::recognize(
        frame,
        engine == "apple-vision-accurate",
        true,
        &["en-US"],
        own_cpu_ms,
    )
}

pub fn engine_settings(engine: &str) -> Vec<(&'static str, String)> {
    let level = if engine == "apple-vision-accurate" {
        "accurate"
    } else {
        "fast"
    };
    vec![
        ("level", level.to_string()),
        ("languages", "en-US".to_string()),
        ("correction", "on".to_string()),
        ("revision", screen::text_request_revision().to_string()),
    ]
}

pub fn listen_preflight() -> Option<bool> {
    Some(ax::listen_event_preflight())
}

pub fn count_input(_ms: u64, _ready: &Path, _stop: &Path) -> Result<[u32; 4], String> {
    Err("Raw Input exists on Windows only".to_string())
}

pub fn send_input(_x: i32, _y: i32) -> Result<(i32, i32), String> {
    Err("the input cell runs on Windows only".to_string())
}

pub fn input_driver_version() -> String {
    String::new()
}
