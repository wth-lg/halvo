use r4_trial_app::pages::{may_navigate, page_url};

#[test]
fn the_window_may_navigate_to_its_own_scheme_alone() {
    assert!(may_navigate(&page_url("index.html")));
    assert!(may_navigate(&page_url("sub/view.html")));
    for url in [
        "https://example.com/",
        "http://example.com/index.html",
        "http://127.0.0.1:47811/",
        "http://localhost/index.html",
        "about:blank",
        "file:///etc/hosts",
        "data:text/html,x",
        "ipc://localhost/report",
        "http://ipc.localhost/report",
        "r4trial://localhost.example.com/index.html",
        "r4trial://other/index.html",
        "http://r4trial.localhost.example.com/index.html",
        "http://r4trial.localhost:8080/index.html",
        "https://r4trial.localhost/index.html",
        "",
    ] {
        assert!(!may_navigate(url), "{url:?}");
    }
}
