use r4_driver_trial::card::Form;
use r4_driver_trial::options::{self, parse_args, Options};
use r4_trial_app::{Arm, Refusal};

fn parse(args: &[&str]) -> Result<Options, Refusal> {
    parse_args(args.iter().copied())
}

fn port_args() -> Vec<&'static str> {
    if options::CANDIDATE.is_some() {
        vec!["--webdriver-port", "47002"]
    } else {
        vec![]
    }
}

#[test]
fn the_candidate_is_the_feature_that_the_build_has() {
    let expected = if cfg!(feature = "webdriver") {
        Some("webdriver")
    } else if cfg!(feature = "wdio") {
        Some("wdio")
    } else {
        None
    };
    assert_eq!(options::CANDIDATE, expected);
}

#[test]
fn no_flags_give_one_show_no_dialog_and_no_ports() {
    let o = parse(&port_args()).unwrap();
    assert_eq!(o.control_port, None);
    assert_eq!(o.dialog, None);
    assert_eq!(o.shows, 1);
    let trial = o.trial();
    assert_eq!(trial.control_port, None);
    assert_eq!(trial.arm, Arm::Main);
    assert_eq!(trial.sync, None);
    assert!(!trial.tray);
    assert_eq!(trial.pages, r4_trial_app::default_pages_dir());
}

#[test]
fn the_dialog_form_shows_and_control_port_are_read() {
    let mut args = port_args();
    args.extend([
        "--dialog",
        "system",
        "--shows",
        "3",
        "--control-port",
        "47001",
    ]);
    let o = parse(&args).unwrap();
    assert_eq!(o.dialog, Some(Form::System));
    assert_eq!(o.shows, 3);
    assert_eq!(o.control_port, Some(47001));
    assert_eq!(o.trial().control_port, Some(47001));
    let mut args = port_args();
    args.extend(["--dialog", "plugin"]);
    assert_eq!(parse(&args).unwrap().dialog, Some(Form::Plugin));
}

#[test]
fn bad_flags_are_usage() {
    let cases: &[&[&str]] = &[
        &["--dialog"],
        &["--dialog", "alert"],
        &["--dialog", "plugin", "--dialog", "plugin"],
        &["--dialog", "plugin", "--shows", "2", "--shows", "2"],
        &["--control-port", "47001", "--control-port", "47001"],
        &["--shows", "2"],
        &["--dialog", "plugin", "--shows", "0"],
        &["--dialog", "plugin", "--shows", "10"],
        &["--dialog", "plugin", "--shows", "x"],
        &["--control-port", "0"],
        &["--control-port", "70000"],
        &["--control-port"],
        &["--arm", "b"],
        &["--webdriver-port", "0"],
        &["--webdriver-port", "x"],
        &["show"],
    ];
    for case in cases {
        let mut args = port_args();
        args.extend_from_slice(case);
        let r = parse(&args);
        assert!(matches!(r, Err(Refusal::Usage(_))), "{args:?} gave {r:?}");
    }
}

#[test]
fn the_webdriver_port_goes_with_a_candidate_build_alone() {
    let given = parse(&["--webdriver-port", "47002"]);
    let missing = parse(&[]);
    if options::CANDIDATE.is_some() {
        assert_eq!(given.unwrap().webdriver_port, Some(47002));
        assert!(matches!(missing, Err(Refusal::Usage(_))), "{missing:?}");
        let twice = parse(&["--webdriver-port", "47002", "--webdriver-port", "47003"]);
        assert!(matches!(twice, Err(Refusal::Usage(_))), "{twice:?}");
    } else {
        assert!(matches!(given, Err(Refusal::Usage(_))), "{given:?}");
        assert_eq!(missing.unwrap().webdriver_port, None);
    }
}
