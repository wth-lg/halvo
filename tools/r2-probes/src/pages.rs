use crate::text::fold;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TEXT_PAGES: [&str; 3] = ["text-1", "text-2", "text-3"];
pub const LIST_PAGE: &str = "list-1";
pub const PAGES: [(&str, &str); 7] = [
    ("text-1", include_str!("../pages/text-1.html")),
    ("text-2", include_str!("../pages/text-2.html")),
    ("text-3", include_str!("../pages/text-3.html")),
    ("list-1", include_str!("../pages/list-1.html")),
    ("input", include_str!("../pages/input.html")),
    ("page-one", include_str!("../pages/page-one.html")),
    ("page-two", include_str!("../pages/page-two.html")),
];
pub const DOCUMENTS: [(&str, &str, &str); 2] = [
    (
        "writer",
        "letter.fodt",
        include_str!("../documents/letter.fodt"),
    ),
    (
        "calc",
        "sheet.fods",
        include_str!("../documents/sheet.fods"),
    ),
];
pub const OFFICE_PROFILE: &str = include_str!("../documents/registrymodifications.xcu");

pub fn html(name: &str) -> Option<&'static str> {
    PAGES
        .iter()
        .find(|(page, _)| *page == name)
        .map(|(_, html)| *html)
}

pub fn document(target: &str) -> Option<(&'static str, &'static str)> {
    DOCUMENTS
        .iter()
        .find(|(name, _, _)| *name == target)
        .map(|(_, file, content)| (*file, *content))
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)?;
    let after = start + text[start..].find('>')? + 1;
    let end = after + text[after..].find(close)?;
    Some(&text[after..end])
}

fn without_block(text: &str, tag: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    while let Some(start) = rest.find(&open) {
        out.push_str(&rest[..start]);
        match rest[start..].find(&close) {
            Some(end) => rest = &rest[start + end + close.len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn strip_tags(markup: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in markup.chars() {
        match c {
            '<' => {
                inside = true;
                out.push(' ');
            }
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    fold(
        &out.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&nbsp;", " "),
    )
}

pub fn title(name: &str) -> String {
    html(name)
        .and_then(|html| between(html, "<title", "</title>"))
        .map(fold)
        .unwrap_or_default()
}

pub fn known_text(target: &str) -> Option<String> {
    if let Some(html) = html(target) {
        let body = between(html, "<body", "</body>")?;
        return Some(strip_tags(&without_block(
            &without_block(body, "script"),
            "style",
        )));
    }
    let (_, content) = document(target)?;
    Some(strip_tags(between(
        content,
        "<office:body",
        "</office:body>",
    )?))
}

pub struct Server {
    pub port: u16,
    hits: Arc<Mutex<Vec<String>>>,
}

fn answer(mut stream: TcpStream, hits: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
    let path = target.split('?').next().unwrap_or("");
    let name = path.trim_start_matches('/').trim_end_matches(".html");
    let (status, body) = match html(name) {
        Some(html) => ("200 OK", html),
        None => ("404 Not Found", ""),
    };
    if let Ok(mut hits) = hits.lock() {
        hits.push(target);
    }
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

impl Server {
    pub fn start() -> Result<Server, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("bind: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let hits = Arc::clone(&shared);
                std::thread::spawn(move || answer(stream, &hits));
            }
        });
        Ok(Server { port, hits })
    }

    pub fn url(&self, page: &str, token: &str) -> String {
        format!("http://127.0.0.1:{}/{page}.html?r={token}", self.port)
    }

    pub fn requested(&self, token: &str) -> bool {
        let wanted = format!("r={token}");
        self.hits
            .lock()
            .map(|hits| hits.iter().any(|hit| hit.ends_with(&wanted)))
            .unwrap_or(false)
    }

    pub fn wait_for(&self, token: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.requested(token) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        self.requested(token)
    }
}
