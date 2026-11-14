#[path = "hosted/gate.rs"]
#[allow(dead_code)]
mod gate;

#[test]
fn the_dialog_cases_run_on_a_hosted_runner_alone() {
    assert!(gate::hosted(Some("true"), Some("github-hosted")));
    assert!(!gate::hosted(Some("true"), None), "no runner environment");
    assert!(!gate::hosted(None, Some("github-hosted")), "not in Actions");
    assert!(
        !gate::hosted(None, None),
        "this Mac and every other machine"
    );
    assert!(
        !gate::hosted(Some("true"), Some("self-hosted")),
        "a runner of the owner's own"
    );
    assert!(
        !gate::hosted(Some("1"), Some("github-hosted")),
        "another word"
    );
}

fn reader(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name: &str| {
        pairs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.to_string())
    }
}

const HOSTED: &[(&str, &str)] = &[
    ("GITHUB_ACTIONS", "true"),
    ("RUNNER_ENVIRONMENT", "github-hosted"),
];

#[test]
fn the_early_return_is_taken_only_when_the_read_at_the_return_says_not_hosted() {
    let fail = gate::at_return(Some("true"), Some("github-hosted"));
    let gate::Start::Fail(line) = fail else {
        panic!("the read at the return said hosted and the target did not fail: {fail:?}");
    };
    assert!(line.contains("GITHUB_ACTIONS=Some(\"true\")"), "{line}");
    assert!(
        line.contains("RUNNER_ENVIRONMENT=Some(\"github-hosted\")"),
        "{line}"
    );
    assert_eq!(gate::at_return(Some("true"), None), gate::Start::Off);
    assert_eq!(
        gate::at_return(None, Some("github-hosted")),
        gate::Start::Off
    );
    assert_eq!(gate::at_return(None, None), gate::Start::Off);
}

#[test]
fn the_guard_runs_the_cases_on_a_hosted_runner_and_is_off_on_every_other_machine() {
    assert_eq!(gate::start(&reader(HOSTED)), gate::Start::Run);
    assert_eq!(gate::start(&reader(&[])), gate::Start::Off);
    assert_eq!(
        gate::start(&reader(&[("GITHUB_ACTIONS", "true")])),
        gate::Start::Off
    );
}

#[test]
fn a_second_read_that_says_hosted_after_a_first_that_said_not_fails_the_target() {
    let reads = std::cell::Cell::new(0);
    let read = |name: &str| {
        reads.set(reads.get() + 1);
        if reads.get() <= 2 {
            None
        } else {
            reader(HOSTED)(name)
        }
    };
    assert!(matches!(gate::start(&read), gate::Start::Fail(_)));
    assert_eq!(reads.get(), 4, "two variables, read twice");
}
