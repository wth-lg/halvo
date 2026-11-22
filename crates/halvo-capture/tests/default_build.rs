//! A build of halvo-capture that names no feature holds the code of the workflows run: this file has no `cfg` line
//! and compiles only where the feature `r2` is a default.

#[test]
fn a_build_with_no_feature_flag_holds_the_run_mark_kind() {
    assert_eq!(
        halvo_capture::daemon::port::EntryKind::RUN_MARK.as_str(),
        "run_mark"
    );
}
