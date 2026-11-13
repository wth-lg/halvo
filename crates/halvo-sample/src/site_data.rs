//! The site data of the fixture server (contracts.md C11.1), read by the rules D1 to D13 as the server's
//! reader reads them.

use std::collections::BTreeSet;

use halvo_schema::Category;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::Deserialize;

use crate::form::{json_opt, json_str, json_strs, TITLE_MAX};

/// The test card number that the title of a card page holds (C11.2).
pub const CARD_NUMBER: &str = "4242 4242 4242 4242";

const CAPTURE_KEYS: [&str; 4] = ["category", "site", "trigger", "recurrence"];

/// The role of a flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Task,
    NearMiss,
    Other,
}

/// A page of a service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub page: String,
    pub title: String,
    pub text: String,
    pub password_input: bool,
    pub card_title: bool,
}

/// A flow of a service: the page order of one way through it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
    pub flow: String,
    pub role: Role,
    pub pages: Vec<String>,
}

/// One service: one site data file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    pub service: String,
    pub category: Category,
    pub web: bool,
    pub host: Option<String>,
    pub app: Option<String>,
    pub parameters: Vec<String>,
    pub values: Vec<(String, Vec<String>)>,
    pub pages: Vec<Page>,
    pub flows: Vec<Flow>,
}

struct Values(Vec<(String, Vec<String>)>);

