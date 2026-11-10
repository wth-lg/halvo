use super::windows_input as input;
use super::windows_screen as screen;
use super::windows_uia as uia;
use super::{
    marked, AddressRead, Browser, Focused, Own, Usage, WindowFacts, WALK_LEVELS, WALK_NODES,
};
use crate::frame::Frame;
use crate::tree::{line, walk, Read, Tree};
use std::collections::VecDeque;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use windows::Win32::UI::Accessibility::{IUIAutomation, IUIAutomationElement};

pub const RUNNER: &str = "windows-2025";
pub const OS_ENGINES: [&str; 1] = ["windows-media-ocr"];
pub const MEM_KIND: &str = "private_usage";
pub const PEAK_KIND: &str = "peak_pagefile_usage";
pub const NAME_ATTR: &str = "Name";
pub const VALUE_ATTR: &str = "ValuePattern.Value";
pub const TREE_VIEW: &str = "ControlView";
const DOCUMENT: i32 = 50030;

type Window = (isize, u32, String, String);

pub fn system_version() -> String {
    screen::windows_build()
}

pub fn display_size() -> (usize, usize) {
    let (width, height) = uia::display_size();
    (width as usize, height as usize)
}

pub fn set_display_size(width: usize, height: usize) -> Result<(usize, usize), String> {
    uia::set_display_size(width as u32, height as u32)
        .map(|(width, height)| (width as usize, height as usize))
}

