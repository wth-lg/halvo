use halvo_account::grammar::{parse, Build, Command, Invocation, Show, Usage, THIS_BUILD};
use halvo_wire::types::Period;
use halvo_wire::{CAP_AMOUNT_MAX, CAP_AMOUNT_MIN};
use std::ffi::OsString;

fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

fn parsed(build: Build, words: &[&str]) -> Invocation {
    match parse(&args(words), build) {
        Ok(invocation) => invocation,
        Err(usage) => panic!("{words:?} was refused as {usage:?}"),
    }
}

fn refused(build: Build, words: &[&str]) -> Usage {
    match parse(&args(words), build) {
        Ok(invocation) => panic!("{words:?} was taken as {invocation:?}"),
        Err(usage) => usage,
    }
}

fn cap_of(command: &Command) -> (u64, Period) {
    match command {
        Command::Subscribe(cap) | Command::SetCap(cap) => (cap.amount.get(), cap.period),
        other => panic!("{other:?} carries no cap"),
    }
}

const BOTH: [Build; 2] = [Build::Debug, Build::Release];

#[test]
fn each_command_without_an_operand_is_read_as_itself() {
    for build in BOTH {
        assert!(matches!(
            parsed(build, &["status"]).command,
            Command::Status
        ));
        assert!(matches!(parsed(build, &["cap"]).command, Command::Cap));
        assert!(matches!(
            parsed(build, &["manage"]).command,
            Command::Manage
        ));
        assert!(matches!(
            parsed(build, &["delete"]).command,
            Command::Delete
        ));
        assert_eq!(parsed(build, &["status"]).show, Show::Open);
    }
}

#[test]
fn subscribe_and_set_cap_carry_the_amount_and_the_period_as_given() {
    for build in BOTH {
        let subscribe = parsed(build, &["subscribe", "5", "week"]);
        assert!(matches!(subscribe.command, Command::Subscribe(_)));
        assert_eq!(cap_of(&subscribe.command), (5, Period::Week));
        let set_cap = parsed(build, &["set-cap", "12345", "month"]);
        assert!(matches!(set_cap.command, Command::SetCap(_)));
        assert_eq!(cap_of(&set_cap.command), (12345, Period::Month));
    }
}

#[test]
fn the_amount_spans_the_bounds_of_the_vendored_schema_and_no_more() {
    let below = (CAP_AMOUNT_MIN - 1).to_string();
    let above = (CAP_AMOUNT_MAX + 1).to_string();
    for build in BOTH {
        for (amount, period) in [(CAP_AMOUNT_MIN, "week"), (CAP_AMOUNT_MAX, "month")] {
            let text = amount.to_string();
            let invocation = parsed(build, &["set-cap", &text, period]);
            assert_eq!(cap_of(&invocation.command).0, amount);
        }
        assert_eq!(
            refused(build, &["subscribe", &below, "week"]),
            Usage::Amount
        );
        assert_eq!(refused(build, &["set-cap", &above, "week"]), Usage::Amount);
    }
}

#[test]
fn an_amount_is_plain_decimal_digits_without_a_sign_a_leading_zero_or_a_space() {
    let forms = [
        "",
        "0",
        "00",
        "01",
        "007",
        "+1",
        "-1",
        "1.0",
        "1.",
        ".5",
        "1e2",
        "1E2",
        "0x10",
        " 1",
        "1 ",
        "1\n",
        "1_000",
        "1,000",
        "١",
        "１",
        "²",
        "99999999999999999999999",
        "18446744073709551616",
        "five",
    ];
    for build in BOTH {
        for form in forms {
            assert_eq!(
                refused(build, &["subscribe", form, "week"]),
                Usage::Amount,
                "{form:?}"
            );
            assert_eq!(
                refused(build, &["set-cap", form, "month"]),
                Usage::Amount,
                "{form:?}"
            );
        }
    }
}

#[test]
fn the_period_is_week_or_month_spelled_so() {
    for build in BOTH {
        for form in [
            "", "Week", "WEEK", "weeks", "weekly", "month ", " month", "Month", "day", "year", "w",
            "1",
        ] {
            assert_eq!(
                refused(build, &["subscribe", "1", form]),
                Usage::Period,
                "{form:?}"
            );
            assert_eq!(
                refused(build, &["set-cap", "1", form]),
                Usage::Period,
                "{form:?}"
            );
        }
    }
}

#[test]
fn a_command_name_in_another_spelling_or_outside_the_grammar_is_refused() {
    for build in BOTH {
        for name in [
            "Status",
            "STATUS",
            "status ",
            " status",
            "statu",
            "statuses",
            "help",
            "--help",
            "-h",
            "--version",
            "version",
            "create",
            "sign-in",
            "login",
            "open",
            "url",
            "set_cap",
            "setcap",
            "Set-Cap",
            "subscribe=1",
            "--status",
            "-",
            "--",
            "",
        ] {
            assert_eq!(refused(build, &[name]), Usage::Unknown, "{name:?}");
        }
    }
}

