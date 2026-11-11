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

/// One read of a boolean activation attribute of a browser's application element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeRead {
    True,
    False,
    Absent,
    Failed(i32),
}

const AX_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
const AX_NO_VALUE: i32 = -25212;

/// Classifies the error code and the value of one attribute read: the codes
/// for an unsupported attribute and for no value mean that it is absent, and
/// every other code that is not 0 is a failed read.
pub fn attribute_read(error: i32, value: Option<bool>) -> AttributeRead {
    match (error, value) {
        (0, Some(true)) => AttributeRead::True,
        (0, Some(false)) => AttributeRead::False,
        (0 | AX_ATTRIBUTE_UNSUPPORTED | AX_NO_VALUE, _) => AttributeRead::Absent,
        (code, _) => AttributeRead::Failed(code),
    }
}

/// The state of activation before an address read, as the cell's `activation` records it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Activation {
    Set,
    Unset,
    #[default]
    ReadFailed,
    NoAttribute,
}

impl Activation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Activation::Set => "set",
            Activation::Unset => "unset",
            Activation::ReadFailed => "read-failed",
            Activation::NoAttribute => "no-attribute",
        }
    }

    /// Whether a run in this state counts in `readable_runs` when it returns the value:
    /// every attribute false or absent, or a system whose client can read no such attribute.
    pub fn allows_a_readable_run(&self) -> bool {
        matches!(self, Activation::Unset | Activation::NoAttribute)
    }
}

/// `set` when one read is true; else `read-failed` when one read failed or there is
/// no read; else `unset`.
pub fn activation_of(reads: &[AttributeRead]) -> Activation {
    if reads.contains(&AttributeRead::True) {
        Activation::Set
    } else if reads.is_empty()
        || reads
            .iter()
            .any(|read| matches!(read, AttributeRead::Failed(_)))
    {
        Activation::ReadFailed
    } else {
        Activation::Unset
    }
}

#[derive(Clone, Debug, Default)]
pub struct AddressRead {
    pub activation: Activation,
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
