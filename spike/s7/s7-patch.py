"""Probe patch for S7 (scratch only, never committed).

Applies three changes to src-tauri/src/main.rs of the worktree given as argv[1]:
1. the app takes the Accessory policy and does not activate itself at launch, so a
   start on the development Mac leaves the front application as it was;
2. stderr lines `spike tray=installed ms=<n>` after tray::install returns Ok, and
   `spike page_load=finished ms=<n> label=<window>` when the page-load event arrives;
3. at page load the page's three setInterval timers are cleared ("timers stopped").
Each ms value counts from the start of main().
"""
import pathlib
import sys

path = pathlib.Path(sys.argv[1]) / "src-tauri" / "src" / "main.rs"
text = path.read_text()

edits = [
    (
        "fn main() {\n",
        "static SPIKE_T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();\n"
        "fn spike_ms() -> u128 {\n"
        "    SPIKE_T0.get_or_init(std::time::Instant::now).elapsed().as_millis()\n"
        "}\n\n"
        "fn main() {\n"
        "    let _ = spike_ms();\n",
    ),
    (
        "    tauri::Builder::default()\n",
        "    let builder = tauri::Builder::default();\n"
        "    #[cfg(target_os = \"macos\")]\n"
        "    let builder = builder.activate_ignoring_other_apps(false);\n"
        "    builder\n",
    ),
    (
        "        .setup(|app| {\n",
        "        .setup(|app| {\n"
        "            #[cfg(target_os = \"macos\")]\n"
        "            app.set_activation_policy(tauri::ActivationPolicy::Accessory);\n",
    ),
    (
        "            tray::install(app.handle())?;\n",
        "            tray::install(app.handle())?;\n"
        "            eprintln!(\"spike tray=installed ms={}\", spike_ms());\n",
    ),
    (
        "            if payload.event() == PageLoadEvent::Finished {\n",
        "            if payload.event() == PageLoadEvent::Finished {\n"
        "                eprintln!(\"spike page_load=finished ms={} label={}\", spike_ms(), webview.label());\n"
        "                let _ = webview.eval(\"for (let i = 1; i < 1000; i++) { clearInterval(i); }\");\n",
    ),
]
for old, new in edits:
    if text.count(old) != 1:
        sys.exit(f"anchor not found exactly once: {old!r}")
    text = text.replace(old, new)
path.write_text(text)
print(f"patched {path.name}: {len(edits)} edits")
