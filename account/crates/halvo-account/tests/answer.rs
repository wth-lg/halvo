use halvo_account::answer::{read, Fault, Reading, Reply, RouteAnswer, Unread};
use halvo_wire::request::{Route, APP_ROUTES};
use halvo_wire::types::{
    CapGetAnswer, CapGetAnswerResult, CapSetAnswer, CheckoutAnswer, CheckoutAnswerResult,
    DeleteAnswer, ManageAnswer, Period, StatusAnswer, StatusAnswerResult,
};

const SAMPLES: &str = include_str!("../../../schema/samples.v1.json");

fn route(name: &str) -> &'static Route {
    APP_ROUTES
        .iter()
        .find(|route| route.name == name)
        .expect("an app route")
}

fn reply(status: u16, body: &str) -> Reply {
    Reply {
        status,
        body: body.as_bytes().to_vec(),
    }
}

fn read_as<T: RouteAnswer>(name: &str, status: u16, body: &str) -> Result<Reading<T>, Unread> {
    read::<T>(route(name), &reply(status, body))
}

fn malformed<T: RouteAnswer + std::fmt::Debug>(name: &str, body: &str) -> Fault {
    match read_as::<T>(name, 200, body) {
        Err(Unread::Malformed(fault)) => fault,
        Ok(Reading::Answer(answer)) => panic!("{body} was read as {answer:?}"),
        Ok(Reading::Rejected) => panic!("{body} was read as rejected"),
        Err(other) => panic!("{body} gave {other:?}"),
    }
}

fn answer<T: RouteAnswer + std::fmt::Debug>(name: &str, body: &str) -> T {
    match read_as::<T>(name, 200, body) {
        Ok(Reading::Answer(answer)) => answer,
        Ok(Reading::Rejected) => panic!("{body} was read as rejected"),
        Err(unread) => panic!("{body} gave {unread:?}"),
    }
}

