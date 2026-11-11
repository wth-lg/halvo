use std::path::{Path, PathBuf};

use tauri::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use tauri::http::Response;

/// The scheme of the app's own pages.
pub const SCHEME: &str = "r4trial";

/// The path under which every pages directory gets the app's own `call.js`.
pub const CALL_JS_PATH: &str = "/__app/call.js";

/// The app's `call.js`: the call function over `window.__TAURI_INTERNALS__`, the report of the
/// first animation frame after a show, `__r4Window(state)` and the page's two counters.
pub const CALL_JS: &str = include_str!("call.js");

/// One answer of the scheme: a status, a content type and a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// Answers the request path `path` from `pages`: `/` is `index.html`, `CALL_JS_PATH` is
/// `CALL_JS`, a file whose real path is under the real path of `pages` is its bytes, a path
/// that leaves it is 403, and anything else is 404.
pub fn serve(pages: &Path, path: &str) -> Served {
    if path == CALL_JS_PATH {
        return Served {
            status: 200,
            content_type: content_type("call.js"),
            body: CALL_JS.as_bytes().to_vec(),
        };
    }
    let Some(relative) = path.strip_prefix('/') else {
        return status(404);
    };
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let file = match resolve(pages, relative) {
        Ok(file) => file,
        Err(code) => return status(code),
    };
    match std::fs::read(&file) {
        Ok(body) => Served {
            status: 200,
            content_type: content_type(relative),
            body,
        },
        Err(_) => status(500),
    }
}

/// The address of `file` of `pages` for the control line `load`, where the scheme serves that
/// file; `None` where it does not, so that `load` answers `err no_file`.
pub fn page_to_load(pages: &Path, file: &str) -> Option<String> {
    resolve(pages, file).ok().map(|_| page_url(file))
}

fn resolve(pages: &Path, relative: &str) -> Result<PathBuf, u16> {
    if !relative.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    }) {
        return Err(404);
    }
    let (Ok(root), Ok(file)) = (pages.canonicalize(), pages.join(relative).canonicalize()) else {
        return Err(404);
    };
    if !file.starts_with(&root) {
        return Err(403);
    }
    if !file.is_file() {
        return Err(404);
    }
    Ok(file)
}

/// The response of the scheme to a request for `path`: `serve` with its headers, among them
/// `policy()` as the Content-Security-Policy, whatever the status.
pub fn respond(pages: &Path, path: &str) -> Response<Vec<u8>> {
    let served = serve(pages, path);
    Response::builder()
        .status(served.status)
        .header(CONTENT_TYPE, served.content_type)
        .header("Content-Security-Policy", policy())
        .header("X-Content-Type-Options", "nosniff")
        .header(CACHE_CONTROL, "no-store")
        .body(served.body)
        .unwrap_or_default()
}

fn status(code: u16) -> Served {
    Served {
        status: code,
        content_type: "text/plain; charset=utf-8",
        body: Vec::new(),
    }
}

fn content_type(file: &str) -> &'static str {
    match file.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The address of `file` of the pages directory under the app's scheme, in the form of the
/// web view of this system.
pub fn page_url(file: &str) -> String {
    format!("{}/{file}", origin())
}

/// The Content-Security-Policy of every response of the scheme: scripts from the page's own
/// origin, connections to the core's IPC in the form of this system, and nothing else, so that
/// a page of the scheme reaches no other origin. The core's own scripts, channel messages and
/// events come by the web view's script evaluation, which the policy does not govern.
pub fn policy() -> String {
    let ipc = if cfg!(windows) {
        "http://ipc.localhost"
    } else {
        "ipc://localhost"
    };
    format!(
        "default-src 'none'; script-src 'self' {}; connect-src {ipc}; base-uri 'none'; \
         form-action 'none'; frame-ancestors 'none'",
        origin()
    )
}

/// Whether the window may navigate to `url`: an address under the app's own scheme alone.
pub fn may_navigate(url: &str) -> bool {
    url.starts_with(&format!("{}/", origin()))
}

fn origin() -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost")
    } else {
        format!("{SCHEME}://localhost")
    }
}
