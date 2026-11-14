//! The site of a window title (R2-Q4): a registrable domain read from the
//! address field of a browser, or nothing.

use std::fmt;

use zeroize::Zeroize;

use super::text::{CapturedText, TextField};

/// The most bytes of a site: the length limit of a DNS name.
pub const SITE_MAX_BYTES: usize = 253;

/// The longest address-field value that the cut reads; a longer one is dropped.
pub const ADDRESS_MAX_BYTES: usize = 2048;

/// A registrable domain, lower case, read from the address field: no scheme,
/// user, port, path, query or fragment. Its `Debug` prints a placeholder.
pub struct Site {
    domain: CapturedText,
}

impl Site {
    /// The cut, the one constructor: a site only for a URL with an explicit
    /// `http` or `https` scheme whose host is an ASCII DNS name under a known
    /// public suffix of the compiled-in Public Suffix List, keeping that host's
    /// registrable domain alone. Every other value gives nothing: a search text,
    /// a value with no scheme, another scheme, an IP literal, `localhost`, a
    /// bare public suffix, a host with a non-ASCII label, a value above
    /// [`ADDRESS_MAX_BYTES`]. The address is wiped.
    pub fn cut(mut address: String) -> Option<Site> {
        let domain = registrable_domain(&address);
        address.zeroize();
        domain.map(|domain| Site {
            domain: CapturedText::enter(TextField::Site, domain),
        })
    }
}

fn registrable_domain(address: &str) -> Option<String> {
    if address.len() > ADDRESS_MAX_BYTES {
        return None;
    }
    let rest = strip_scheme(address)?;
    let authority = rest.split(['/', '\\', '?', '#']).next()?;
    let host_and_port = authority.rsplit('@').next()?;
    if host_and_port.starts_with('[') {
        return None;
    }
    let host = match host_and_port.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
        Some(_) => return None,
        None => host_and_port,
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > SITE_MAX_BYTES || !host.is_ascii() {
        return None;
    }
    let mut lower = host.to_ascii_lowercase();
    let domain = known_domain(&lower);
    lower.zeroize();
    domain
}

fn strip_scheme(address: &str) -> Option<&str> {
    let (scheme, rest) = address.split_once("://")?;
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        Some(rest)
    } else {
        None
    }
}

fn known_domain(host: &str) -> Option<String> {
    if !host.split('.').all(is_dns_label) {
        return None;
    }
    let domain = psl::domain(host.as_bytes())?;
    if !domain.suffix().is_known() {
        return None;
    }
    let domain = std::str::from_utf8(domain.as_bytes()).ok()?;
    (domain.len() <= SITE_MAX_BYTES).then(|| domain.to_owned())
}

fn is_dns_label(label: &str) -> bool {
    (1..=63).contains(&label.len())
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Site {
    pub(super) fn domain(&self) -> &CapturedText {
        &self.domain
    }

    #[cfg(test)]
    pub(crate) fn heap_bytes(&self) -> usize {
        self.domain.heap_bytes()
    }
}

impl fmt::Debug for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Site(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::floor::cases::stored;

    fn cut(address: &str) -> Option<String> {
        Site::cut(address.to_string()).map(|site| stored(&site.domain, TextField::Site))
    }

    #[test]
    fn the_cut() {
        let kept = [
            ("https://www.example.co.uk/a?b=c#d", "example.co.uk"),
            ("http://user:pw@host.example.com:8080/x", "example.com"),
            ("HTTPS://Mail.Example.COM/inbox", "example.com"),
            ("https://example.com", "example.com"),
            ("https://example.com.", "example.com"),
            ("https://a.b.c.example.org?q=1", "example.org"),
            ("https://example.com#top", "example.com"),
            ("https://example.com\\path", "example.com"),
            ("http://u@p@deep.example.net/", "example.net"),
            ("https://shop.xn--bcher-kva.de/", "xn--bcher-kva.de"),
        ];
        let long_host = format!("https://{}.example.com/", "a".repeat(63));
        let dropped = [
            "how to bake bread",
            "example.com/path",
            "www.example.com",
            "file:///etc/hosts",
            "about:blank",
            "chrome://settings",
            "data:text/html,<p>x</p>",
            "javascript:alert(1)",
            "ftp://example.com/",
            "https://192.168.1.10/admin",
            "http://127.0.0.1:8080/",
            "http://[::1]/",
            "https://[2001:db8::1]:443/",
            "http://localhost:3000/",
            "http://app.localhost/",
            "https://co.uk/",
            "https://com",
            "https://bücher.de/",
            "https://example.com:80a/",
            "https://exa mple.com/",
            "https://under_score.example.com/",
            "https://-dash.example.com/",
            "https://intranet.corp/",
            "https://example..com/",
            "https://",
            "https:example.com",
            "",
        ];
        for (address, domain) in kept {
            assert_eq!(cut(address).as_deref(), Some(domain), "{address}");
        }
        assert_eq!(cut(&long_host).as_deref(), Some("example.com"));
        for address in dropped {
            assert_eq!(cut(address), None, "{address}");
        }
        let labels = format!("{}.{}.{}", "a".repeat(63), "b".repeat(63), "c".repeat(63));
        let longest = format!("{labels}.{}.com", "d".repeat(57));
        assert_eq!(longest.len(), SITE_MAX_BYTES);
        let domain = format!("{}.com", "d".repeat(57));
        assert_eq!(cut(&format!("https://{longest}/")), Some(domain));
        let too_long = format!("{labels}.{}.com", "d".repeat(58));
        assert_eq!(too_long.len(), SITE_MAX_BYTES + 1);
        assert_eq!(cut(&format!("https://{too_long}/")), None);
        assert_eq!(
            cut(&format!("https://{}.com/", "a".repeat(63))).map(|d| d.len()),
            Some(67)
        );
        assert_eq!(cut(&format!("https://{}.com/", "a".repeat(64))), None);
        assert_eq!(
            cut(&format!("https://{}.example.com/", "a".repeat(64))),
            None
        );
        let over = format!("https://example.com/{}", "a".repeat(ADDRESS_MAX_BYTES));
        assert_eq!(cut(&over), None);
        let at_limit = format!("https://example.com/{}", "a".repeat(ADDRESS_MAX_BYTES - 20));
        assert_eq!(at_limit.len(), ADDRESS_MAX_BYTES);
        assert_eq!(cut(&at_limit).as_deref(), Some("example.com"));
        for (address, _) in kept {
            let domain = cut(address).unwrap();
            assert!(domain.len() <= SITE_MAX_BYTES);
            for banned in ['/', '?', '@', ':', '#', '\\'] {
                assert!(!domain.contains(banned), "{address}: {domain}");
            }
        }
    }

    #[test]
    fn a_site_prints_a_placeholder() {
        let site = Site::cut("https://www.example.com/".to_string()).unwrap();
        assert_eq!(format!("{site:?}"), "Site(..)");
    }
}
