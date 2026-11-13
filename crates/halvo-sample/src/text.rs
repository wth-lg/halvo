//! Site names, title tokens, site-masked titles and episode templates (contracts.md C12.4).

/// The site name of a domain: the domain without its public suffix and the dot before it. A Domain holds
/// exactly one label more than its public suffix (C1.6), so the site name is its first label.
pub fn site_name(domain: &str) -> &str {
    domain.split('.').next().unwrap_or(domain)
}

/// The tokens of a text: its ASCII letters put in lower case, then its longest runs of ASCII letters and
/// digits; every other byte parts two tokens.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() {
            current.push(char::from(b.to_ascii_lowercase()));
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// The site-masked form of a title: its tokens in order joined by one space, with every token that is also a
/// token of the site name of `site` replaced by `*`. A step with no site masks nothing.
pub fn site_masked(title: &str, site: Option<&str>) -> String {
    let site_tokens = site.map(|d| tokens(site_name(d))).unwrap_or_default();
    tokens(title)
        .into_iter()
        .map(|t| {
            if site_tokens.contains(&t) {
                "*".to_owned()
            } else {
                t
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The template of an episode: the site-masked forms of its title events in the order given, with an entry
/// that equals the one before it left out.
pub fn template<I: IntoIterator<Item = String>>(masked: I) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in masked {
        if out.last() != Some(&m) {
            out.push(m);
        }
    }
    out
}