#[test]
fn a_command_takes_no_operand_beyond_its_own() {
    for build in BOTH {
        for words in [
            &["status", "x"][..],
            &["cap", "1"],
            &["manage", "--print-url"],
            &["delete", "now"],
            &["subscribe", "1", "week", "x"],
            &["set-cap", "1", "month", "1"],
            &["status", "--"],
        ] {
            assert_eq!(refused(build, words), Usage::Extra, "{words:?}");
        }
        for words in [
            &["subscribe"][..],
            &["subscribe", "1"],
            &["set-cap"],
            &["set-cap", "7"],
        ] {
            assert_eq!(refused(build, words), Usage::Missing, "{words:?}");
        }
    }
}

#[test]
fn no_arguments_name_no_command() {
    for build in BOTH {
        assert_eq!(refused(build, &[]), Usage::NoCommand);
    }
    assert_eq!(refused(Build::Debug, &["--print-url"]), Usage::NoCommand);
}

#[test]
fn a_debug_build_takes_print_url_before_the_command_alone() {
    for words in [
        &["--print-url", "status"][..],
        &["--print-url", "subscribe", "1", "week"],
        &["--print-url", "set-cap", "2", "month"],
        &["--print-url", "cap"],
        &["--print-url", "manage"],
        &["--print-url", "delete"],
    ] {
        assert_eq!(parsed(Build::Debug, words).show, Show::Print, "{words:?}");
    }
    assert_eq!(
        refused(Build::Debug, &["status", "--print-url"]),
        Usage::Extra
    );
    assert_eq!(
        refused(Build::Debug, &["subscribe", "1", "week", "--print-url"]),
        Usage::Extra
    );
    assert_eq!(
        refused(Build::Debug, &["subscribe", "--print-url", "1", "week"]),
        Usage::Extra
    );
    assert_eq!(
        refused(Build::Debug, &["--print-url", "--print-url", "status"]),
        Usage::Unknown
    );
    for flag in [
        "--print-url=1",
        "--print-urls",
        "--PRINT-URL",
        "-print-url",
        "--print_url",
    ] {
        assert_eq!(
            refused(Build::Debug, &[flag, "status"]),
            Usage::Unknown,
            "{flag:?}"
        );
    }
}

#[test]
fn a_release_build_refuses_print_url_as_an_unknown_argument() {
    for words in [
        &["--print-url", "status"][..],
        &["--print-url", "subscribe", "1", "week"],
        &["--print-url", "manage"],
        &["--print-url"],
    ] {
        assert_eq!(refused(Build::Release, words), Usage::Unknown, "{words:?}");
    }
    assert_eq!(
        refused(Build::Release, &["manage", "--print-url"]),
        Usage::Extra
    );
}

#[test]
fn this_build_is_the_debug_build_exactly_when_debug_assertions_are_on() {
    let expected = if cfg!(debug_assertions) {
        Build::Debug
    } else {
        Build::Release
    };
    assert_eq!(THIS_BUILD, expected);
}

#[cfg(unix)]
#[test]
fn an_argument_that_is_not_unicode_is_refused_wherever_it_stands() {
    use std::os::unix::ffi::OsStringExt;
    let bad = OsString::from_vec(vec![b's', 0xff]);
    for build in BOTH {
        assert_eq!(
            parse(std::slice::from_ref(&bad), build).err(),
            Some(Usage::NotText)
        );
        let in_place_of_amount = [
            OsString::from("subscribe"),
            bad.clone(),
            OsString::from("week"),
        ];
        assert_eq!(
            parse(&in_place_of_amount, build).err(),
            Some(Usage::NotText)
        );
        let after = [OsString::from("status"), bad.clone()];
        assert_eq!(parse(&after, build).err(), Some(Usage::NotText));
    }
}

#[cfg(windows)]
#[test]
fn an_argument_that_is_not_unicode_is_refused_wherever_it_stands() {
    use std::os::windows::ffi::OsStringExt;
    let bad = OsString::from_wide(&[u16::from(b's'), 0xd800]);
    for build in BOTH {
        assert_eq!(
            parse(std::slice::from_ref(&bad), build).err(),
            Some(Usage::NotText)
        );
        let in_place_of_amount = [
            OsString::from("subscribe"),
            bad.clone(),
            OsString::from("week"),
        ];
        assert_eq!(
            parse(&in_place_of_amount, build).err(),
            Some(Usage::NotText)
        );
        let after = [OsString::from("status"), bad.clone()];
        assert_eq!(parse(&after, build).err(), Some(Usage::NotText));
    }
}
