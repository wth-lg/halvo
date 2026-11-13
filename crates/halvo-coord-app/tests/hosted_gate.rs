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
