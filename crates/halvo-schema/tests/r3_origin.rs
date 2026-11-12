use std::path::Path;

use halvo_schema::executable::text::WebOrigin;
use halvo_schema::executable::RefusalCause;

const DRAWS: usize = 100_000;

const TABLE_MADE_BY_NODE: &str = "origin-oracle.tsv";

fn lowered(c: char) -> Option<char> {
    let table: [(u32, u32, u32); 20] = [
        (0x2D, 0x2D, 0),
        (0x30, 0x39, 0),
        (0x61, 0x7A, 0),
        (0x41, 0x5A, 0x20),
        (0xE0, 0xF6, 0),
        (0xF8, 0xFF, 0),
        (0xC0, 0xD6, 0x20),
        (0xD8, 0xDE, 0x20),
        (0x3B1, 0x3C1, 0),
        (0x3C3, 0x3C9, 0),
        (0x391, 0x3A1, 0x20),
        (0x3A3, 0x3A9, 0x20),
        (0x430, 0x45F, 0),
        (0x410, 0x42F, 0x20),
        (0x400, 0x40F, 0x50),
        (0x3041, 0x3096, 0),
        (0x30A1, 0x30FA, 0),
        (0x30FC, 0x30FC, 0),
        (0x4E00, 0x9FFF, 0),
        (0xAC00, 0xD7A3, 0),
    ];
    let u = u32::from(c);
    table
        .iter()
        .find(|(first, last, _)| *first <= u && u <= *last)
        .and_then(|(_, _, add)| char::from_u32(u + add))
}

