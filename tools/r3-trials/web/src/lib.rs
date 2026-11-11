#[cfg(feature = "driver")]
pub mod cdp;
pub mod fixture;

pub const CANDIDATE: &str = "cdp";

pub const STEPS: [&str; 9] = [
    "signin_form",
    "table_read",
    "form_fill",
    "payment_frame",
    "file_upload",
    "session_expiry",
    "changed_page_1",
    "block_request",
    "credential_mark",
];

pub const FIRST_FLAGS: [&str; 11] = [
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-sync",
    "--disable-field-trial-config",
    "--disable-client-side-phishing-detection",
    "--disable-default-apps",
    "--disable-breakpad",
    "--disable-domain-reliability",
    "--metrics-recording-only",
    "--no-first-run",
    "--no-default-browser-check",
];

pub const RESOLVER_RULE: &str = "--host-resolver-rules=MAP *.test 127.0.0.1,MAP * ~NOTFOUND";

pub fn variants_of(step: &str) -> &'static [&'static str] {
    match step {
        "session_expiry" => &["session_expiry"],
        "changed_page_1" => &["changed_page_1"],
        "credential_mark" => &["second_factor", "captcha"],
        _ => &[],
    }
}

pub fn browser_kind(path: &str) -> Option<&'static str> {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    match name.as_str() {
        "google chrome" | "chrome.exe" | "chrome" | "google chrome for testing" => Some("chrome"),
        "microsoft edge" | "msedge.exe" | "msedge" => Some("edge"),
        _ => None,
    }
}

pub fn major_of(product: &str) -> Option<u32> {
    product.rsplit('/').next()?.split('.').next()?.parse().ok()
}

pub fn median(values: &mut [u128]) -> u128 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2
    }
}

pub fn extract_amount(text: &str) -> Option<u64> {
    let (whole, cents) = text.split_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || cents.len() != 2 || !digits(cents) {
        return None;
    }
    let whole: u64 = whole.parse().ok()?;
    let cents: u64 = cents.parse().ok()?;
    whole.checked_mul(100)?.checked_add(cents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_amount_takes_two_decimals_only() {
        assert_eq!(extract_amount("84.37"), Some(8437));
        assert_eq!(extract_amount("0.05"), Some(5));
        for bad in [
            "84.3", "84.370", "84", ".37", "8a.37", "-1.00", "84.37 ", "1,084.37", "",
        ] {
            assert_eq!(extract_amount(bad), None, "{bad}");
        }
    }

    #[test]
    fn browser_kind_reads_the_file_name() {
        assert_eq!(
            browser_kind("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            Some("chrome")
        );
        assert_eq!(
            browser_kind("C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe"),
            Some("edge")
        );
        assert_eq!(browser_kind("/usr/bin/firefox"), None);
    }

    #[test]
    fn major_of_reads_the_product() {
        assert_eq!(major_of("HeadlessChrome/158.0.8127.18"), Some(158));
        assert_eq!(major_of("Chrome/x"), None);
    }

    #[test]
    fn median_of_even_and_odd() {
        assert_eq!(median(&mut [5, 1, 3]), 3);
        assert_eq!(median(&mut [4, 1, 3, 2]), 2);
        assert_eq!(median(&mut []), 0);
    }

    #[test]
    fn every_step_has_its_variants() {
        assert_eq!(
            variants_of("credential_mark"),
            &["second_factor", "captcha"]
        );
        assert!(variants_of("signin_form").is_empty());
        assert_eq!(STEPS.len(), 9);
    }

    #[test]
    fn no_flag_is_the_resolver_rule() {
        assert!(FIRST_FLAGS
            .iter()
            .all(|f| !f.contains("host-resolver-rules")));
    }
}
