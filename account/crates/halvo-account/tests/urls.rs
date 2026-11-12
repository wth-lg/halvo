use halvo_account::origin::{is_hosted_list, is_hosted_origin, page_url, Backend, Pages, PAGES};

const HOSTED: Pages = Pages::Hosted("https://checkout.example https://billing.example");

fn opened(pages: &Pages, url: &str) -> bool {
    page_url(url, pages).is_some_and(|page| page.as_str() == url)
}

#[test]
fn a_debug_build_opens_the_loopback_origin_with_any_port() {
    assert!(matches!(PAGES, Pages::Loopback) == cfg!(debug_assertions));
    for url in [
        "http://127.0.0.1",
        "http://127.0.0.1/",
        "http://127.0.0.1:1/x",
        "http://127.0.0.1:8787",
        "http://127.0.0.1:65535/c/pay?session=cs_1#frag",
        "http://127.0.0.1?x=1",
        "http://127.0.0.1#top",
        "http://127.0.0.1:9/a/b/c.html",
    ] {
        assert!(opened(&Pages::Loopback, url), "{url}");
    }
}

#[test]
fn a_debug_build_refuses_every_other_origin_and_every_other_spelling_of_loopback() {
    for url in [
        "",
        "https://127.0.0.1/",
        "http://127.0.0.2/",
        "http://localhost/",
        "http://localhost:8787/",
        "http://[::1]/",
        "http://0x7f.0.0.1/",
        "http://0177.0.0.1/",
        "http://2130706433/",
        "http://127.1/",
        "http://127.0.0.1.evil.example/",
        "http://127.0.0.1@evil.example/",
        "http://user@127.0.0.1/",
        "http://127.0.0.1\\@evil.example/",
        "http://127.0.0.1:0/",
        "http://127.0.0.1:65536/",
        "http://127.0.0.1:080/",
        "http://127.0.0.1:/",
        "http://127.0.0.1:8a/",
        "http://127.0.0.1:123456/",
        "HTTP://127.0.0.1/",
        "http:/127.0.0.1/",
        "http:127.0.0.1",
        "//127.0.0.1/",
        "javascript:alert(1)",
        "file:///etc/hosts",
        "http://127.0.0.1/ x",
        "http://127.0.0.1/\n",
        "http://127.0.0.1/\u{e9}",
        " http://127.0.0.1/",
        "-http://127.0.0.1/",
        "http://127.0.0.1:8787\t",
        "https://checkout.example/",
    ] {
        assert!(page_url(url, &Pages::Loopback).is_none(), "{url:?}");
    }
}

#[test]
fn a_hosted_list_opens_its_own_origins_and_nothing_else() {
    for url in [
        "https://checkout.example",
        "https://checkout.example/c/pay/cs_test_1#fid=abc",
        "https://billing.example/p/session/test_1",
        "https://billing.example?x",
    ] {
        assert!(opened(&HOSTED, url), "{url}");
    }
    for url in [
        "http://checkout.example/",
        "https://checkout.example.evil/",
        "https://checkout.example:443/",
        "https://checkout.example@evil.example/",
        "https://x.checkout.example/",
        "https://CHECKOUT.example/",
        "https://evil.example/",
        "https://checkout.examplex/",
        "http://127.0.0.1:1/x",
        "https://checkout.example/\u{7f}",
        "https://billing.example/a b",
    ] {
        assert!(page_url(url, &HOSTED).is_none(), "{url:?}");
    }
}

#[test]
fn a_hosted_origin_is_https_a_lower_case_host_and_a_port_at_most() {
    for origin in [
        "https://api.halvo.example",
        "https://a.b:8443",
        "https://1.2.3.4",
        "https://a-b.example",
    ] {
        assert!(is_hosted_origin(origin), "{origin}");
    }
    for origin in [
        "",
        "https://",
        "http://api.example",
        "https://api.example/",
        "https://api.example/x",
        "https://API.example",
        "https://a b",
        "https://a:",
        "https://a:0",
        "https://a:65536",
        "https://a:08443",
        "https://a@b",
        "https://a?x",
        "https://a#x",
        "https://a:1:2",
        "ftp://a",
        "https:// a",
    ] {
        assert!(!is_hosted_origin(origin), "{origin:?}");
    }
}

#[test]
fn a_hosted_list_is_hosted_origins_separated_by_one_space() {
    for list in [
        "https://checkout.example",
        "https://checkout.example https://billing.example",
        "https://a https://b:8443 https://c",
    ] {
        assert!(is_hosted_list(list), "{list}");
    }
    for list in [
        "",
        " ",
        " https://a",
        "https://a ",
        "https://a  https://b",
        "https://a,https://b",
        "https://a\thttps://b",
        "https://a http://b",
        "https://a https://b/",
    ] {
        assert!(!is_hosted_list(list), "{list:?}");
    }
}

#[test]
fn a_debug_backend_is_an_address_and_never_a_name() {
    for (text, url) in [
        ("http://127.0.0.1:8787", "http://127.0.0.1:8787/v1/cap"),
        ("https://127.0.0.1:8443", "https://127.0.0.1:8443/v1/cap"),
        ("http://127.0.0.1", "http://127.0.0.1/v1/cap"),
        ("http://10.0.0.2:80", "http://10.0.0.2:80/v1/cap"),
        (
            "http://255.255.255.255:65535",
            "http://255.255.255.255:65535/v1/cap",
        ),
    ] {
        let backend = Backend::from_address(text).unwrap_or_else(|| panic!("{text} refused"));
        assert_eq!(backend.url_of("/v1/cap"), url);
    }
    for text in [
        "",
        "http://localhost:8787",
        "http://backend.example:8787",
        "http://127.0.0.1:8787/",
        "http://127.0.0.1:8787/v1",
        "http://127.1:8787",
        "http://0x7f.0.0.1",
        "http://127.0.0.01:8787",
        "http://010.0.0.1",
        "http://001.2.3.4:8787",
        "http://1.2.3.010",
        "http://127.0.0.001",
        "http://256.0.0.1",
        "http://1.2.3",
        "http://1.2.3.4.5",
        "http://1..2.3",
        "http://127.0.0.1:0",
        "http://127.0.0.1:65536",
        "http://127.0.0.1:08787",
        "http://127.0.0.1:",
        "http://[::1]:8787",
        "ftp://127.0.0.1",
        "HTTP://127.0.0.1",
        "http://user@127.0.0.1",
        "http://127.0.0.1?x",
        "http://127.0.0.1#x",
        " http://127.0.0.1",
        "http://127.0.0.1:8787 ",
        "http://127.0.0.1:8787\n",
        "127.0.0.1:8787",
    ] {
        assert!(Backend::from_address(text).is_none(), "{text:?}");
    }
}