impl<'de> Deserialize<'de> for Values {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Values;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Values, A::Error> {
                let mut out: Vec<(String, Vec<String>)> = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Vec<String>>()? {
                    if out.iter().any(|(n, _)| *n == k) {
                        return Err(de::Error::custom("twice"));
                    }
                    out.push((k, v));
                }
                Ok(Values(out))
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPage {
    page: String,
    title: String,
    text: String,
    password_input: bool,
    card_title: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFlow {
    flow: String,
    role: String,
    pages: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawService {
    v: u64,
    service: String,
    category: String,
    kind: String,
    host: Option<String>,
    app: Option<String>,
    parameters: Vec<String>,
    values: Values,
    pages: Vec<RawPage>,
    flows: Vec<RawFlow>,
}

fn write_page(p: &Page) -> String {
    format!(
        "{{\"page\":{},\"title\":{},\"text\":{},\"password_input\":{},\"card_title\":{}}}",
        json_str(&p.page),
        json_str(&p.title),
        json_str(&p.text),
        p.password_input,
        p.card_title
    )
}

fn role_name(r: Role) -> &'static str {
    match r {
        Role::Task => "task",
        Role::NearMiss => "near_miss",
        Role::Other => "other",
    }
}

fn write_flow(f: &Flow) -> String {
    format!(
        "{{\"flow\":{},\"role\":{},\"pages\":{}}}",
        json_str(&f.flow),
        json_str(role_name(f.role)),
        json_strs(&f.pages)
    )
}

impl Service {
    /// The file's compact rewriting in the key order of C11.1, with its final line feed.
    pub fn write(&self) -> String {
        let values: Vec<String> = self
            .values
            .iter()
            .map(|(n, v)| format!("{}:{}", json_str(n), json_strs(v)))
            .collect();
        format!(
            "{{\"v\":1,\"service\":{},\"category\":{},\"kind\":{},\"host\":{},\"app\":{},\"parameters\":{},\"values\":{{{}}},\"pages\":[{}],\"flows\":[{}]}}\n",
            json_str(&self.service),
            json_str(self.category.as_str()),
            json_str(if self.web { "web" } else { "native" }),
            json_opt(self.host.as_deref()),
            json_opt(self.app.as_deref()),
            json_strs(&self.parameters),
            values.join(","),
            self.pages.iter().map(write_page).collect::<Vec<_>>().join(","),
            self.flows.iter().map(write_flow).collect::<Vec<_>>().join(",")
        )
    }

    /// The values of a parameter name.
    pub fn values_of(&self, name: &str) -> &[String] {
        self.values
            .iter()
            .find(|(n, _)| n == name)
            .map_or(&[], |(_, v)| v.as_slice())
    }

    /// The names that a page uses, in the order of `parameters`.
    pub fn names_of(&self, page: &Page) -> Vec<String> {
        let used: BTreeSet<String> = references(&page.title)
            .into_iter()
            .chain(references(&page.text))
            .collect();
        self.parameters
            .iter()
            .filter(|n| used.contains(*n))
            .cloned()
            .collect()
    }

    /// The page with this id.
    pub fn page(&self, id: &str) -> Option<&Page> {
        self.pages.iter().find(|p| p.page == id)
    }
}

fn is_service_name(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=32).contains(&b.len())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && b[b.len() - 1] != b'-'
}

fn is_page_id(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// A parameter name of C8.2: a lower-case letter, then up to 31 lower-case letters, digits and underscores.
pub fn is_name(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// The references of a template, left to right: a `{`, a name of C8.2 and a `}`. All other text is plain.
/// Returns the byte range of each reference with its name.
fn reference_spans(template: &str) -> Vec<(usize, usize, String)> {
    let b = template.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' && i + 1 < b.len() && b[i + 1].is_ascii_lowercase() {
            let mut j = i + 2;
            while j < b.len()
                && (b[j].is_ascii_lowercase() || b[j].is_ascii_digit() || b[j] == b'_')
            {
                j += 1;
            }
            let len = j - (i + 1);
            if len <= 32 && j < b.len() && b[j] == b'}' {
                out.push((i, j + 1, template[i + 1..j].to_owned()));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// The names that a template refers to, in the order in which they stand.
pub fn references(template: &str) -> Vec<String> {
    reference_spans(template).into_iter().map(|r| r.2).collect()
}

/// The template with each reference replaced by the value of its name, read once, left to right.
pub fn fill(template: &str, value: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut at = 0;
    for (s, e, name) in reference_spans(template) {
        out.push_str(&template[at..s]);
        out.push_str(&value(&name));
        at = e;
    }
    out.push_str(&template[at..]);
    out
}

/// A value percent-encoded byte by byte outside the unreserved set of RFC 3986 (C10.9).
pub fn encode_value(v: &str) -> String {
    let mut out = String::new();
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn spaced_well(s: &str) -> bool {
    !s.starts_with(' ') && !s.ends_with(' ') && !s.contains("  ")
}

fn app_ok(s: &str) -> bool {
    (1..=255).contains(&s.len())
        && s != "."
        && s != ".."
        && !s.chars().any(|c| c == '/' || c == '\\' || c.is_control())
}

/// Reads the bytes of one site data file: taken only when they are UTF-8, one JSON object of the form of C11.1
/// byte-equal to its compact rewriting with one line feed, and the rules D1 to D7 and D9 to D13 hold. D8 is a
/// rule over several files.
pub fn parse_service(bytes: &[u8]) -> Option<Service> {
    let text = std::str::from_utf8(bytes).ok()?;
    let raw: RawService = serde_json::from_str(text).ok()?;
    if raw.v != 1 {
        return None;
    }
    let category = Category::parse(&raw.category).ok()?;
    let web = match raw.kind.as_str() {
        "web" => true,
        "native" => false,
        _ => return None,
    };
    let pages: Vec<Page> = raw
        .pages
        .into_iter()
        .map(|p| Page {
            page: p.page,
            title: p.title,
            text: p.text,
            password_input: p.password_input,
            card_title: p.card_title,
        })
        .collect();
    let flows: Vec<Flow> = raw
        .flows
        .into_iter()
        .map(|f| {
            let role = match f.role.as_str() {
                "task" => Role::Task,
                "near_miss" => Role::NearMiss,
                "other" => Role::Other,
                _ => return None,
            };
            Some(Flow {
                flow: f.flow,
                role,
                pages: f.pages,
            })
        })
        .collect::<Option<_>>()?;
    let s = Service {
        service: raw.service,
        category,
        web,
        host: raw.host,
        app: raw.app,
        parameters: raw.parameters,
        values: raw.values.0,
        pages,
        flows,
    };
    if s.write().as_bytes() != bytes {
        return None;
    }
    rules_hold(&s).then_some(s)
}

fn rules_hold(s: &Service) -> bool {
    if !is_service_name(&s.service) {
        return false;
    }
    let kind_ok = if s.web {
        s.host.as_deref() == Some(format!("{}.test", s.service).as_str()) && s.app.is_none()
    } else {
        s.host.is_none() && s.app.as_deref().is_some_and(app_ok)
    };
    if !kind_ok || !s.parameters.iter().all(|n| is_name(n)) {
        return false;
    }
    let names: Vec<&String> = s.values.iter().map(|(n, _)| n).collect();
    let params: Vec<&String> = s.parameters.iter().collect();
    if names != params {
        return false;
    }
    for (_, list) in &s.values {
        let distinct: BTreeSet<&String> = list.iter().collect();
        if list.is_empty() || distinct.len() != list.len() {
            return false;
        }
        let ok = list.iter().all(|v| {
            (1..=64).contains(&v.len())
                && !v.chars().any(|c| c < ' ' || c == '\u{7f}')
                && spaced_well(v)
        });
        if !ok {
            return false;
        }
    }
    let page_ids: Vec<&str> = s.pages.iter().map(|p| p.page.as_str()).collect();
    let distinct_pages: BTreeSet<&str> = page_ids.iter().copied().collect();
    let flow_ids: BTreeSet<&str> = s.flows.iter().map(|f| f.flow.as_str()).collect();
    if s.pages.is_empty()
        || distinct_pages.len() != page_ids.len()
        || flow_ids.len() != s.flows.len()
        || !s
            .pages
            .iter()
            .all(|p| is_page_id(&p.page) && p.page != "api")
        || !s.flows.iter().all(|f| is_page_id(&f.flow))
    {
        return false;
    }
    let mut used = BTreeSet::new();
    for p in &s.pages {
        for n in references(&p.title).into_iter().chain(references(&p.text)) {
            if !s.parameters.contains(&n) {
                return false;
            }
            used.insert(n);
        }
    }
    if s.flows
        .iter()
        .any(|f| f.pages.is_empty() || f.pages.iter().any(|p| !distinct_pages.contains(p.as_str())))
    {
        return false;
    }
    let seed = s.category != Category::Other;
    let count = |r: Role| s.flows.iter().filter(|f| f.role == r).count();
    if seed && (count(Role::Task) != 1 || count(Role::NearMiss) != 1 || count(Role::Other) != 0) {
        return false;
    }
    if !seed && (s.flows.is_empty() || s.flows.iter().any(|f| f.role != Role::Other)) {
        return false;
    }
    let task = s.flows.iter().find(|f| f.role == Role::Task);
    if seed && s.web {
        let pw: Vec<&Page> = s.pages.iter().filter(|p| p.password_input).collect();
        if pw.len() != 1 || task.and_then(|t| t.pages.first()) != Some(&pw[0].page) {
            return false;
        }
    }
    if s.category == Category::ShopOrder {
        let card: Vec<&Page> = s.pages.iter().filter(|p| p.card_title).collect();
        if card.len() != 1 || !task.is_some_and(|t| t.pages.contains(&card[0].page)) {
            return false;
        }
    }
    if s.pages
        .iter()
        .any(|p| p.card_title && !p.title.contains(CARD_NUMBER))
    {
        return false;
    }
    if s.parameters
        .iter()
        .any(|n| CAPTURE_KEYS.contains(&n.as_str()))
    {
        return false;
    }
    if !seed && !s.parameters.is_empty() {
        return false;
    }
    if s.pages.iter().any(|p| !spaced_well(&p.title)) {
        return false;
    }
    if s.parameters.iter().any(|n| !used.contains(n)) {
        return false;
    }
    s.pages.iter().all(|p| filled_title_ok(s, &p.title))
}

/// D12: after filling, a title is 1 to 1,024 bytes and holds no character below U+0020 and not U+007F. The
/// values hold no such character (their own rule), so the plain text of the template is checked, and the
/// shortest and the longest filling are measured with the shortest and the longest value of each name.
fn filled_title_ok(s: &Service, title: &str) -> bool {
    let spans = reference_spans(title);
    let mut plain = String::new();
    let mut at = 0;
    for (b, e, _) in &spans {
        plain.push_str(&title[at..*b]);
        at = *e;
    }
    plain.push_str(&title[at..]);
    if plain.chars().any(|c| c < ' ' || c == '\u{7f}') {
        return false;
    }
    let (mut low, mut high) = (plain.len(), plain.len());
    for (_, _, name) in &spans {
        let lens: Vec<usize> = s.values_of(name).iter().map(String::len).collect();
        low += lens.iter().min().copied().unwrap_or(0);
        high += lens.iter().max().copied().unwrap_or(0);
    }
    low >= 1 && high <= TITLE_MAX
}