#[test]
fn probe_a_null_url_is_no_absent_url() {
    assert_eq!(
        malformed::<CheckoutAnswer>("checkout-start", r#"{"result":"session","url":null}"#),
        Fault::Shape
    );
    assert_eq!(
        malformed::<ManageAnswer>("manage", r#"{"result":"page","url":null}"#),
        Fault::Shape
    );
}

#[test]
fn probe_a_null_cap_and_spend_are_no_absent_cap_and_spend() {
    let body = r#"{"result":"cap","cap":null,"spend":null}"#;
    assert_eq!(malformed::<CapGetAnswer>("cap-get", body), Fault::Shape);
    assert_eq!(malformed::<CapSetAnswer>("cap-set", body), Fault::Shape);
}

#[test]
fn probe_an_answer_written_as_an_array_is_no_object() {
    assert_eq!(
        malformed::<StatusAnswer>("status", r#"["active"]"#),
        Fault::Shape
    );
    assert_eq!(
        malformed::<DeleteAnswer>("account-delete", r#"["deleted"]"#),
        Fault::Shape
    );
    assert_eq!(
        malformed::<CheckoutAnswer>("checkout-start", r#"["session","http://127.0.0.1:1/x"]"#),
        Fault::Shape
    );
    assert_eq!(
        malformed::<CapGetAnswer>("cap-get", r#"[[5,"week"],"cap",[1,2,3]]"#),
        Fault::Shape
    );
}

#[test]
fn probe_a_cap_and_a_spend_written_as_arrays_inside_an_object_are_refused() {
    assert_eq!(
        malformed::<CapGetAnswer>(
            "cap-get",
            r#"{"result":"cap","cap":[5,"week"],"spend":{"spent":0,"held":0,"period_end":0}}"#
        ),
        Fault::Shape
    );
    assert_eq!(
        malformed::<CapSetAnswer>(
            "cap-set",
            r#"{"result":"cap","cap":{"amount":5,"period":"week"},"spend":[1,2,3]}"#
        ),
        Fault::Shape
    );
}

#[test]
fn probe_a_rejection_written_as_an_array_is_no_rejection() {
    for status in [400, 404, 405, 413, 415, 499] {
        assert_eq!(
            read_as::<StatusAnswer>("status", status, r#"["rejected"]"#).err(),
            Some(Unread::Malformed(Fault::Shape)),
            "status {status}"
        );
    }
}

#[test]
fn probe_a_negative_spend_is_below_its_minimum() {
    for body in [
        r#"{"result":"cap","cap":{"amount":5,"period":"week"},"spend":{"spent":-1,"held":0,"period_end":0}}"#,
        r#"{"result":"cap","cap":{"amount":5,"period":"week"},"spend":{"spent":0,"held":-9223372036854775808,"period_end":0}}"#,
        r#"{"result":"cap","cap":{"amount":5,"period":"week"},"spend":{"spent":0,"held":0,"period_end":-1}}"#,
    ] {
        assert_eq!(malformed::<CapGetAnswer>("cap-get", body), Fault::Bound);
        assert_eq!(malformed::<CapSetAnswer>("cap-set", body), Fault::Bound);
    }
}

#[test]
fn probe_numbers_above_their_maximum_are_refused() {
    let spend_above = r#"{"result":"cap","cap":{"amount":5,"period":"week"},"spend":{"spent":9007199254740992,"held":0,"period_end":0}}"#;
    let amount_above = r#"{"result":"cap","cap":{"amount":100000001,"period":"week"},"spend":{"spent":0,"held":0,"period_end":0}}"#;
    let amount_far_above = r#"{"result":"cap","cap":{"amount":18446744073709551615,"period":"week"},"spend":{"spent":0,"held":0,"period_end":0}}"#;
    for body in [spend_above, amount_above, amount_far_above] {
        assert_eq!(malformed::<CapGetAnswer>("cap-get", body), Fault::Bound);
        assert_eq!(malformed::<CapSetAnswer>("cap-set", body), Fault::Bound);
    }
}

#[test]
fn probe_the_largest_values_of_the_schema_are_read() {
    let body = r#"{"result":"cap","cap":{"amount":100000000,"period":"month"},"spend":{"spent":9007199254740991,"held":9007199254740991,"period_end":9007199254740991}}"#;
    let read = answer::<CapGetAnswer>("cap-get", body);
    let cap = read.cap.expect("a cap");
    let spend = read.spend.expect("a spend");
    assert_eq!((cap.amount.get(), cap.period), (100_000_000, Period::Month));
    assert_eq!(
        (spend.spent, spend.held, spend.period_end),
        (9007199254740991, 9007199254740991, 9007199254740991)
    );
}

#[test]
fn probe_an_amount_of_zero_is_below_its_minimum() {
    let body = r#"{"result":"cap","cap":{"amount":0,"period":"week"},"spend":{"spent":0,"held":0,"period_end":0}}"#;
    assert_eq!(malformed::<CapGetAnswer>("cap-get", body), Fault::Bound);
}

#[test]
fn probe_an_integer_with_a_fraction_or_an_exponent_or_a_negative_zero_is_malformed() {
    for amount in [
        "1.0", "1e2", "1E2", "1e0", "5.5", "1.0e1", "-0", "0.0", "-0.0", "1E+0", "100e-2",
    ] {
        let body = format!(
            r#"{{"result":"cap","cap":{{"amount":{amount},"period":"week"}},"spend":{{"spent":0,"held":0,"period_end":0}}}}"#
        );
        assert_eq!(
            malformed::<CapGetAnswer>("cap-get", &body),
            Fault::IntegerForm,
            "{amount}"
        );
    }
    for spent in ["-0", "0.0", "1e2", "2.0", "18446744073709551616"] {
        let body = format!(
            r#"{{"result":"cap","cap":{{"amount":5,"period":"week"}},"spend":{{"spent":{spent},"held":0,"period_end":0}}}}"#
        );
        assert_eq!(
            malformed::<CapSetAnswer>("cap-set", &body),
            Fault::IntegerForm,
            "{spent}"
        );
    }
}

#[test]
fn probe_a_leading_zero_or_a_plus_sign_is_no_json() {
    for amount in ["01", "+1", "00", "0x5", "Infinity", "NaN", "1e400", "-"] {
        let body = format!(
            r#"{{"result":"cap","cap":{{"amount":{amount},"period":"week"}},"spend":{{"spent":0,"held":0,"period_end":0}}}}"#
        );
        assert_eq!(
            malformed::<CapGetAnswer>("cap-get", &body),
            Fault::Json,
            "{amount}"
        );
    }
}

#[test]
fn probe_a_repeated_key_is_refused() {
    assert_eq!(
        malformed::<StatusAnswer>("status", r#"{"result":"active","result":"active"}"#),
        Fault::Shape
    );
    assert_eq!(
        malformed::<CapGetAnswer>(
            "cap-get",
            r#"{"result":"cap","cap":{"amount":5,"amount":6,"period":"week"},"spend":{"spent":0,"held":0,"period_end":0}}"#
        ),
        Fault::Shape
    );
}

#[test]
fn probe_a_url_of_2048_characters_is_read_and_one_of_2049_is_refused() {
    let wide = "\u{1F600}".repeat(2048);
    let read = answer::<CheckoutAnswer>(
        "checkout-start",
        &format!(r#"{{"result":"session","url":"{wide}"}}"#),
    );
    assert_eq!(read.url.expect("a url").chars().count(), 2048);
    let long = "a".repeat(2049);
    assert_eq!(
        malformed::<CheckoutAnswer>(
            "checkout-start",
            &format!(r#"{{"result":"session","url":"{long}"}}"#)
        ),
        Fault::Bound
    );
}

#[test]
fn probe_a_lone_surrogate_is_no_json() {
    assert_eq!(
        malformed::<CheckoutAnswer>("checkout-start", r#"{"result":"session","url":"\ud800"}"#),
        Fault::Json
    );
}

#[test]
fn probe_an_unknown_result_a_missing_result_and_a_missing_period_are_refused() {
    assert_eq!(
        malformed::<StatusAnswer>("status", r#"{"result":"trial"}"#),
        Fault::Shape
    );
    assert_eq!(malformed::<StatusAnswer>("status", "{}"), Fault::Shape);
    assert_eq!(
        malformed::<CapGetAnswer>(
            "cap-get",
            r#"{"result":"cap","cap":{"amount":5},"spend":{"spent":0,"held":0,"period_end":0}}"#
        ),
        Fault::Shape
    );
}

#[test]
fn a_value_of_another_type_is_refused() {
    for body in [
        r#"{"result":true}"#,
        r#"{"result":1}"#,
        r#"{"result":{"x":"active"}}"#,
        r#"{"result":["active"]}"#,
        r#""active""#,
        "1",
        "null",
        "true",
    ] {
        assert_eq!(
            malformed::<StatusAnswer>("status", body),
            Fault::Shape,
            "{body}"
        );
    }
    for amount in [r#""5""#, "true", "null", "{}", "[]"] {
        let body = format!(
            r#"{{"result":"cap","cap":{{"amount":{amount},"period":"week"}},"spend":{{"spent":0,"held":0,"period_end":0}}}}"#
        );
        assert_eq!(
            malformed::<CapGetAnswer>("cap-get", &body),
            Fault::Shape,
            "{amount}"
        );
    }
}

#[test]
fn a_key_outside_the_schema_is_refused() {
    assert_eq!(
        malformed::<StatusAnswer>("status", r#"{"result":"active","tier":"frontier"}"#),
        Fault::Shape
    );
    assert_eq!(
        malformed::<CapGetAnswer>(
            "cap-get",
            r#"{"result":"cap","cap":{"amount":5,"period":"week","currency":"usd"},"spend":{"spent":0,"held":0,"period_end":0}}"#
        ),
        Fault::Shape
    );
}

#[test]
fn an_answer_whose_keys_differ_from_its_results_is_refused() {
    assert_eq!(
        malformed::<CheckoutAnswer>(
            "checkout-start",
            r#"{"result":"unavailable","url":"http://127.0.0.1:1/x"}"#
        ),
        Fault::Results
    );
    assert_eq!(
        malformed::<CheckoutAnswer>("checkout-start", r#"{"result":"session"}"#),
        Fault::Results
    );
    assert_eq!(
        malformed::<ManageAnswer>("manage", r#"{"result":"page"}"#),
        Fault::Results
    );
    assert_eq!(
        malformed::<CapGetAnswer>("cap-get", r#"{"result":"cap"}"#),
        Fault::Results
    );
    assert_eq!(
        malformed::<CapGetAnswer>(
            "cap-get",
            r#"{"result":"cap","cap":{"amount":5,"period":"week"}}"#
        ),
        Fault::Results
    );
    assert_eq!(
        malformed::<CapGetAnswer>(
            "cap-get",
            r#"{"result":"no-cap","cap":{"amount":5,"period":"week"}}"#
        ),
        Fault::Results
    );
    assert_eq!(
        malformed::<CapSetAnswer>(
            "cap-set",
            r#"{"result":"pending","spend":{"spent":0,"held":0,"period_end":0}}"#
        ),
        Fault::Results
    );
}

#[test]
fn a_body_that_is_no_json_text_is_refused() {
    for body in [
        "",
        " ",
        "{",
        r#"{"result":"active"} x"#,
        r#"{"result":"active"}{}"#,
        r#"{'result':'active'}"#,
        r#"{"result":"active",}"#,
    ] {
        assert_eq!(
            malformed::<StatusAnswer>("status", body),
            Fault::Json,
            "{body:?}"
        );
    }
    let not_utf8 = Reply {
        status: 200,
        body: vec![b'{', b'"', 0xff, b'"', b':', b'1', b'}'],
    };
    assert_eq!(
        read::<StatusAnswer>(route("status"), &not_utf8).err(),
        Some(Unread::Malformed(Fault::Json))
    );
}

#[test]
fn white_space_around_and_inside_an_answer_is_read() {
    let read = answer::<StatusAnswer>("status", " {\n \"result\" : \"cancelled\" }\n");
    assert_eq!(read.result, StatusAnswerResult::Cancelled);
}

#[test]
fn every_answer_of_the_vendored_samples_is_read_with_its_result() {
    let samples: serde_json::Value = serde_json::from_str(SAMPLES).expect("the samples");
    let answers = samples["answers"].as_object().expect("answers");
    let mut read_count = 0;
    for (name, by_result) in answers {
        for (result, sample) in by_result.as_object().expect("results") {
            let body = sample.to_string();
            let got = match name.as_str() {
                "checkout-start" => answer::<CheckoutAnswer>(name, &body).result.to_string(),
                "status" => answer::<StatusAnswer>(name, &body).result.to_string(),
                "cap-get" => answer::<CapGetAnswer>(name, &body).result.to_string(),
                "cap-set" => answer::<CapSetAnswer>(name, &body).result.to_string(),
                "manage" => answer::<ManageAnswer>(name, &body).result.to_string(),
                "account-delete" => answer::<DeleteAnswer>(name, &body).result.to_string(),
                other => panic!("no route {other}"),
            };
            assert_eq!(&got, result);
            read_count += 1;
        }
    }
    assert_eq!(read_count, 29);
    let read = answer::<CheckoutAnswer>(
        "checkout-start",
        &answers["checkout-start"]["session"].to_string(),
    );
    assert_eq!(read.result, CheckoutAnswerResult::Session);
    assert_eq!(read.url.expect("a url").as_str(), "http://127.0.0.1:1/x");
    let read = answer::<CapGetAnswer>("cap-get", &answers["cap-get"]["cap"].to_string());
    assert_eq!(read.result, CapGetAnswerResult::Cap);
}

#[test]
fn a_rejection_of_status_four_hundred_to_four_hundred_ninety_nine_reads_as_rejected() {
    for status in [400, 401, 404, 405, 413, 415, 429, 499] {
        assert!(
            matches!(
                read_as::<StatusAnswer>("status", status, r#"{"result":"rejected"}"#),
                Ok(Reading::Rejected)
            ),
            "status {status}"
        );
    }
    for body in [
        "",
        r#"{"result":"rejected","why":"x"}"#,
        r#"{"result":"active"}"#,
        r#"{"result":null}"#,
    ] {
        assert_eq!(
            read_as::<CapGetAnswer>("cap-get", 400, body).err(),
            Some(Unread::Malformed(match body {
                "" => Fault::Json,
                _ => Fault::Shape,
            })),
            "{body:?}"
        );
    }
}

#[test]
fn an_answer_of_any_other_status_is_not_read() {
    for status in [
        0, 100, 199, 201, 204, 299, 301, 302, 304, 399, 500, 501, 502, 503, 599,
    ] {
        assert_eq!(
            read_as::<StatusAnswer>("status", status, r#"{"result":"active"}"#).err(),
            Some(Unread::Status(status)),
            "status {status}"
        );
    }
}

#[test]
fn an_answer_type_read_for_another_route_is_refused() {
    assert_eq!(
        read_as::<StatusAnswer>("cap-get", 200, r#"{"result":"pending"}"#).err(),
        Some(Unread::Malformed(Fault::Mapping))
    );
    assert_eq!(
        read_as::<DeleteAnswer>("manage", 200, r#"{"result":"pending"}"#).err(),
        Some(Unread::Malformed(Fault::Mapping))
    );
}

#[test]
fn each_answer_type_names_its_definition_in_the_vendored_schema() {
    let pairs = [
        (CheckoutAnswer::DEF, "checkout-start"),
        (StatusAnswer::DEF, "status"),
        (CapGetAnswer::DEF, "cap-get"),
        (CapSetAnswer::DEF, "cap-set"),
        (ManageAnswer::DEF, "manage"),
        (DeleteAnswer::DEF, "account-delete"),
    ];
    for (def, name) in pairs {
        assert_eq!(def, route(name).answer);
    }
}

#[test]
fn a_deeply_nested_body_is_refused_without_a_crash() {
    let body = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    assert_eq!(malformed::<StatusAnswer>("status", &body), Fault::Json);
}
