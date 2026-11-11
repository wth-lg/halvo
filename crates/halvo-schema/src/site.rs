use std::fmt;

use serde::{Deserialize, Serialize, Serializer};

use crate::ids::{hex64, name_enum, object, string_type, ParseError};

pub(crate) struct Key(());

/// The names that this module writes in a record or a request: the keys of a site value and of an observed site,
/// and the three sources.
pub const NAMES: &[&str] = &[
    "key",
    "source",
    "address_field",
    "sign_in",
    "title",
    "domain",
    "token",
];

fn app(s: &str) -> Result<(), &'static str> {
    if s.is_empty() || s.len() > 255 {
        return Err("length");
    }
    if s.contains(['/', '\\']) {
        return Err("separator");
    }
    if s.chars().any(char::is_control) {
        return Err("control");
    }
    if s == "." || s == ".." {
        return Err("dot_name");
    }
    Ok(())
}

string_type! {
    /// An app id: a bundle id or the file name of an executable, never a path. `parse` accepts 1 to 255 bytes
    /// with no `/`, no `\`, no control character (the Unicode category Cc: U+0000 to U+001F, U+007F and U+0080
    /// to U+009F), and neither `.` nor `..`, and keeps the bytes as given.
    AppId [Debug, PartialOrd, Ord, Hash] = app
}

impl AppId {
    /// Cuts a stored executable path to its file name: the part after the last `/` or `\`, or the whole
    /// string when it has neither, and returns what `parse` returns for it.
    pub fn from_capture(s: &str) -> Result<AppId, ParseError> {
        match s.rfind(['/', '\\']) {
            Some(i) => AppId::parse(&s[i + 1..]),
            None => AppId::parse(s),
        }
    }
}

fn domain_form(s: &str) -> Result<(), &'static str> {
    if s.is_empty() || s.len() > 253 || !s.is_ascii() {
        return Err("ascii_length");
    }
    if s.split('.').any(str::is_empty) {
        return Err("label_join");
    }
    for label in s.split('.') {
        let b = label.as_bytes();
        let allowed = b
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-');
        if b.len() > 63 || !allowed || b[0] == b'-' || b[b.len() - 1] == b'-' {
            return Err("label_form");
        }
    }
    let last = s.rsplit('.').next().unwrap_or(s);
    if last.bytes().all(|c| c.is_ascii_digit()) {
        return Err("numeric_last_label");
    }
    if let Some(rest) = last.strip_prefix("0x") {
        if rest.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("numeric_last_label");
        }
    }
    Ok(())
}

fn suffix_labels(name: &str) -> Option<usize> {
    psl::suffix(name.as_bytes()).map(|suffix| suffix.as_bytes().split(|b| *b == b'.').count())
}

fn domain(s: &str) -> Result<(), &'static str> {
    domain_form(s)?;
    match suffix_labels(s) {
        Some(n) if s.split('.').count() == n + 1 => Ok(()),
        _ => Err("public_suffix"),
    }
}

string_type! {
    /// A registrable domain in one written form: 1 to 253 ASCII bytes; lower-case labels of 1 to 63 bytes from
    /// `a` to `z`, `0` to `9` and `-` that neither begin nor end with `-`, joined by single dots; a last label
    /// that is neither all digits nor `0x` with hexadecimal digits; and exactly one label more than its public
    /// suffix, by the rules of the Public Suffix List that the crate `psl` embeds, both sections and the
    /// default rule `*` included.
    Domain [Debug, PartialOrd, Ord, Hash] = domain
}

impl Domain {
    /// Cuts a host name, never a URL, to its registrable domain: folds ASCII letters to lower case, drops one
    /// `.` at the end, checks the written form and keeps the public suffix plus one label. Returns none when the
    /// name breaks a rule of the form or is itself a public suffix; a byte outside ASCII gives none.
    pub fn registrable(host: &str) -> Option<Domain> {
        let lower = host.to_ascii_lowercase();
        let name = lower.strip_suffix('.').unwrap_or(&lower);
        domain_form(name).ok()?;
        let n = suffix_labels(name)?;
        let labels: Vec<&str> = name.split('.').collect();
        if labels.len() <= n {
            return None;
        }
        Domain::parse(&labels[labels.len() - n - 1..].join(".")).ok()
    }
}

fn site_domain(s: &str) -> Result<(), &'static str> {
    domain_form(s)?;
    if !s.contains('.') {
        return Err("two_labels");
    }
    Ok(())
}

string_type! {
    /// A domain as a stored record holds it: the written form of a `Domain` (its rules 1 to 4) with two labels or
    /// more, and no check against the Public Suffix List, so that a stored key stays readable when the pin of the
    /// list moves. `From<Domain>` is its one conversion; a caller that needs rule 5 calls `Domain::parse` on its
    /// `as_str()`.
    SiteDomain [Debug, PartialOrd, Ord, Hash] = site_domain
}

impl From<Domain> for SiteDomain {
    fn from(domain: Domain) -> Self {
        SiteDomain(domain.0)
    }
}

