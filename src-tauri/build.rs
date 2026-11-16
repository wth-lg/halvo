#[allow(dead_code)]
#[path = "src/commands/names.rs"]
mod names;

fn main() {
    println!("cargo:rerun-if-changed=src/commands/names.rs");
    println!("cargo:rerun-if-changed=windows-tests.manifest");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "msvc" {
        let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
        let manifest = std::path::Path::new(&dir).join("windows-tests.manifest");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
    let commands: Vec<&'static str> = names::MODULES
        .iter()
        .flat_map(|module| module.commands.iter().copied())
        .collect();
    let manifest = tauri_build::AppManifest::new().commands(commands.leak());
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("tauri-build fails");
}
