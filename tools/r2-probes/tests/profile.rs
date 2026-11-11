use r2_probes::suite::{profile_of, Profile};
use r2_probes::sys::Browser;

fn browser(name: &str) -> Browser {
    Browser {
        name: name.to_string(),
        path: String::new(),
        version: String::new(),
    }
}

#[test]
fn safari_runs_with_the_profile_of_the_account_and_every_other_browser_with_a_new_one() {
    assert_eq!(profile_of(&browser("safari")), Profile::Account);
    for name in ["chrome", "edge", "firefox"] {
        assert_eq!(profile_of(&browser(name)), Profile::New, "{name}");
    }
    assert_eq!(Profile::Account.as_str(), "account");
    assert_eq!(Profile::New.as_str(), "new");
}
