//! Embeds the Common Controls v6 manifest in the binaries that this package links on a Windows
//! MSVC target. In a workspace run tauri gets src-tauri's default feature
//! `common-controls-v6` also in this crate's test binaries, which then import entry points of
//! comctl32 v6; with no manifest such a binary exits 0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND) at
//! its start, as the test binary `commands` did in scratch run 43415707049. The link arguments
//! reach this package's own link steps alone, not those of another package.

fn main() {
    println!("cargo:rerun-if-changed=windows-tests.manifest");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "msvc" {
        return;
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let manifest = std::path::Path::new(&dir).join("windows-tests.manifest");
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
