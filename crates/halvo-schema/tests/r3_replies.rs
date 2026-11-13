use halvo_schema::executable::names::{Cause, LogEvent};
use halvo_schema::executable::record::{AgentEffect, AgentLog};
use halvo_schema::executable::{Refusal, RefusalCause};

const W: &str = "30000000000000000000000000000001";
const A: &str = "40000000000000000000000000000001";
const R: &str = "50000000000000000000000000000001";
const E: &str = "60000000000000000000000000000001";

fn of_run(rest: &str) -> String {
    format!(r#"{{"v":1,"workflow":"{W}","agent":"{A}","run":"{R}",{rest}}}"#)
}

fn swap(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from}");
    text.replacen(from, to, 1)
}

fn expect(result: Result<impl std::fmt::Debug, Refusal>, type_name: &str, rule: &str, key: &str) {
    let refusal = result.expect_err("refused");
    assert_eq!(refusal.cause(), RefusalCause::Malformed, "{refusal}");
    assert_eq!(
        (
            refusal.type_name(),
            refusal.rule(),
            refusal.key().unwrap_or("-")
        ),
        (type_name, rule, key)
    );
}

fn line(seq: u32, event: &str, cause: Option<&str>) -> String {
    let cause = cause.map_or("null".to_owned(), |c| format!("\"{c}\""));
    of_run(&format!(
        r#""seq":{seq},"day":20768,"at":1794339000000000000,"event":"{event}","step":null,"cause":{cause},"model":null,"ms":null,"tokens_in":null,"tokens_out":null"#
    ))
}

#[test]
fn the_cause_list_holds_unauthorized_not_selected_and_no_start_in_the_order_of_the_contract() {
    let names: Vec<&str> = Cause::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        names,
        [
            "approval",
            "digest",
            "used",
            "unknown",
            "autonomy",
            "checkpoint",
            "class",
            "class_mismatch",
            "origin",
            "provenance",
            "declined",
            "timeout",
            "session_size",
            "step_failed",
            "steps",
            "wall",
            "tokens",
            "sheet",
            "workbook",
            "adapter",
            "effect",
            "ended",
            "changed",
            "secret",
            "unauthorized",
            "not_selected",
            "no_start",
        ]
    );
    for cause in Cause::ALL {
        assert_eq!(Cause::parse(cause.as_str()), Ok(*cause));
    }
}

#[test]
fn a_start_accepted_line_is_line_1_with_no_step_cause_model_or_figures() {
    assert_eq!(LogEvent::ALL[0].as_str(), "start_accepted");
    assert_eq!(
        LogEvent::parse("start_accepted"),
        Ok(LogEvent::StartAccepted)
    );
    let good = line(1, "start_accepted", None);
    let read = AgentLog::parse(good.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.event, LogEvent::StartAccepted);
    assert_eq!(read.to_json().unwrap(), good);
    expect(
        AgentLog::parse(swap(&good, r#""seq":1,"#, r#""seq":2,"#).as_bytes()),
        "AgentLog",
        "start_accepted_first",
        "seq",
    );
    for (key, from, to) in [
        ("step", r#""step":null"#, r#""step":1"#),
        ("cause", r#""cause":null"#, r#""cause":"secret""#),
        ("model", r#""model":null"#, r#""model":"qwen3_1_7b""#),
        ("ms", r#""ms":null"#, r#""ms":0"#),
        ("tokens_in", r#""tokens_in":null"#, r#""tokens_in":0"#),
        ("tokens_out", r#""tokens_out":null"#, r#""tokens_out":0"#),
    ] {
        expect(
            AgentLog::parse(swap(&good, from, to).as_bytes()),
            "AgentLog",
            "start_accepted_form",
            key,
        );
    }
}

#[test]
fn unauthorized_stands_in_a_step_failed_line_and_not_selected_and_no_start_in_a_stopped_line() {
    for (cause, event) in [
        ("unauthorized", "step_failed"),
        ("not_selected", "stopped"),
        ("no_start", "stopped"),
    ] {
        for other in LogEvent::ALL {
            if *other == LogEvent::StartAccepted {
                continue;
            }
            let text = line(2, other.as_str(), Some(cause));
            if other.as_str() == event {
                let read = AgentLog::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(read.cause.map(|c| c.as_str()), Some(cause));
                assert_eq!(read.to_json().unwrap(), text);
            } else {
                expect(
                    AgentLog::parse(text.as_bytes()),
                    "AgentLog",
                    "cause_of_event",
                    "cause",
                );
            }
        }
    }
    for other in ["secret", "session_size", "adapter"] {
        assert!(AgentLog::parse(line(2, "step_ok", Some(other)).as_bytes()).is_ok());
    }
}

const PAY_KEPT: &str = r#"[{"name":"payee","value":["Northwind Power"]},{"name":"amount","value":["84.20"]},{"name":"instrument","value":["card 4242"]},{"name":"origin","value":["http://pay.test:18090"]}]"#;

fn effect_row(state: &str, approval: Option<&str>, cause: &str) -> String {
    let approval = approval.map_or("null".to_owned(), |a| format!("\"{a}\""));
    of_run(&format!(
        r#""effect":"{E}","seq":2,"day":20768,"at":1794339000000000000,"state":"{state}","class":"payment","digest":"{}","kept":{PAY_KEPT},"fields":4,"approval":{approval},"cause":{cause}"#,
        "a".repeat(64)
    ))
}

#[test]
fn no_effect_row_holds_unauthorized_not_selected_or_no_start_while_the_states_have_no_not_made() {
    let approval = "70000000000000000000000000000001";
    for (state, approval) in [
        ("held", None),
        ("rejected", None),
        ("releasing", Some(approval)),
        ("unknown", Some(approval)),
    ] {
        assert!(AgentEffect::parse(effect_row(state, approval, "null").as_bytes()).is_ok());
        for cause in ["unauthorized", "not_selected", "no_start"] {
            expect(
                AgentEffect::parse(effect_row(state, approval, &format!("\"{cause}\"")).as_bytes()),
                "AgentEffect",
                "cause_of_state",
                "cause",
            );
        }
    }
}
