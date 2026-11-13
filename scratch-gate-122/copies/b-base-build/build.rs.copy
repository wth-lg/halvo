#[allow(dead_code)]
#[path = "src/commands/names.rs"]
mod names;

fn main() {
    println!("cargo:rerun-if-changed=src/commands/names.rs");
    let commands: Vec<&'static str> = names::MODULES
        .iter()
        .flat_map(|module| module.commands.iter().copied())
        .collect();
    let manifest = tauri_build::AppManifest::new().commands(commands.leak());
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("tauri-build fails");
}
