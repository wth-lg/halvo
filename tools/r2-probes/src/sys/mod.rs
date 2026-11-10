use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct Browser {
    pub name: String,
    pub path: String,
    pub version: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub cpu_ms: f64,
    pub mem_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Own {
    pub cpu_ms: f64,
    pub peak_bytes: u64,
}

#[derive(Clone, Debug, Default)]
pub struct AddressRead {
    pub activation: String,
    pub value: Option<String>,
    pub role: String,
    pub attempts: u32,
    pub nodes: u32,
    pub role_query: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Focused {
    pub found: bool,
    pub bytes: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WindowFacts {
    pub title: String,
    pub class: String,
    pub hits: Vec<String>,
    pub nodes: u32,
}

pub const MARKERS: [&str; 3] = ["incognito", "inprivate", "private"];
pub const WALK_LEVELS: usize = 8;
pub const WALK_NODES: u32 = 600;

pub fn marked(text: &str) -> bool {
    let lower = text.to_lowercase();
    MARKERS.iter().any(|marker| lower.contains(marker))
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_ax;
#[cfg(target_os = "macos")]
mod macos_screen;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_input;
#[cfg(windows)]
mod windows_screen;
#[cfg(windows)]
mod windows_uia;
#[cfg(windows)]
pub use windows::*;
