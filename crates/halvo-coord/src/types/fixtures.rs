//! The page's fixtures (I8.3), written from the Rust types.

use super::commands;
use crate::words;
use std::path::Path;

/// Writes the fixture files into `dir`, which it makes if needed, and returns their count.
pub fn write(dir: &Path) -> std::io::Result<u32> {
    std::fs::create_dir_all(dir)?;
    let files = [
        ("commands.json", commands::json()),
        ("words.json", words::export()),
    ];
    for (name, text) in &files {
        std::fs::write(dir.join(name), text)?;
    }
    Ok(files.len() as u32)
}
