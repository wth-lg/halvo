use super::text::{is_port, number_form, WebOrigin};
use super::Refusal;

fn refused(rule: &'static str) -> Refusal {
    Refusal::malformed("WebOrigin", rule)
}

fn lower(c: char) -> Option<char> {
    let shift = |by: u32| char::from_u32(u32::from(c) + by);
    match c {
        'a'..='z' | '0'..='9' | '-' => Some(c),
        'A'..='Z' => Some(c.to_ascii_lowercase()),
        '\u{e0}'..='\u{f6}' | '\u{f8}'..='\u{ff}' => Some(c),
        '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{de}' => shift(0x20),
        '\u{3b1}'..='\u{3c1}' | '\u{3c3}'..='\u{3c9}' => Some(c),
        '\u{391}'..='\u{3a1}' | '\u{3a3}'..='\u{3a9}' => shift(0x20),
        '\u{430}'..='\u{45f}' => Some(c),
        '\u{410}'..='\u{42f}' => shift(0x20),
        '\u{400}'..='\u{40f}' => shift(0x50),
        '\u{3041}'..='\u{3096}' | '\u{30a1}'..='\u{30fa}' | '\u{30fc}' => Some(c),
        '\u{4e00}'..='\u{9fff}' | '\u{ac00}'..='\u{d7a3}' => Some(c),
        _ => None,
    }
}

const BASE: u32 = 36;
const T_MIN: u32 = 1;
const T_MAX: u32 = 26;

fn digit(d: u32) -> char {
    match d {
        0..=25 => char::from(b'a' + d as u8),
        _ => char::from(b'0' + (d - 26) as u8),
    }
}

fn adapt(delta: u32, points: u32, first: bool) -> u32 {
    let mut delta = if first { delta / 700 } else { delta / 2 };
    delta += delta / points;
    let mut k = 0;
    while delta > ((BASE - T_MIN) * T_MAX) / 2 {
        delta /= BASE - T_MIN;
        k += BASE;
    }
    k + ((BASE - T_MIN + 1) * delta) / (delta + 38)
}

fn encode(label: &[char]) -> Option<String> {
    let mut out: String = label.iter().filter(|c| c.is_ascii()).collect();
    let basic = u32::try_from(out.len()).ok()?;
    let total = u32::try_from(label.len()).ok()?;
    if basic > 0 {
        out.push('-');
    }
    let mut n: u32 = 0x80;
    let mut delta: u32 = 0;
    let mut bias: u32 = 72;
    let mut done = basic;
    while done < total {
        let next = label
            .iter()
            .map(|c| u32::from(*c))
            .filter(|c| *c >= n)
            .min()?;
        delta = delta.checked_add((next - n).checked_mul(done + 1)?)?;
        n = next;
        for c in label.iter().map(|c| u32::from(*c)) {
            if c < n {
                delta = delta.checked_add(1)?;
            } else if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = k.saturating_sub(bias).clamp(T_MIN, T_MAX);
                    if q < t {
                        break;
                    }
                    out.push(digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(digit(q));
                bias = adapt(delta, done + 1, done == basic);
                delta = 0;
                done += 1;
            }
        }
        delta = delta.checked_add(1)?;
        n = n.checked_add(1)?;
    }
    Some(out)
}

fn a_label(label: &str) -> Result<String, Refusal> {
    let lowered: Vec<char> = label
        .chars()
        .map(lower)
        .collect::<Option<_>>()
        .ok_or_else(|| refused("address_host"))?;
    if lowered.is_empty() {
        return Err(refused("address_host"));
    }
    if lowered.iter().all(char::is_ascii) {
        return Ok(lowered.into_iter().collect());
    }
    if lowered.starts_with(&['x', 'n', '-', '-']) || lowered.len() > 59 {
        return Err(refused("address_host"));
    }
    encode(&lowered)
        .map(|encoded| format!("xn--{encoded}"))
        .ok_or_else(|| refused("address_host"))
}

impl WebOrigin {
    /// The origin of an address that a browser reports, `<scheme>://<host>[:<port>]` and then, after a `/`, `?`, `#` or
    /// `\`, any text but an ASCII space or control character: the scheme `http` or `https` in any case; the default
    /// port, 80 or 443, added where none is written; the host in lower case, with one dot at its end dropped and each
    /// internationalized label turned into its A-label (RFC 3492). The origin is the form of `WebOrigin::parse`.
    /// Refused, with the cause `malformed` and no text of the address: an ASCII space or control character (U+0000 to
    /// U+0020, U+007F) anywhere, another scheme or no `://`, userinfo, an address in brackets, a port that is not 1 to
    /// 65,535 with no leading zero, an empty label, a label or a host too long for the form, a character outside the
    /// form that is not one of the letters below, a non-ASCII label that starts with `xn--`, and a host whose last
    /// label is a number (all digits, or `0x` and hex digits) unless it is four decimal octets with no leading zero,
    /// the form in which a browser writes an IPv4 address. The non-ASCII letters taken, each one for which a browser's
    /// mapping of a host is lower case alone, are U+00C0 to U+00FF but `×`, `÷` and `ß`; U+0391 to U+03A9 and U+03B1 to
    /// U+03C9 but U+03A2 and `ς`; U+0400 to U+045F; U+3041 to U+3096; U+30A1 to U+30FA and `ー`; U+4E00 to U+9FFF; and
    /// U+AC00 to U+D7A3. Any other non-ASCII character is refused. An ASCII label that starts with `xn--` is taken as
    /// it stands, also where it is no valid A-label.
    pub fn of_address(address: &str) -> Result<WebOrigin, Refusal> {
        if address.chars().any(|c| c <= ' ' || c == '\u{7f}') {
            return Err(refused("address_control"));
        }
        let (scheme, rest) = address
            .split_once("://")
            .ok_or_else(|| refused("address_scheme"))?;
        let scheme = scheme.to_ascii_lowercase();
        let default_port = match scheme.as_str() {
            "http" => "80",
            "https" => "443",
            _ => return Err(refused("address_scheme")),
        };
        let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or_default();
        if authority.contains('@') {
            return Err(refused("address_userinfo"));
        }
        if authority.contains(['[', ']']) {
            return Err(refused("address_ip_literal"));
        }
        let (host, port) = match authority.split_once(':') {
            None => (authority, default_port),
            Some((host, port)) if is_port(port) => (host, port),
            Some(_) => return Err(refused("address_port")),
        };
        let host = host.strip_suffix('.').unwrap_or(host);
        let labels = host
            .split('.')
            .map(a_label)
            .collect::<Result<Vec<String>, Refusal>>()?;
        let host = labels.join(".");
        if number_form(&host) {
            return Err(refused("address_ipv4"));
        }
        WebOrigin::parse(&format!("{scheme}://{host}:{port}")).map_err(|_| refused("address_host"))
    }
}