fn product_version(path: &str) -> String {
    let script = format!("(Get-Item -LiteralPath '{path}').VersionInfo.ProductVersion");
    Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn browsers() -> Vec<Browser> {
    let candidates = [
        (
            "chrome",
            [
                "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
                "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
            ],
        ),
        (
            "edge",
            [
                "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
                "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
            ],
        ),
        (
            "firefox",
            [
                "C:\\Program Files\\Mozilla Firefox\\firefox.exe",
                "C:\\Program Files (x86)\\Mozilla Firefox\\firefox.exe",
            ],
        ),
    ];
    candidates
        .iter()
        .filter_map(|(name, paths)| {
            let path = paths.iter().find(|path| Path::new(path).exists())?;
            Some(Browser {
                name: name.to_string(),
                path: path.to_string(),
                version: product_version(path),
            })
        })
        .collect()
}

pub fn granted() -> Result<(), String> {
    Ok(())
}

fn window_titled(needle: &str) -> Option<Window> {
    uia::top_windows(0)
        .into_iter()
        .find(|(_, _, title, _)| title.contains(needle))
}

pub fn resolve_pid(launched: u32, title: &str) -> u32 {
    window_titled(title)
        .map(|window| window.1)
        .unwrap_or(launched)
}

pub fn pid_named(_name: &str) -> Option<u32> {
    None
}

pub fn tree_usage(pid: u32, _webkit: bool) -> Usage {
    let mut total = Usage::default();
    for pid in uia::process_tree(pid) {
        if let Some(usage) = uia::process_usage(pid) {
            total.cpu_ms += usage.cpu_ms;
            total.mem_bytes += usage.private_bytes;
        }
    }
    total
}

pub fn own_usage() -> Own {
    uia::process_usage(std::process::id())
        .map(|usage| Own {
            cpu_ms: usage.cpu_ms,
            peak_bytes: usage.peak_pagefile_bytes,
        })
        .unwrap_or_default()
}

pub fn kill_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

pub fn address_read(_pid: u32, needle: &str, title: &str) -> AddressRead {
    let mut read = AddressRead {
        activation: "not-readable".to_string(),
        ..AddressRead::default()
    };
    let start = Instant::now();
    let mut automation: Option<IUIAutomation> = None;
    loop {
        read.attempts += 1;
        if automation.is_none() {
            automation = uia::uia().ok();
        }
        let root = match (&automation, window_titled(title)) {
            (Some(a), Some(window)) => uia::element_from_hwnd(a, window.0).ok(),
            _ => None,
        };
        if let (Some(a), Some(root)) = (&automation, root) {
            let mut queue = VecDeque::from([(root, 1usize)]);
            let mut seen = 0u32;
            while let Some((node, level)) = queue.pop_front() {
                seen += 1;
                let kind = uia::control_type(&node);
                if kind == DOCUMENT {
                    continue;
                }
                if let Some(value) = uia::value(&node) {
                    if value.contains(needle) {
                        read.value = Some(value);
                        read.role = format!("control-type-{kind}");
                        break;
                    }
                }
                if level < 12 && seen < 1500 {
                    for child in uia::children(a, &node) {
                        queue.push_back((child, level + 1));
                    }
                }
            }
            read.nodes = read.nodes.max(seen);
        }
        if read.value.is_some() || start.elapsed() > Duration::from_secs(5) {
            return read;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

pub fn activate(_pid: u32) -> Result<String, String> {
    Err("no attribute turns the tree on for a client on Windows".to_string())
}

struct UiaTree {
    automation: IUIAutomation,
}

impl Tree for UiaTree {
    type Node = IUIAutomationElement;
    fn name(&self, node: &IUIAutomationElement) -> String {
        uia::name(node)
    }
    fn value(&self, node: &IUIAutomationElement) -> String {
        uia::value(node).unwrap_or_default()
    }
    fn children(&self, node: &IUIAutomationElement) -> Vec<IUIAutomationElement> {
        uia::children(&self.automation, node)
    }
}

pub fn focused_read(_pid: u32) -> Focused {
    let Ok(automation) = uia::uia() else {
        return Focused::default();
    };
    match uia::focused(&automation) {
        Ok(element) => Focused {
            found: true,
            bytes: line(
                &uia::name(&element),
                &uia::value(&element).unwrap_or_default(),
            )
            .map(|line| line.len())
            .unwrap_or(0),
        },
        Err(_) => Focused::default(),
    }
}

pub fn subtree_read(_pid: u32, title: &str) -> Result<Read, String> {
    let automation = uia::uia()?;
    let window = window_titled(title).ok_or("no visible window carries the title")?;
    let root = uia::element_from_hwnd(&automation, window.0)?;
    Ok(walk(&UiaTree { automation }, &root))
}

fn facts(automation: &IUIAutomation, window: &Window) -> WindowFacts {
    let mut facts = WindowFacts {
        title: window.2.clone(),
        class: window.3.clone(),
        ..WindowFacts::default()
    };
    let Ok(root) = uia::element_from_hwnd(automation, window.0) else {
        return facts;
    };
    let properties = |element: &IUIAutomationElement| {
        [
            ("Name", uia::name(element)),
            ("ClassName", uia::class_name(element)),
            ("AutomationId", uia::automation_id(element)),
            ("HelpText", uia::help_text(element)),
            ("Value", uia::value(element).unwrap_or_default()),
        ]
    };
    let kind = uia::control_type(&root);
    for (name, value) in properties(&root) {
        if marked(&value) {
            facts
                .hits
                .push(format!("window|{kind}|{name}|{}", value.to_lowercase()));
        }
    }
    let mut queue: VecDeque<(IUIAutomationElement, usize)> = uia::children(automation, &root)
        .into_iter()
        .map(|child| (child, 2))
        .collect();
    while let Some((node, level)) = queue.pop_front() {
        if facts.nodes >= WALK_NODES {
            break;
        }
        facts.nodes += 1;
        let kind = uia::control_type(&node);
        if kind == DOCUMENT {
            continue;
        }
        for (name, value) in properties(&node) {
            if marked(&value) {
                facts
                    .hits
                    .push(format!("descendant|{kind}|{name}|{}", value.to_lowercase()));
            }
        }
        if level < WALK_LEVELS {
            for child in uia::children(automation, &node) {
                queue.push_back((child, level + 1));
            }
        }
    }
    facts.hits.sort();
    facts.hits.dedup();
    facts
}

pub fn private_read(
    _pid: u32,
    normal: &str,
    private: &str,
) -> Result<(WindowFacts, WindowFacts, bool), String> {
    let automation = uia::uia()?;
    let normal_window = window_titled(normal).ok_or("no visible window shows the normal page")?;
    let private_window =
        window_titled(private).ok_or("no visible window shows the private page")?;
    let _ = facts(&automation, &private_window);
    std::thread::sleep(Duration::from_millis(1500));
    Ok((
        facts(&automation, &normal_window),
        facts(&automation, &private_window),
        true,
    ))
}

pub fn press_menu(_pid: u32, _path: &[String]) -> Result<(), String> {
    Err("no cell presses a menu item on Windows".to_string())
}

pub fn capture() -> Result<(Frame, String), String> {
    screen::capture_with_adapter(15_000)
}

pub fn recognize(_engine: &str, frame: &Frame) -> Result<(Vec<String>, f64), String> {
    screen::recognize(frame, Some("en-US"))
}

pub fn engine_settings(_engine: &str) -> Vec<(&'static str, String)> {
    vec![
        ("languages", "en-US".to_string()),
        ("installed_languages", screen::ocr_languages().join(",")),
    ]
}

pub fn listen_preflight() -> Option<bool> {
    None
}

pub fn count_input(ms: u64, ready: &Path, stop: &Path) -> Result<[u32; 4], String> {
    input::count_raw_input(ms, ready, stop)
}

pub fn send_input(x: i32, y: i32) -> Result<(), String> {
    input::send_os_input(x, y, 40)
}

pub fn input_driver_version() -> String {
    let lock = include_str!("../../Cargo.lock");
    lock.split("[[package]]")
        .find(|block| block.contains("name = \"enigo\""))
        .and_then(|block| {
            block
                .lines()
                .find_map(|line| line.strip_prefix("version = "))
        })
        .map(|version| version.trim_matches('"').to_string())
        .unwrap_or_default()
}