fn punycode(input: &[char]) -> Option<String> {
    let digit = |d: u64| -> char {
        if d < 26 {
            char::from(b'a' + d as u8)
        } else {
            char::from(b'0' + (d - 26) as u8)
        }
    };
    let adapt = |delta: u64, points: u64, first: bool| -> u64 {
        let mut delta = if first { delta / 700 } else { delta / 2 };
        delta += delta / points;
        let mut k = 0;
        while delta > (35 * 26) / 2 {
            delta /= 35;
            k += 36;
        }
        k + (36 * delta) / (delta + 38)
    };
    let mut output: String = input.iter().filter(|c| c.is_ascii()).collect();
    let basic = output.len() as u64;
    let mut handled = basic;
    if basic > 0 {
        output.push('-');
    }
    let (mut n, mut delta, mut bias) = (128_u64, 0_u64, 72_u64);
    while handled < input.len() as u64 {
        let m = input
            .iter()
            .map(|c| u64::from(*c))
            .filter(|c| *c >= n)
            .min()?;
        delta = delta.checked_add((m - n).checked_mul(handled + 1)?)?;
        n = m;
        for c in input {
            let c = u64::from(*c);
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = 36;
                loop {
                    let t = if k <= bias {
                        1
                    } else if k >= bias + 26 {
                        26
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    output.push(digit(t + (q - t) % (36 - t)));
                    q = (q - t) / (36 - t);
                    k += 36;
                }
                output.push(digit(q));
                bias = adapt(delta, handled + 1, handled == basic);
                delta = 0;
                handled += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(output)
}

fn octet(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 3
        && label.chars().all(|c| c.is_ascii_digit())
        && (label == "0" || !label.starts_with('0'))
        && label.parse::<u32>().is_ok_and(|v| v <= 255)
}

fn read_origin(address: &str) -> Option<String> {
    let chars: Vec<char> = address.chars().collect();
    if chars
        .iter()
        .any(|c| u32::from(*c) <= 0x20 || *c == '\u{7f}')
    {
        return None;
    }
    let mut i = 0;
    let mut scheme = String::new();
    while i < chars.len() && chars[i] != ':' {
        scheme.push(chars[i].to_ascii_lowercase());
        i += 1;
    }
    let default_port = match scheme.as_str() {
        "http" => "80",
        "https" => "443",
        _ => return None,
    };
    if chars.get(i..i + 3) != Some(&[':', '/', '/'][..]) {
        return None;
    }
    i += 3;
    let mut authority: Vec<char> = Vec::new();
    while i < chars.len() && !matches!(chars[i], '/' | '?' | '#' | '\\') {
        authority.push(chars[i]);
        i += 1;
    }
    if authority.iter().any(|c| matches!(c, '@' | '[' | ']')) {
        return None;
    }
    let colon: Vec<usize> = (0..authority.len())
        .filter(|at| authority[*at] == ':')
        .collect();
    let (host, port): (Vec<char>, String) = match colon.as_slice() {
        [] => (authority.clone(), default_port.to_owned()),
        [at] => {
            let port: String = authority[at + 1..].iter().collect();
            let fits = !port.is_empty()
                && port.len() <= 5
                && port.chars().all(|c| c.is_ascii_digit())
                && !port.starts_with('0')
                && port.parse::<u32>().is_ok_and(|v| (1..=65_535).contains(&v));
            if !fits {
                return None;
            }
            (authority[..*at].to_vec(), port)
        }
        _ => return None,
    };
    let mut labels: Vec<Vec<char>> = vec![Vec::new()];
    for c in host {
        if c == '.' {
            labels.push(Vec::new());
        } else {
            labels.last_mut().unwrap().push(c);
        }
    }
    if labels.len() > 1 && labels.last().unwrap().is_empty() {
        labels.pop();
    }
    let mut written: Vec<String> = Vec::new();
    for label in labels {
        if label.is_empty() {
            return None;
        }
        let low: Vec<char> = label
            .iter()
            .map(|c| lowered(*c))
            .collect::<Option<Vec<char>>>()?;
        let text = if low.iter().all(|c| c.is_ascii()) {
            low.iter().collect::<String>()
        } else {
            if low.starts_with(&['x', 'n', '-', '-']) {
                return None;
            }
            format!("xn--{}", punycode(&low)?)
        };
        if text.is_empty() || text.len() > 63 || text.starts_with('-') || text.ends_with('-') {
            return None;
        }
        written.push(text);
    }
    let host = written.join(".");
    if host.len() > 253 {
        return None;
    }
    let last = written.last().unwrap();
    let numeric = last.chars().all(|c| c.is_ascii_digit())
        || (last.starts_with("0x") && last[2..].chars().all(|c| c.is_ascii_hexdigit()));
    if numeric && !(written.len() == 4 && written.iter().all(|l| octet(l))) {
        return None;
    }
    Some(format!("{scheme}://{host}:{port}"))
}

fn origin_of(address: &str) -> Option<String> {
    match WebOrigin::of_address(address) {
        Ok(origin) => {
            assert_eq!(
                WebOrigin::parse(origin.as_str()).as_ref(),
                Ok(&origin),
                "{address:?}"
            );
            Some(origin.as_str().to_owned())
        }
        Err(refusal) => {
            assert_eq!(
                (refusal.cause(), refusal.type_name()),
                (RefusalCause::Malformed, "WebOrigin"),
                "{address:?}"
            );
            None
        }
    }
}

const KNOWN: &[(&str, Option<&str>)] = &[
    (
        "http://utility.test:18090/pay?x=1",
        Some("http://utility.test:18090"),
    ),
    (
        "http://utility.test:18090",
        Some("http://utility.test:18090"),
    ),
    ("HTTPS://Pay.Test/", Some("https://pay.test:443")),
    ("https://pay.test", Some("https://pay.test:443")),
    ("http://pay.test", Some("http://pay.test:80")),
    ("http://pay.test:80/", Some("http://pay.test:80")),
    ("https://pay.test:443/", Some("https://pay.test:443")),
    ("http://pay.test:443/", Some("http://pay.test:443")),
    ("https://pay.test:80/", Some("https://pay.test:80")),
    ("http://pay.test./", Some("http://pay.test:80")),
    ("http://pay.test.:8080/", Some("http://pay.test:8080")),
    ("http://pay.test../", None),
    ("http://pay..test/", None),
    ("http://./", None),
    ("http:///", None),
    ("http://", None),
    ("http://:80/", None),
    ("http://pay.test:0/", None),
    ("http://pay.test:65535/", Some("http://pay.test:65535")),
    ("http://pay.test:65536/", None),
    ("http://pay.test:080/", None),
    ("http://pay.test:/", None),
    ("http://pay.test:+80/", None),
    ("http://pay.test:80:90/", None),
    ("http://user@pay.test/", None),
    ("http://user:secret@pay.test/", None),
    ("http://pay.test@evil.test/", None),
    ("http://pay.test:80@evil.test/", None),
    ("http://pay.test\\@evil.test/", Some("http://pay.test:80")),
    ("http://pay.test/@evil.test", Some("http://pay.test:80")),
    ("http://pay.test?@evil.test", Some("http://pay.test:80")),
    ("http://pay.test#@evil.test", Some("http://pay.test:80")),
    ("http://[::1]:8080/", None),
    ("http://[127.0.0.1]/", None),
    ("http://127.0.0.1:8080/x", Some("http://127.0.0.1:8080")),
    ("http://127.0.0.1./", Some("http://127.0.0.1:80")),
    ("http://0.0.0.0/", Some("http://0.0.0.0:80")),
    ("http://255.255.255.255/", Some("http://255.255.255.255:80")),
    ("http://256.0.0.1/", None),
    ("http://01.0.0.1/", None),
    ("http://0x7f.0.0.1/", None),
    ("http://0x7f.1/", None),
    ("http://127.1/", None),
    ("http://1.2.3/", None),
    ("http://1.2.3.4.5/", None),
    ("http://2130706433/", None),
    ("http://pay.4294967296/", None),
    ("http://pay.99999999999999999999/", None),
    ("http://pay.999999999999999999999999999999999999999/", None),
    ("http://pay.9999999999999999999999999999999999999999/", None),
    (
        "http://pay.999999999999999999999999999999999999999999999999999999999999999/",
        None,
    ),
    ("http://pay.123/", None),
    ("http://pay.0x/", None),
    ("http://pay.0x1F/", None),
    ("http://pay.0x1g/", Some("http://pay.0x1g:80")),
    ("http://1.2.3.pay/", Some("http://1.2.3.pay:80")),
    (
        "http://b\u{fc}cher.test/",
        Some("http://xn--bcher-kva.test:80"),
    ),
    (
        "http://B\u{dc}CHER.test/",
        Some("http://xn--bcher-kva.test:80"),
    ),
    (
        "http://xn--bcher-kva.test/",
        Some("http://xn--bcher-kva.test:80"),
    ),
    (
        "http://XN--BCHER-KVA.test/",
        Some("http://xn--bcher-kva.test:80"),
    ),
    ("http://xn--b\u{fc}cher.test/", None),
    ("http://fa\u{df}.test/", None),
    ("http://\u{212a}.test/", None),
    ("http://\u{17f}.test/", None),
    ("http://\u{130}stanbul.test/", None),
    ("http://e\u{301}.test/", None),
    ("http://\u{3b5}\u{3bb}\u{3bb}\u{3b1}\u{3c2}.test/", None),
    ("http://pay\u{3002}test/", None),
    ("http://pay\u{ff0e}test/", None),
    ("http://pay\u{ff0f}evil.test/", None),
    ("http://%70ay.test/", None),
    ("http://pay_now.test/", None),
    ("http://-pay.test/", None),
    ("http://pay-.test/", None),
    ("http://pa--y.test/", Some("http://pa--y.test:80")),
    (" http://pay.test/", None),
    ("http://pay.test/ ", None),
    ("http://pay.test/a b", None),
    ("http://pa y.test/", None),
    ("http://pay.te\tst/", None),
    ("http://pay.te\nst/", None),
    ("http://pay.test\u{0}/", None),
    ("http://pay.test/\u{7f}", None),
    ("http://pay.test/\u{fc}ber", Some("http://pay.test:80")),
    ("https://app.test/#!/home", Some("https://app.test:443")),
    ("https://app.test/!", Some("https://app.test:443")),
    ("https://app.test?!", Some("https://app.test:443")),
    ("https://app.test/\u{85}", Some("https://app.test:443")),
    ("https://app.test/\u{a0}x", Some("https://app.test:443")),
    ("https://app.test#\u{3000}", Some("https://app.test:443")),
    ("http:pay.test", None),
    ("http:/pay.test", None),
    ("http:////pay.test", None),
    ("http:\\\\pay.test", None),
    ("ftp://pay.test/", None),
    ("ws://pay.test/", None),
    ("file:///etc/hosts", None),
    ("javascript://pay.test/", None),
    ("about:blank", None),
    ("", None),
    ("pay.test", None),
    ("//pay.test/", None),
];

const RFC_3492: &[(&str, &str)] = &[
    (
        "\u{4ed6}\u{4eec}\u{4e3a}\u{4ec0}\u{4e48}\u{4e0d}\u{8bf4}\u{4e2d}\u{6587}",
        "ihqwcrb4cv8a8dqg056pqjye",
    ),
    (
        "\u{4ed6}\u{5011}\u{7232}\u{4ec0}\u{9ebd}\u{4e0d}\u{8aaa}\u{4e2d}\u{6587}",
        "ihqwctvzc91f659drss3x8bo0yb",
    ),
    (
        "\u{306a}\u{305c}\u{307f}\u{3093}\u{306a}\u{65e5}\u{672c}\u{8a9e}\u{3092}\u{8a71}\u{3057}\u{3066}\u{304f}\u{308c}\u{306a}\u{3044}\u{306e}\u{304b}",
        "n8jok5ay5dzabd5bym9f0cm5685rrjetr6pdxa",
    ),
    (
        "\u{43f}\u{43e}\u{447}\u{435}\u{43c}\u{443}\u{436}\u{435}\u{43e}\u{43d}\u{438}\u{43d}\u{435}\u{433}\u{43e}\u{432}\u{43e}\u{440}\u{44f}\u{442}\u{43f}\u{43e}\u{440}\u{443}\u{441}\u{441}\u{43a}\u{438}",
        "b1abfaaepdrnnbgefbadotcwatmq2g4l",
    ),
    (
        "Porqu\u{e9}nopuedensimplementehablarenEspa\u{f1}ol",
        "porqunopuedensimplementehablarenespaol-fmd56a",
    ),
    (
        "3\u{5e74}B\u{7d44}\u{91d1}\u{516b}\u{5148}\u{751f}",
        "3b-ww4c5e180e575a65lsy2b",
    ),
    (
        "\u{5b89}\u{5ba4}\u{5948}\u{7f8e}\u{6075}-with-SUPER-MONKEYS",
        "-with-super-monkeys-pc58ag80a8qai00g7n9n",
    ),
    (
        "Hello-Another-Way-\u{305d}\u{308c}\u{305e}\u{308c}\u{306e}\u{5834}\u{6240}",
        "hello-another-way--fc4qua05auwb3674vfr0b",
    ),
    (
        "\u{3072}\u{3068}\u{3064}\u{5c4b}\u{6839}\u{306e}\u{4e0b}2",
        "2-u9tlzr9756bt3uc0v",
    ),
    (
        "Maji\u{3067}Koi\u{3059}\u{308b}5\u{79d2}\u{524d}",
        "majikoi5-783gue6qz075azm5e",
    ),
    (
        "\u{30d1}\u{30d5}\u{30a3}\u{30fc}de\u{30eb}\u{30f3}\u{30d0}",
        "de-jg4avhby1noc0d",
    ),
    (
        "\u{305d}\u{306e}\u{30b9}\u{30d4}\u{30fc}\u{30c9}\u{3067}",
        "d9juau41awczczp",
    ),
];

#[test]
fn the_origin_of_a_hand_picked_address_is_the_one_that_the_contract_gives() {
    for (address, want) in KNOWN {
        let want = want.map(str::to_owned);
        assert_eq!(read_origin(address), want, "reader: {address:?}");
        assert_eq!(origin_of(address), want, "function: {address:?}");
    }
}

#[test]
fn an_internationalized_name_becomes_the_a_label_of_rfc_3492() {
    for (name, encoded) in RFC_3492 {
        let low: Vec<char> = name.chars().map(|c| lowered(c).unwrap()).collect();
        assert_eq!(punycode(&low).as_deref(), Some(*encoded), "reader: {name}");
        let want = format!("http://xn--{encoded}.test:80");
        assert_eq!(
            origin_of(&format!("http://{name}.test/")),
            Some(want.clone()),
            "function: {name}"
        );
        assert_eq!(read_origin(&format!("http://{name}.test/")), Some(want));
    }
    let korean = "\u{c138}\u{acc4}\u{c758}\u{baa8}\u{b4e0}\u{c0ac}\u{b78c}\u{b4e4}\u{c774}\u{d55c}\u{ad6d}\u{c5b4}\u{b97c}\u{c774}\u{d574}\u{d55c}\u{b2e4}\u{ba74}\u{c5bc}\u{b9c8}\u{b098}\u{c88b}\u{c744}\u{ae4c}";
    let low: Vec<char> = korean.chars().collect();
    assert_eq!(
        punycode(&low).as_deref(),
        Some("989aomsvi5e83db1d2a355cv1e0vak1dwrv93d5xbh15a0dt30a5jpsd879ccm6fea98c")
    );
    let address = format!("http://{korean}.test/");
    assert_eq!(read_origin(&address), None);
    assert_eq!(origin_of(&address), None);
}

struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }

    fn percent(&mut self, p: u64) -> bool {
        self.next() % 100 < p
    }
}

const SCHEMES: &[&str] = &["http", "https", "HTTP", "HTTPS", "Http", "hTtPs"];
const BAD_SCHEMES: &[&str] = &[
    "ftp",
    "ws",
    "file",
    "httpx",
    "",
    "htt",
    "h ttp",
    "\u{ff48}ttp",
    "javascript",
    "http\u{0}",
    "https:",
];
const SEPARATORS: &[&str] = &[":/", ":", ":///", "//", "://\\", ":\\\\", " ://", ":/\\"];
const USERINFO: &[&str] = &["user@", "u:p@", "pay.test@", "@", ":@", "pay.test:18090@"];
const LABELS: &[&str] = &[
    "a",
    "pay",
    "utility",
    "test",
    "example",
    "com",
    "b-c",
    "x1",
    "localhost",
    "PAY",
    "Test",
    "ExAmPle",
    "xn--bcher-kva",
    "XN--BCHER-KVA",
    "123a",
    "a123",
    "0xg",
    "pa--y",
];
const NUMERIC_LABELS: &[&str] = &["1", "22", "255", "0", "0x1f", "0X1F", "007", "0x", "256"];
const UNICODE_LABELS: &[&str] = &[
    "b\u{fc}cher",
    "B\u{dc}CHER",
    "B\u{fc}cher",
    "\u{43f}\u{440}\u{438}\u{43c}\u{435}\u{440}",
    "\u{41f}\u{420}\u{418}\u{41c}\u{415}\u{420}",
    "\u{3b5}\u{3bb}\u{3bb}\u{3b1}\u{3b4}\u{3b1}",
    "\u{395}\u{39b}\u{39b}\u{391}\u{394}\u{391}",
    "\u{4f8b}\u{3048}",
    "\u{30c6}\u{30b9}\u{30c8}",
    "\u{d55c}\u{ad6d}",
    "\u{65e5}\u{672c}\u{8a9e}",
    "caf\u{e9}",
    "CAF\u{c9}",
    "\u{f8}l",
    "\u{430}pple",
];
const HARD_LABELS: &[&str] = &[
    "fa\u{df}",
    "\u{3bb}\u{3cc}\u{3b3}\u{3bf}\u{3c2}",
    "\u{212a}elvin",
    "\u{17f}",
    "\u{130}stanbul",
    "e\u{301}",
    "a\u{3002}b",
    "a\u{ff0e}b",
    "xn--\u{fc}",
    "a_b",
    "-a",
    "a-",
    "",
    "%61",
    "a b",
    "a\tb",
    "a\u{0}b",
    "*",
    "a!b",
    "\u{fffd}",
    "\u{200b}",
    "\u{ff41}",
];
const IPV4_PARTS: &[&str] = &[
    "0",
    "1",
    "8",
    "10",
    "127",
    "192",
    "255",
    "256",
    "00",
    "01",
    "08",
    "0x7f",
    "0X7F",
    "999",
    "4294967295",
    "a",
];
const BRACKETS: &[&str] = &["[::1]", "[2001:db8::1]", "[127.0.0.1]", "[", "]"];
const PORTS: &[&str] = &[
    ":80",
    ":443",
    ":8080",
    ":18090",
    ":1",
    ":65535",
    ":0",
    ":65536",
    ":080",
    ":",
    ":99999",
    ":8a",
    ":+80",
    ":80:90",
    ": 80",
    ":\u{ff10}",
];
const TAILS: &[&str] = &[
    "",
    "/",
    "/path",
    "/a/b?x=1#f",
    "?q",
    "#frag",
    "\\@evil.test",
    "/@evil.test",
    "?@evil.test",
    "#@evil.test",
    "/ x",
    "/\u{fc}ber",
    "/\t",
    "?\u{0}",
    "//evil.test",
    "\\\\evil.test",
    "/%2e%2e",
    ":80/",
];

#[derive(Default)]
struct Seen {
    accepted: usize,
    refused: usize,
    upper_scheme: usize,
    upper_host: usize,
    trailing_dot: usize,
    default_port: usize,
    port_zero: usize,
    port_65536: usize,
    userinfo: usize,
    path_or_query: usize,
    empty_host: usize,
    brackets: usize,
    non_ascii: usize,
    encoded: usize,
    white_space: usize,
    nul: usize,
    ipv4: usize,
    long_label: usize,
}

fn draw_address(d: &mut Draw, seen: &mut Seen) -> String {
    let mut address = String::new();
    if d.percent(2) {
        address.push(' ');
        seen.white_space += 1;
    }
    let scheme = if d.percent(88) {
        d.pick(SCHEMES)
    } else {
        d.pick(BAD_SCHEMES)
    };
    if scheme.chars().any(|c| c.is_ascii_uppercase()) {
        seen.upper_scheme += 1;
    }
    address.push_str(scheme);
    address.push_str(if d.percent(92) {
        "://"
    } else {
        d.pick(SEPARATORS)
    });
    if d.percent(7) {
        address.push_str(d.pick(USERINFO));
        seen.userinfo += 1;
    }
    let mut host = String::new();
    let shape = d.below(100);
    if shape < 4 {
        host.push_str(d.pick(BRACKETS));
        seen.brackets += 1;
    } else if shape < 18 {
        let parts = [3, 4, 4, 4, 4, 5][d.below(6)];
        let chosen: Vec<&str> = (0..parts)
            .map(|_| {
                if d.percent(80) {
                    ["1", "10", "127", "0", "255", "192", "8"][d.below(7)]
                } else {
                    d.pick(IPV4_PARTS)
                }
            })
            .collect();
        host.push_str(&chosen.join("."));
        seen.ipv4 += 1;
    } else if shape < 20 {
        seen.empty_host += 1;
    } else {
        let count = 1 + d.below(4);
        let chosen: Vec<String> = (0..count)
            .map(|_| {
                let roll = d.below(100);
                if roll < 55 {
                    d.pick(LABELS).to_owned()
                } else if roll < 63 {
                    d.pick(NUMERIC_LABELS).to_owned()
                } else if roll < 83 {
                    d.pick(UNICODE_LABELS).to_owned()
                } else if roll < 96 {
                    d.pick(HARD_LABELS).to_owned()
                } else {
                    let width = 58 + d.below(9);
                    if d.percent(50) {
                        "a".repeat(width)
                    } else {
                        "\u{fc}".repeat(width / 4)
                    }
                }
            })
            .collect();
        for label in &chosen {
            if label.chars().any(|c| c.is_uppercase()) {
                seen.upper_host += 1;
            }
            if !label.is_ascii() {
                seen.non_ascii += 1;
            }
            if label.to_ascii_lowercase().starts_with("xn--") {
                seen.encoded += 1;
            }
            if label.len() > 57 {
                seen.long_label += 1;
            }
            if label.is_empty() {
                seen.empty_host += 1;
            }
        }
        host.push_str(&chosen.join("."));
    }
    if d.percent(14) {
        host.push('.');
        seen.trailing_dot += 1;
    } else if d.percent(3) {
        host.push_str("..");
        seen.trailing_dot += 1;
    }
    address.push_str(&host);
    if d.percent(45) {
        let port = d.pick(PORTS);
        match port {
            ":80" | ":443" => seen.default_port += 1,
            ":0" => seen.port_zero += 1,
            ":65536" => seen.port_65536 += 1,
            _ => {}
        }
        address.push_str(port);
    }
    let tail = d.pick(TAILS);
    if !tail.is_empty() {
        seen.path_or_query += 1;
    }
    address.push_str(tail);
    if d.percent(2) {
        address.push(['\n', '\t', ' ', '\u{0}'][d.below(4)]);
    }
    if address.chars().any(|c| c == ' ' || c == '\t' || c == '\n') {
        seen.white_space += 1;
    }
    if address.contains('\u{0}') {
        seen.nul += 1;
    }
    address
}

#[test]
fn the_origin_of_each_of_100000_drawn_addresses_is_the_one_that_the_reader_of_the_grammar_gives() {
    let mut d = Draw(0x5eed_0b16_1a11_2026);
    let mut seen = Seen::default();
    for _ in 0..DRAWS {
        let address = draw_address(&mut d, &mut seen);
        let want = read_origin(&address);
        assert_eq!(origin_of(&address), want, "{address:?}");
        if want.is_some() {
            seen.accepted += 1;
        } else {
            seen.refused += 1;
        }
    }
    let counts = [
        ("accepted", seen.accepted),
        ("refused", seen.refused),
        ("upper_scheme", seen.upper_scheme),
        ("upper_host", seen.upper_host),
        ("trailing_dot", seen.trailing_dot),
        ("default_port", seen.default_port),
        ("port_zero", seen.port_zero),
        ("port_65536", seen.port_65536),
        ("userinfo", seen.userinfo),
        ("path_or_query", seen.path_or_query),
        ("empty_host", seen.empty_host),
        ("brackets", seen.brackets),
        ("non_ascii", seen.non_ascii),
        ("encoded", seen.encoded),
        ("white_space", seen.white_space),
        ("nul", seen.nul),
        ("ipv4", seen.ipv4),
        ("long_label", seen.long_label),
    ];
    println!(
        "draws={DRAWS} {}",
        counts
            .iter()
            .map(|(name, n)| format!("{name}={n}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert_eq!(seen.accepted + seen.refused, DRAWS);
    for (name, n) in counts {
        assert!(n >= 500, "{name}={n}");
    }
}

#[test]
fn a_refused_address_names_the_rule_that_refused_it_and_no_text_of_the_address() {
    let cases: [(&str, &str); 21] = [
        ("http://pay.test/ x", "address_control"),
        ("http://pay.test/\u{7f}", "address_control"),
        ("http://pay\u{0}.test/", "address_control"),
        ("ftp://pay.test/", "address_scheme"),
        ("http:pay.test", "address_scheme"),
        ("http://user@pay.test/", "address_userinfo"),
        ("http://pay.test:80@evil.test/", "address_userinfo"),
        ("http://[::1]:8080/", "address_ip_literal"),
        ("http://[127.0.0.1]/", "address_ip_literal"),
        ("http://pay.test:0/", "address_port"),
        ("http://pay.test:80:90/", "address_port"),
        ("http://pay.test:/", "address_port"),
        ("http://0x7f.0.0.1/", "address_ipv4"),
        ("http://pay.123/", "address_ipv4"),
        ("http://pay..test/", "address_host"),
        ("http://pay.test../", "address_host"),
        ("http://./", "address_host"),
        ("http:///", "address_host"),
        ("http://pay_now.test/", "address_host"),
        ("http://xn--b\u{fc}cher.test/", "address_host"),
        (
            "http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.test/",
            "address_host",
        ),
    ];
    for (address, rule) in cases {
        let refusal = WebOrigin::of_address(address).unwrap_err();
        assert_eq!(
            (refusal.type_name(), refusal.rule(), refusal.key()),
            ("WebOrigin", rule, None),
            "{address:?}"
        );
        let shown = refusal.to_string();
        for word in ["pay", "evil", "now", "::1", "127", "0x7f", "aaaa"] {
            assert!(!shown.contains(word), "{shown}");
        }
    }
}

#[test]
fn a_capital_of_each_taken_range_gives_the_origin_of_its_small_letter_and_each_range_ends_where_the_text_says(
) {
    let pairs = [
        ('A', 'a'),
        ('Z', 'z'),
        ('\u{c0}', '\u{e0}'),
        ('\u{d6}', '\u{f6}'),
        ('\u{d8}', '\u{f8}'),
        ('\u{de}', '\u{fe}'),
        ('\u{391}', '\u{3b1}'),
        ('\u{3a1}', '\u{3c1}'),
        ('\u{3a3}', '\u{3c3}'),
        ('\u{3a9}', '\u{3c9}'),
        ('\u{400}', '\u{450}'),
        ('\u{40f}', '\u{45f}'),
        ('\u{410}', '\u{430}'),
        ('\u{42f}', '\u{44f}'),
    ];
    for (capital, small) in pairs {
        let address = format!("http://x{capital}y.test/");
        let big = origin_of(&address);
        assert!(big.is_some(), "{capital:?}");
        assert_eq!(
            big,
            origin_of(&format!("http://x{small}y.test/")),
            "{capital:?}"
        );
        assert_eq!(big, read_origin(&address), "{capital:?}");
    }
    for taken in [
        '\u{e0}', '\u{ff}', '\u{3b1}', '\u{3c9}', '\u{3041}', '\u{3096}', '\u{30a1}', '\u{30fa}',
        '\u{30fc}', '\u{4e00}', '\u{9fff}', '\u{ac00}', '\u{d7a3}',
    ] {
        let address = format!("http://x{taken}y.test/");
        assert!(origin_of(&address).is_some(), "{taken:?}");
        assert_eq!(origin_of(&address), read_origin(&address), "{taken:?}");
    }
    for refused in [
        '\u{bf}', '\u{d7}', '\u{df}', '\u{f7}', '\u{100}', '\u{390}', '\u{3a2}', '\u{3aa}',
        '\u{3b0}', '\u{3c2}', '\u{3ca}', '\u{3ff}', '\u{460}', '\u{3040}', '\u{3097}', '\u{30a0}',
        '\u{30fb}', '\u{30fd}', '\u{4dff}', '\u{a000}', '\u{abff}', '\u{d7a4}',
    ] {
        let address = format!("http://x{refused}y.test/");
        assert_eq!(origin_of(&address), None, "{refused:?}");
        assert_eq!(read_origin(&address), None, "{refused:?}");
    }
}

const CODEC: &[(&str, &str)] = &[
    ("\u{3078}\u{305e}\u{3072}pay", "pay-q93bzd8a"),
    ("x1-\u{576d}\u{9330}\u{7da9}", "x1--kx6er10oz80a"),
    (
        "x1-\u{3069}\u{3072}\u{3084}\u{3076}\u{305b}\u{3087}",
        "x1--h93btcsc2a5i3a",
    ),
    (
        "\u{b9f5}\u{bf44}\u{d113}\u{b87c}\u{b7d0}",
        "2k2b9lo0bl9kk0z",
    ),
    ("x1-\u{3087}\u{305c}\u{3083}", "x1--l93buhwa"),
    ("pay\u{305f}\u{3075}", "pay-x93b9d"),
    ("pay12\u{b240}\u{b300}", "pay12-346ty4b"),
    ("\u{307f}\u{306a}\u{305b}pay", "pay-e93byc4f"),
    ("\u{308e}\u{308d}\u{3079}\u{3067}pay", "pay-rb4bodtfg"),
];

#[test]
fn a_label_takes_the_a_label_that_another_encoder_of_rfc_3492_gives() {
    for (label, encoded) in CODEC {
        let chars: Vec<char> = label.chars().collect();
        assert_eq!(
            punycode(&chars).as_deref(),
            Some(*encoded),
            "reader: {label}"
        );
        assert_eq!(
            origin_of(&format!("http://{label}.test/")),
            Some(format!("http://xn--{encoded}.test:80")),
            "function: {label}"
        );
    }
}

#[test]
fn a_long_label_is_taken_while_its_a_label_fits_63_bytes() {
    let taken = [
        "\u{fc}".repeat(57),
        format!("{}\u{fc}", "a".repeat(50)),
        format!("{}\u{fc}", "a".repeat(55)),
    ];
    for label in &taken {
        let address = format!("http://{label}.test/");
        let origin =
            origin_of(&address).unwrap_or_else(|| panic!("{} letters", label.chars().count()));
        assert_eq!(Some(origin.clone()), read_origin(&address));
        let host = origin
            .trim_start_matches("http://")
            .trim_end_matches(".test:80");
        assert!(host.starts_with("xn--") && host.len() <= 63, "{host}");
    }
    assert_eq!(
        origin_of(&format!("http://{}.test/", "\u{fc}".repeat(57))),
        Some(format!("http://xn--tda{}.test:80", "a".repeat(56)))
    );
    for label in ["\u{fc}".repeat(58), format!("{}\u{fc}", "a".repeat(56))] {
        let address = format!("http://{label}.test/");
        assert_eq!(
            origin_of(&address),
            None,
            "{} letters",
            label.chars().count()
        );
        assert_eq!(read_origin(&address), None);
    }
}

fn unhex(text: &str) -> String {
    let bytes: Vec<u8> = (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn each_row_of_the_oracle_table_gets_the_origin_that_node_gives_or_none() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/r3")
        .join(TABLE_MADE_BY_NODE);
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.ends_with('\n') && !text.contains('\r'));
    let mut lines = text.lines();
    let first = lines.next().unwrap();
    assert!(first.starts_with("# made by node v"), "{first}");
    let count = |key: &str| -> usize {
        first
            .split([' ', ';'])
            .find_map(|word| word.strip_prefix(key))
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("{key} in {first}"))
    };
    let (want_labels, want_addresses) = (count("labels="), count("addresses="));
    let (mut labels, mut addresses, mut node_takes, mut both_take) = (0, 0, 0, 0);
    for line in lines {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(columns.len(), 3, "{line}");
        let (kind, input, answer) = (columns[0], unhex(columns[1]), columns[2]);
        match kind {
            "label" => {
                labels += 1;
                let want = (answer != "-" && answer.len() <= 63)
                    .then(|| format!("http://{answer}.test:80"));
                assert_eq!(
                    origin_of(&format!("http://{input}.test/")),
                    want,
                    "{input:?}"
                );
            }
            "address" => {
                addresses += 1;
                let got = origin_of(&input);
                if answer == "-" {
                    assert_eq!(got, None, "node refuses {input:?}");
                } else {
                    node_takes += 1;
                    if let Some(got) = got {
                        both_take += 1;
                        assert_eq!(got, answer, "{input:?}");
                    }
                }
            }
            other => panic!("kind {other}"),
        }
    }
    println!("labels={labels} addresses={addresses} node_takes={node_takes} both_take={both_take}");
    assert_eq!(
        (labels, addresses),
        (want_labels, want_addresses),
        "{first}"
    );
    assert_eq!((labels, addresses), (2_985, 2_023), "{first}");
    assert!(both_take * 4 >= node_takes, "{both_take} of {node_takes}");
}

#[test]
fn a_last_label_of_digits_alone_is_refused_at_every_length() {
    for digit in ['0', '1', '9'] {
        for count in 1..=63 {
            let address = format!("http://pay.{}/", digit.to_string().repeat(count));
            let refusal = WebOrigin::of_address(&address).unwrap_err();
            assert_eq!(refusal.rule(), "address_ipv4", "{count} of {digit}");
            assert_eq!(read_origin(&address), None, "{count} of {digit}");
            let origin = format!("http://pay.{}:80", digit.to_string().repeat(count));
            assert_eq!(
                WebOrigin::parse(&origin).unwrap_err().rule(),
                "origin_ipv4",
                "{count} of {digit}"
            );
        }
    }
}

fn origin_as_written(address: &str) -> Option<String> {
    let (scheme, rest) = address.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let authority = rest
        .split(['/', '?', '#', '\\'])
        .next()?
        .to_ascii_lowercase();
    let port = if scheme == "https" { "443" } else { "80" };
    Some(match authority.contains(':') {
        true => format!("{scheme}://{authority}"),
        false => format!("{scheme}://{authority}:{port}"),
    })
}

fn number_form(origin: &str) -> bool {
    let host = origin
        .split_once("://")
        .and_then(|(_, rest)| rest.rsplit_once(':'))
        .map_or("", |(host, _)| host);
    let last = host.rsplit('.').next().unwrap_or_default();
    let numeric = last.bytes().all(|b| b.is_ascii_digit())
        || last
            .strip_prefix("0x")
            .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()));
    numeric && !(host.split('.').count() == 4 && host.split('.').all(octet))
}

/// `WebOrigin::parse` and `WebOrigin::of_address` take the same origins over strings built from the table's inputs
/// and answers: the parser takes a string exactly when the function gives that string back. It compares the code
/// with the code and reads node's answers as inputs only; node's answers bind the code in
/// `each_row_of_the_oracle_table_gets_the_origin_that_node_gives_or_none`. Its three counts were read from a run.
#[test]
fn the_parser_and_of_address_take_the_same_origins_over_the_inputs_of_the_oracle_table() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/r3")
        .join(TABLE_MADE_BY_NODE);
    let text = std::fs::read_to_string(path).unwrap();
    let mut candidates = Vec::new();
    for line in text.lines().skip(1) {
        let columns: Vec<&str> = line.split('\t').collect();
        let (kind, input, answer) = (columns[0], unhex(columns[1]), columns[2]);
        match kind {
            "label" => {
                candidates.push(format!("http://{input}.test:80"));
                candidates.push(format!("http://{answer}.test:80"));
            }
            _ => {
                candidates.extend(origin_as_written(&input));
                candidates.push(answer.to_owned());
            }
        }
    }
    let (mut taken, mut numbers) = (0, 0);
    for origin in &candidates {
        let parsed = WebOrigin::parse(origin);
        let made = WebOrigin::of_address(origin);
        match &parsed {
            Ok(o) => {
                taken += 1;
                assert_eq!(made.as_ref(), Ok(o), "{origin:?}");
            }
            Err(_) => assert!(
                made.as_ref().map_or(true, |o| o.as_str() != origin),
                "{origin:?}"
            ),
        }
        if number_form(origin) {
            let rule = parsed.expect_err(origin).rule();
            numbers += usize::from(rule == "origin_ipv4");
        }
    }
    println!(
        "candidates={} taken={taken} numbers={numbers}",
        candidates.len()
    );
    assert_eq!((candidates.len(), taken, numbers), (9_798, 4_646, 30));
}
