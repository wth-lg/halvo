use tauri::webview::PageLoadEvent;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    if std::env::args().skip(1).any(|arg| arg == "--version") {
        println!("{VERSION}");
        return;
    }

    tauri::Builder::default()
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = webview.window().set_title(&format!("Halvo {VERSION}"));
                let _ = webview.eval(format!(
                    "document.getElementById('version').textContent = '{VERSION}';"
                ));
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run the Halvo window");
}
