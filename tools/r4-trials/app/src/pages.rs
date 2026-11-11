use std::path::{Path, PathBuf};

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
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost/{file}")
    } else {
        format!("{SCHEME}://localhost/{file}")
    }
}
