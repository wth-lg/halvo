/// The origins whose pages `subscribe` and `manage` open or print. A debug build has the
/// loopback origin `http://127.0.0.1` with any port, where the provider fake, its stand-in
/// page and the stub serve their pages. A release build has the provider's hosted origins,
/// which it takes at compile time from `HALVO_ACCOUNT_PAGE_ORIGINS`: one or more origins of
/// the form that `is_hosted_origin` admits, separated by one space.
pub enum Pages {
    Loopback,
    Hosted(&'static str),
}

/// The list of this build.
#[cfg(debug_assertions)]
pub const PAGES: Pages = Pages::Loopback;

/// The list of this build.
#[cfg(not(debug_assertions))]
pub const PAGES: Pages = Pages::Hosted(HOSTED_PAGES);

#[cfg(not(debug_assertions))]
const HOSTED_PAGES: &str = env!(
    "HALVO_ACCOUNT_PAGE_ORIGINS",
    "a release build of halvo-account takes the provider's page origins from HALVO_ACCOUNT_PAGE_ORIGINS at compile time"
);

#[cfg(not(debug_assertions))]
const _: () = assert!(
    is_hosted_list(HOSTED_PAGES),
    "HALVO_ACCOUNT_PAGE_ORIGINS holds https origins with no path, separated by one space"
);

/// A URL of a page whose origin is on the list: every byte is printable ASCII other than
/// the space, and the origin is followed by the end, `/`, `?` or `#`.
#[derive(Debug, PartialEq, Eq)]
pub struct PageUrl(String);

impl PageUrl {
    /// The URL as the backend wrote it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Takes `url` when its origin is on `pages`, written exactly as the list writes it.
pub fn page_url(url: &str, pages: &Pages) -> Option<PageUrl> {
    if url.is_empty() || !url.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        return None;
    }
    let fits = match pages {
        Pages::Loopback => url
            .strip_prefix("http://127.0.0.1")
            .and_then(after_port)
            .is_some_and(ends_origin),
        Pages::Hosted(list) => list
            .split(' ')
            .any(|origin| url.strip_prefix(origin).is_some_and(ends_origin)),
    };
    fits.then(|| PageUrl(url.to_string()))
}

fn after_port(rest: &str) -> Option<&str> {
    let Some(after) = rest.strip_prefix(':') else {
        return Some(rest);
    };
    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
    is_port(&after.as_bytes()[..digits], 0, digits).then(|| &after[digits..])
}

fn ends_origin(rest: &str) -> bool {
    rest.is_empty() || rest.starts_with(['/', '?', '#'])
}

/// True for `https://`, a host of `a` to `z`, `0` to `9`, `.` and `-`, and at most a port
/// from 1 to 65535 written with no leading zero: the form of the backend's origin and of
/// each page origin of a release build.
pub const fn is_hosted_origin(origin: &str) -> bool {
    let bytes = origin.as_bytes();
    is_hosted_span(bytes, 0, bytes.len())
}

/// True for one or more origins that `is_hosted_origin` admits, separated by one space: the
/// form of `HALVO_ACCOUNT_PAGE_ORIGINS` of a release build.
pub const fn is_hosted_list(list: &str) -> bool {
    let bytes = list.as_bytes();
    let mut start = 0;
    let mut at = 0;
    while at <= bytes.len() {
        if at == bytes.len() || bytes[at] == b' ' {
            if !is_hosted_span(bytes, start, at) {
                return false;
            }
            start = at + 1;
        }
        at += 1;
    }
    true
}

const fn is_hosted_span(bytes: &[u8], start: usize, end: usize) -> bool {
    let scheme = b"https://";
    if end < start + scheme.len() {
        return false;
    }
    let mut at = 0;
    while at < scheme.len() {
        if bytes[start + at] != scheme[at] {
            return false;
        }
        at += 1;
    }
    let host = start + scheme.len();
    let mut at = host;
    while at < end && bytes[at] != b':' {
        let byte = bytes[at];
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-') {
            return false;
        }
        at += 1;
    }
    if at == host {
        return false;
    }
    at == end || is_port(bytes, at + 1, end)
}

const fn is_port(bytes: &[u8], start: usize, end: usize) -> bool {
    if end <= start || end - start > 5 || bytes[start] == b'0' {
        return false;
    }
    let mut value: u32 = 0;
    let mut at = start;
    while at < end {
        if !bytes[at].is_ascii_digit() {
            return false;
        }
        value = value * 10 + (bytes[at] - b'0') as u32;
        at += 1;
    }
    value <= 65535
}

/// The origin of the backend: a scheme, a host and a port, with no path.
pub struct Backend(String);

#[cfg(not(debug_assertions))]
const COMPILED_BACKEND: &str = env!(
    "HALVO_ACCOUNT_BACKEND",
    "a release build of halvo-account takes the backend's origin from HALVO_ACCOUNT_BACKEND at compile time"
);

#[cfg(not(debug_assertions))]
const _: () = assert!(
    is_hosted_origin(COMPILED_BACKEND),
    "HALVO_ACCOUNT_BACKEND of a release build is an https origin with no path"
);

impl Backend {
    /// The backend of a debug build, from the value of `HALVO_ACCOUNT_BACKEND`: `http://` or
    /// `https://`, an IPv4 address in dotted decimal with no leading zero, and at most a port
    /// from 1 to 65535 with no leading zero. A name, `localhost` among them, is refused.
    pub fn from_address(text: &str) -> Option<Backend> {
        let rest = text
            .strip_prefix("http://")
            .or_else(|| text.strip_prefix("https://"))?;
        let (host, port) = match rest.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (rest, None),
        };
        let port_fits = port.is_none_or(|port| is_port(port.as_bytes(), 0, port.len()));
        (is_dotted_quad(host) && port_fits).then(|| Backend(text.to_string()))
    }

    /// The backend of a release build: the origin that it was given at compile time.
    #[cfg(not(debug_assertions))]
    pub fn compiled() -> Backend {
        Backend(COMPILED_BACKEND.to_string())
    }

    /// The URL of `path` at this origin.
    pub fn url_of(&self, path: &str) -> String {
        format!("{}{path}", self.0)
    }
}

fn is_dotted_quad(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            let bytes = part.as_bytes();
            !bytes.is_empty()
                && bytes.len() <= 3
                && bytes.iter().all(u8::is_ascii_digit)
                && (bytes.len() == 1 || bytes[0] != b'0')
                && part.parse::<u16>().is_ok_and(|value| value <= 255)
        })
}