fn token(s: &str) -> Result<(), &'static str> {
    if s.is_empty() || s.len() > 64 {
        return Err("length");
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
    {
        return Err("character");
    }
    Ok(())
}

string_type! {
    /// The one title-derived site token in plain text: 1 to 64 bytes from `a` to `z`, `0` to `9`, `-` and `.`.
    /// It is legal only in window-class data and in the analysis write. `Debug` prints the type name alone.
    SiteToken [PartialOrd, Ord, Hash] = token
}

impl fmt::Debug for SiteToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SiteToken")
    }
}

string_type! {
    /// The keyed hash of a site token: 64 lower-case hex digits. halvo-derived computes it at commit; this crate
    /// holds no key and no hash function.
    SiteHash [Debug, PartialOrd, Ord, Hash] = hex64
}

name_enum! {
    /// The two sources of a domain-shaped site value.
    pub enum DomainSource {
        AddressField = "address_field",
        SignIn = "sign_in",
    }
}

/// A site value as bounded and kept records store it, by its key and its source:
/// `{"key":"<domain>","source":"address_field"|"sign_in"}` or `{"key":"<site hash>","source":"title"}`.
/// A key whose form does not fit its source cannot be built and is refused by the reader. The key of the domain
/// form is a `SiteDomain`, which the reader does not check against the Public Suffix List.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SiteValue {
    /// A domain from the address field or a sign-in, stored from a registrable domain.
    Domain {
        key: SiteDomain,
        source: DomainSource,
    },
    /// The keyed hash of the title-derived site token. It is a hint and never an allowed origin.
    Title { key: SiteHash },
}

impl Serialize for SiteValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Out<'a> {
            key: &'a str,
            source: &'static str,
        }
        match self {
            SiteValue::Domain { key, source } => Out {
                key: key.as_str(),
                source: source.as_str(),
            },
            SiteValue::Title { key } => Out {
                key: key.as_str(),
                source: "title",
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SiteValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
        enum Stored {
            AddressField { key: SiteDomain },
            SignIn { key: SiteDomain },
            Title { key: SiteHash },
        }
        Ok(match object(deserializer)? {
            Stored::AddressField { key } => SiteValue::Domain {
                key,
                source: DomainSource::AddressField,
            },
            Stored::SignIn { key } => SiteValue::Domain {
                key,
                source: DomainSource::SignIn,
            },
            Stored::Title { key } => SiteValue::Title { key },
        })
    }
}

/// A site as the analysis write carries it, before commit: `{"domain":"<domain>","source":"address_field"}` or
/// `{"token":"<site token>","source":"title"}`. Its key names differ from those of `SiteValue`, so a plain
/// token never parses as a stored site value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ObservedSite {
    /// The registrable domain cut from the address field.
    AddressField { domain: Domain },
    /// The title-derived site token in plain text.
    Title { token: SiteToken },
}

impl<'de> Deserialize<'de> for ObservedSite {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
        enum Observed {
            AddressField { domain: Domain },
            Title { token: SiteToken },
        }
        Ok(match object(deserializer)? {
            Observed::AddressField { domain } => ObservedSite::AddressField { domain },
            Observed::Title { token } => ObservedSite::Title { token },
        })
    }
}

impl Serialize for ObservedSite {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct AddressField<'a> {
            domain: &'a Domain,
            source: &'static str,
        }
        #[derive(Serialize)]
        struct Title<'a> {
            token: &'a SiteToken,
            source: &'static str,
        }
        match self {
            ObservedSite::AddressField { domain } => AddressField {
                domain,
                source: "address_field",
            }
            .serialize(serializer),
            ObservedSite::Title { token } => Title {
                token,
                source: "title",
            }
            .serialize(serializer),
        }
    }
}

macro_rules! conversion_case {
    ($fence:literal, $from:literal, $into:literal) => {
        concat!(
            "```",
            $fence,
            "\n",
            "use halvo_schema::{Domain, SiteDomain};\n",
            "let from = ",
            $from,
            "::parse(\"shop.test\").unwrap();\n",
            "let into = ",
            $into,
            "::from(from);\n",
            "assert_eq!(into.as_str(), \"shop.test\");\n",
            "```"
        )
    };
}

/// What must not compile: no conversion runs from a stored `SiteDomain` to a `Domain`, because a stored key need
/// not hold rule 5 under the list of this build. The case differs from its twin in `compiles` in the direction
/// alone.
pub mod compile_fail {
    /// No `Domain::from` of a `SiteDomain`. Twin: `compiles::site_domain_from_domain`.
    #[doc = conversion_case!("compile_fail", "SiteDomain", "Domain")]
    pub mod domain_from_site_domain {}
}

/// The twin of the case of `compile_fail`, which compiles and runs.
pub mod compiles {
    /// A `SiteDomain` is built from a `Domain`.
    #[doc = conversion_case!("", "Domain", "SiteDomain")]
    pub mod site_domain_from_domain {}
}
