use r4_trial_app::pages::*;
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("pages-{name}-{}", std::process::id()));
    if !dir.exists() {
        fs::create_dir_all(dir.join("pages/sub")).unwrap();
        fs::write(dir.join("pages/index.html"), "<p>index</p>").unwrap();
        fs::write(dir.join("pages/sub/view.js"), "let a = 1;").unwrap();
        fs::write(dir.join("pages/data.bin"), [0u8, 1, 2]).unwrap();
        fs::write(dir.join("outside.html"), "outside").unwrap();
    }
    dir
}

#[test]
fn index_and_files_are_served_with_their_types() {
    let dir = scratch("files");
    let pages = dir.join("pages");
    let index = serve(&pages, "/");
    assert_eq!(index.status, 200);
    assert_eq!(index.content_type, "text/html; charset=utf-8");
    assert_eq!(index.body, b"<p>index</p>");
    assert_eq!(serve(&pages, "/index.html"), index);
    let js = serve(&pages, "/sub/view.js");
    assert_eq!(js.status, 200);
    assert_eq!(js.content_type, "text/javascript; charset=utf-8");
    let bin = serve(&pages, "/data.bin");
    assert_eq!(bin.content_type, "application/octet-stream");
    assert_eq!(bin.body, vec![0u8, 1, 2]);
}

#[test]
fn call_js_is_served_for_any_pages_directory() {
    assert_eq!(CALL_JS_PATH, "/__app/call.js");
    let dir = scratch("calljs");
    for pages in [dir.join("pages"), dir.join("no-such-dir")] {
        let served = serve(&pages, "/__app/call.js");
        assert_eq!(served.status, 200);
        assert_eq!(served.content_type, "text/javascript; charset=utf-8");
        assert_eq!(served.body, CALL_JS.as_bytes());
    }
}

#[test]
fn missing_and_malformed_paths_are_404() {
    let dir = scratch("missing");
    let pages = dir.join("pages");
    for path in [
        "/missing.html",
        "/sub",
        "/sub/",
        "/../outside.html",
        "/sub/../index.html",
        "/./index.html",
        "//index.html",
        "/index.html%00",
        "/%2e%2e/outside.html",
        "",
        "index.html",
    ] {
        assert_eq!(serve(&pages, path).status, 404, "{path:?}");
    }
    assert_eq!(serve(&dir.join("no-such-dir"), "/").status, 404);
}

#[cfg(unix)]
#[test]
fn a_link_that_leaves_the_directory_is_403() {
    let dir = scratch("link");
    let link = dir.join("pages/away.html");
    if !link.exists() {
        std::os::unix::fs::symlink(dir.join("outside.html"), &link).unwrap();
    }
    assert_eq!(serve(&dir.join("pages"), "/away.html").status, 403);
}

#[test]
fn the_page_url_uses_the_scheme_of_this_system() {
    let url = page_url("probe.html");
    if cfg!(windows) {
        assert_eq!(url, "http://r4trial.localhost/probe.html");
    } else {
        assert_eq!(url, "r4trial://localhost/probe.html");
    }
}

fn own_page(file: &str) -> String {
    fs::read_to_string(r4_trial_app::default_pages_dir().join(file)).unwrap()
}

#[test]
fn the_trial_page_has_twenty_rows_the_button_and_the_count() {
    let html = own_page("index.html");
    assert_eq!(html.matches("data-row=").count(), 20);
    for i in 1..=20 {
        assert!(html.contains(&format!("data-row=\"{i}\"")), "row {i}");
    }
    assert_eq!(html.matches("id=\"act\"").count(), 1);
    assert_eq!(html.matches("id=\"count\"").count(), 1);
    assert!(html.contains("<script src=\"/__app/call.js\"></script>"));
    assert!(html.contains("<script src=\"page.js\"></script>"));
    let page = own_page("page.js");
    for name in [
        "setInterval(tick, 100)",
        "requestAnimationFrame(frame)",
        "'bump'",
        "'r4-window'",
        "'next'",
        "'subscribe'",
        "'r4-change'",
    ] {
        assert!(page.contains(name), "page.js lacks {name}");
    }
}

#[test]
fn call_js_holds_its_functions() {
    for name in [
        "window.__TAURI_INTERNALS__",
        "internals.invoke(",
        "window.__r4Window",
        "new CustomEvent('r4-window'",
        "window.__r4AppFrame",
        "window.__r4AppCounts",
        "'__CHANNEL__:'",
        "'plugin:event|listen'",
    ] {
        assert!(CALL_JS.contains(name), "call.js lacks {name}");
    }
}
