mod common;

use common::{assert_strict, read, refused, round_trip, with, MAX63};
use halvo_schema::{
    AddressFieldLayer, Confidence, DismissedWorkflow, Episode, Evidence, EvidencePage, FloorLayer,
    FullWorkflow, InputClass, Instant, KeyCounts, ObservedStep, Ordinal, Part, PointerCounts,
    Recurrence, Summary, Support, Tagged, Text, TextLayer, Trigger, Workflow, WorkflowItem,
    WorkflowState, WorkflowsPage,
};

const HASH: &str = "9f2c4b7a1d3e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5061728394a5b6c7d8e";
const PROPOSED: &str = r#"{"v":1,"id":"5f1c0a9e2b7d4c36a8e0f4b1d2c3a495","state":"proposed","category":"utility_bill_payment","site":{"key":"northwind-power.test","source":"address_field"},"app":"com.google.Chrome","trigger":"schedule","recurrence":{"count":3,"period":31,"first":20705,"last":20767},"confidence":87,"agent":null}"#;
const APPROVED: &str = r#"{"v":1,"id":"6a1c0a9e2b7d4c36a8e0f4b1d2c3a495","state":"approved","category":"email","site":null,"app":"com.apple.mail","trigger":"manual","recurrence":{"count":1,"period":null,"first":20767,"last":20767},"confidence":0,"agent":"aa1c0a9e2b7d4c36a8e0f4b1d2c3a495"}"#;
const SUMMARY: &str = r#"{"v":1,"id":"c41d7e90a2b34f658791a0b2c3d4e5f6","category":"utility_bill_payment","site":{"key":"northwind-power.test","source":"address_field"},"app":"com.google.Chrome","day":20767,"part":"evening","minutes":5,"keys":{"printable":42,"editing":3,"navigation":0,"enter":2,"tab":4,"shortcut":0,"other":0},"pointer":null}"#;
const SUPPORT: &str = r#"{"v":1,"workflow":"5f1c0a9e2b7d4c36a8e0f4b1d2c3a495","summary":"c41d7e90a2b34f658791a0b2c3d4e5f6","day":20767}"#;
const STEP: &str = r#"{"app":{"value":"com.google.Chrome","layer":"floor"},"title":{"value":"Pay bill - Northwind Power","layer":"floor"},"input":{"value":"click","layer":"floor"},"site":null,"text":null}"#;
const FULL_STEP: &str = r#"{"app":{"value":"chrome.exe","layer":"floor"},"title":{"value":"Pay bill","layer":"floor"},"input":{"value":"move","layer":"floor"},"site":{"value":"northwind-power.test","layer":"address_field"},"text":{"value":"Amount due","layer":"accessibility_text"}}"#;
const NULL_STEP: &str = r#"{"app":null,"title":null,"input":null,"site":null,"text":null}"#;

fn dismissed() -> String {
    format!(
        r#"{{"v":1,"id":"0b9d6e3fa1c24857b6e9d0c1a2f3e4d5","state":"dismissed","category":"shop_order","site":{{"key":"{HASH}","source":"title"}},"app":"chrome.exe"}}"#
    )
}

fn evidence() -> String {
    format!(
        r#"{{"v":1,"summary":"c41d7e90a2b34f658791a0b2c3d4e5f6","segments":[4410,4411],"expires":1795478400000000000,"start":1794339000000000000,"end":1794339135000000000,"hint":"northwind-power","steps":[{STEP}]}}"#
    )
}

fn episode() -> String {
    format!(
        r#"{{"id":"c41d7e90a2b34f658791a0b2c3d4e5f6","category":"utility_bill_payment","site":{{"token":"northwind-power","source":"title"}},"app":"com.google.Chrome","day":20767,"part":"evening","minutes":5,"keys":null,"pointer":null,"segments":[4410,4411],"expires":1795478400000000000,"start":1794339000000000000,"end":1794339135000000000,"steps":[{STEP}]}}"#
    )
}

fn assert_write_refused<T: serde::Serialize>(value: &T) {
    assert!(
        serde_json::to_string(value).is_err(),
        "written although its reader refuses it"
    );
}

#[test]
fn the_records_of_the_contract_read_and_write_back_to_the_same_bytes() {
    assert_strict::<FullWorkflow>(PROPOSED);
    assert_strict::<FullWorkflow>(APPROVED);
    assert_strict::<DismissedWorkflow>(&dismissed());
    assert_strict::<Workflow>(PROPOSED);
    assert_strict::<Workflow>(&dismissed());
    assert_strict::<Summary>(SUMMARY);
    assert_strict::<Summary>(&with(&with(SUMMARY, "keys", "null"), "site", "null"));
    assert_strict::<Support>(SUPPORT);
    assert_strict::<Evidence>(&evidence());
    assert_strict::<Episode>(&episode());
    assert_strict::<ObservedStep>(STEP);
    assert_strict::<ObservedStep>(FULL_STEP);
    assert_strict::<ObservedStep>(NULL_STEP);
    assert_strict::<Recurrence>(r#"{"count":3,"period":31,"first":20705,"last":20767}"#);
    assert_strict::<KeyCounts>(
        r#"{"printable":1,"editing":2,"navigation":3,"enter":4,"tab":5,"shortcut":6,"other":7}"#,
    );
    assert_strict::<PointerCounts>(r#"{"click":1,"scroll":2,"drag":3,"move":4294967295}"#);
}

#[test]
fn every_record_carries_version_1_first() {
    for line in [PROPOSED, SUMMARY, SUPPORT] {
        assert!(line.starts_with(r#"{"v":1,"#));
        for v in [
            "2",
            "0",
            "\"1\"",
            "1.0",
            "null",
            "-1",
            "18446744073709551617",
        ] {
            refused::<Workflow>(&with(line, "v", v));
            refused::<Summary>(&with(line, "v", v));
            refused::<Support>(&with(line, "v", v));
        }
    }
    refused::<Evidence>(&with(&evidence(), "v", "2"));
    refused::<Workflow>(&with(&dismissed(), "v", "2"));
}

#[test]
fn a_workflow_has_a_full_shape_and_a_dismissed_shape() {
    let full = match read::<Workflow>(PROPOSED) {
        Workflow::Full(w) => w,
        other => panic!("{other:?}"),
    };
    assert_eq!(full.state, WorkflowState::Proposed);
    assert_eq!(full.trigger, Trigger::Schedule);
    assert_eq!(full.confidence, Confidence(87));
    assert_eq!(full.agent, None);
    assert_eq!(
        full.recurrence,
        Recurrence {
            count: 3,
            period: Some(31),
            first: 20705,
            last: 20767
        }
    );
    match read::<Workflow>(&dismissed()) {
        Workflow::Dismissed(d) => assert_eq!(d.id.as_str(), "0b9d6e3fa1c24857b6e9d0c1a2f3e4d5"),
        other => panic!("{other:?}"),
    }
    let w = read::<Workflow>(APPROVED);
    assert_eq!(w.id().as_str(), "6a1c0a9e2b7d4c36a8e0f4b1d2c3a495");
    for state in ["proposed", "approved", "active", "paused"] {
        let line = with(PROPOSED, "state", &format!("\"{state}\""));
        assert_eq!(
            serde_json::to_string(&read::<Workflow>(&line)).unwrap(),
            line
        );
        for agent in ["null", "\"aa1c0a9e2b7d4c36a8e0f4b1d2c3a495\""] {
            round_trip::<Workflow>(&with(&line, "agent", agent));
        }
    }
    for state in ["dismissed", "deleted", "Proposed", "null"] {
        refused::<FullWorkflow>(&with(PROPOSED, "state", &format!("\"{state}\"")));
        refused::<Workflow>(&with(PROPOSED, "state", &format!("\"{state}\"")));
    }
    refused::<Workflow>(&with(&dismissed(), "state", "\"proposed\""));
    refused::<DismissedWorkflow>(&with(&dismissed(), "state", "\"proposed\""));
    let mut grown = common::entries(&dismissed());
    grown.push(("trigger".to_owned(), common::raw("\"manual\"")));
    refused::<Workflow>(&common::object(&grown));
    refused::<Workflow>(r#"{"v":1}"#);
    refused::<FullWorkflow>(&with(PROPOSED, "category", "\"other\""));
    refused::<FullWorkflow>(&with(PROPOSED, "trigger", "\"hourly\""));
    refused::<FullWorkflow>(&with(
        PROPOSED,
        "agent",
        "\"5F1C0A9E2B7D4C36A8E0F4B1D2C3A495\"",
    ));
    refused::<FullWorkflow>(&with(PROPOSED, "app", "\"/Applications/Chrome.app\""));
    refused::<FullWorkflow>(&with(
        PROPOSED,
        "site",
        r#"{"token":"northwind-power","source":"title"}"#,
    ));
}

#[test]
fn recurrence_and_confidence_keep_their_ranges() {
    let base = r#"{"count":3,"period":31,"first":20705,"last":20767}"#;
    for (key, value) in [
        ("count", "0"),
        ("count", "4294967296"),
        ("period", "0"),
        ("period", "94"),
        ("period", "-1"),
        ("first", "20768"),
        ("last", "20704"),
        ("first", "1.5"),
    ] {
        refused::<Recurrence>(&with(base, key, value));
    }
    for (key, value) in [
        ("period", "1"),
        ("period", "93"),
        ("period", "null"),
        ("first", "20767"),
        ("count", "4294967295"),
    ] {
        round_trip::<Recurrence>(&with(base, key, value));
    }
    for bad in [
        Recurrence {
            count: 0,
            period: None,
            first: 1,
            last: 1,
        },
        Recurrence {
            count: 1,
            period: Some(0),
            first: 1,
            last: 1,
        },
        Recurrence {
            count: 1,
            period: Some(94),
            first: 1,
            last: 1,
        },
        Recurrence {
            count: 1,
            period: None,
            first: 2,
            last: 1,
        },
    ] {
        assert_write_refused(&bad);
    }
    round_trip::<FullWorkflow>(&with(PROPOSED, "confidence", "100"));
    refused::<FullWorkflow>(&with(PROPOSED, "confidence", "101"));
    refused::<Confidence>("101");
    refused::<Confidence>("-1");
    assert_eq!(read::<Confidence>("0"), Confidence(0));
    assert_write_refused(&Confidence(101));
    let mut w = read::<FullWorkflow>(PROPOSED);
    w.confidence = Confidence(101);
    assert_write_refused(&w);
    let mut w = read::<FullWorkflow>(PROPOSED);
    w.recurrence.first = w.recurrence.last + 1;
    assert_write_refused(&Workflow::Full(w));
}

#[test]
fn a_summary_counts_minutes_in_steps_of_5_and_its_counts_in_u32() {
    for minutes in ["5", "10", "1440"] {
        round_trip::<Summary>(&with(SUMMARY, "minutes", minutes));
    }
    for minutes in ["0", "3", "7", "1445", "-5", "5.0"] {
        refused::<Summary>(&with(SUMMARY, "minutes", minutes));
    }
    for part in ["night", "morning", "afternoon", "evening"] {
        round_trip::<Summary>(&with(SUMMARY, "part", &format!("\"{part}\"")));
    }
    refused::<Summary>(&with(SUMMARY, "part", "\"noon\""));
    refused::<Summary>(&with(SUMMARY, "category", "\"other\""));
    refused::<Summary>(&with(SUMMARY, "day", "4294967296"));
    refused::<Summary>(&with(SUMMARY, "day", "-1"));
    refused::<Summary>(&with(
        SUMMARY,
        "site",
        r#"{"token":"northwind-power","source":"title"}"#,
    ));
    round_trip::<Summary>(&with(SUMMARY, "day", "4294967295"));
    round_trip::<Summary>(&with(
        SUMMARY,
        "pointer",
        r#"{"click":0,"scroll":4294967295,"drag":1,"move":2}"#,
    ));
    refused::<Summary>(&with(
        SUMMARY,
        "pointer",
        r#"{"click":0,"scroll":4294967296,"drag":1,"move":2}"#,
    ));
    refused::<Summary>(&with(SUMMARY, "keys", r#"{"printable":1}"#));
    refused::<Summary>(&with(SUMMARY, "keys", "{}"));
    let mut s = read::<Summary>(SUMMARY);
    s.minutes = 7;
    assert_write_refused(&s);
    s.minutes = 1445;
    assert_write_refused(&s);
    s.minutes = 1440;
    assert!(serde_json::to_string(&s).is_ok());
    let p = read::<PointerCounts>(r#"{"click":1,"scroll":2,"drag":3,"move":4}"#);
    assert_eq!(p.r#move, 4);
}

#[test]
fn evidence_keeps_its_segments_ascending_and_its_range_half_open() {
    let line = evidence();
    for segments in [
        "[]",
        "[4411,4410]",
        "[4410,4410]",
        "[9223372036854775808]",
        "[-1]",
        "null",
    ] {
        refused::<Evidence>(&with(&line, "segments", segments));
    }
    for segments in ["[0]", "[9223372036854775807]", "[1,2,3]"] {
        round_trip::<Evidence>(&with(&line, "segments", segments));
    }
    refused::<Evidence>(&with(&line, "end", "1794339000000000000"));
    refused::<Evidence>(&with(&line, "end", "1794338999999999999"));
    refused::<Evidence>(&with(&line, "expires", "1.7e18"));
    refused::<Evidence>(&with(&line, "expires", "9223372036854775808"));
    round_trip::<Evidence>(&with(
        &with(&line, "start", "-9223372036854775808"),
        "expires",
        "9223372036854775807",
    ));
    refused::<Evidence>(&with(&line, "hint", "\"Northwind Power\""));
    round_trip::<Evidence>(&with(&line, "hint", "null"));
    round_trip::<Evidence>(&with(&line, "steps", "[]"));
    let mut e = read::<Evidence>(&line);
    e.end = e.start;
    assert_write_refused(&e);
    let mut e = read::<Evidence>(&line);
    e.segments.clear();
    assert_write_refused(&e);
    let mut e = read::<Evidence>(&line);
    e.segments.reverse();
    assert_write_refused(&e);
}

#[test]
fn an_episode_holds_the_observed_site_and_the_evidence_fields() {
    let line = episode();
    round_trip::<Episode>(&with(
        &line,
        "site",
        r#"{"domain":"northwind-power.test","source":"address_field"}"#,
    ));
    round_trip::<Episode>(&with(&line, "site", "null"));
    refused::<Episode>(&with(
        &line,
        "site",
        r#"{"key":"northwind-power.test","source":"address_field"}"#,
    ));
    refused::<Episode>(&with(&line, "minutes", "6"));
    refused::<Episode>(&with(&line, "segments", "[]"));
    refused::<Episode>(&with(&line, "segments", "[2,1]"));
    refused::<Episode>(&with(&line, "end", "1794339000000000000"));
    refused::<Episode>(&with(&line, "category", "\"other\""));
    let mut grown = common::entries(&line);
    grown.push(("v".to_owned(), common::raw("1")));
    refused::<Episode>(&common::object(&grown));
    let mut e = read::<Episode>(&line);
    e.end = Instant(e.start.0 - 1);
    assert_write_refused(&e);
    let mut e = read::<Episode>(&line);
    e.minutes = 0;
    assert_write_refused(&e);
}

#[test]
fn an_observed_step_takes_each_field_only_from_its_layers() {
    for (key, value) in [
        ("app", r#"{"value":"chrome.exe","layer":"address_field"}"#),
        ("title", r#"{"value":"t","layer":"accessibility_text"}"#),
        ("input", r#"{"value":"click","layer":"extracted_text"}"#),
        ("site", r#"{"value":"shop.test","layer":"floor"}"#),
        ("text", r#"{"value":"t","layer":"floor"}"#),
        ("text", r#"{"value":"t","layer":"address_field"}"#),
        (
            "site",
            r#"{"value":"www.shop.test","layer":"address_field"}"#,
        ),
        ("app", r#"{"value":"/usr/bin/vim","layer":"floor"}"#),
        ("input", r#"{"value":"keystroke","layer":"floor"}"#),
        ("input", r#"{"value":"click","layer":"floor","extra":1}"#),
        ("input", r#"{"value":"click"}"#),
        ("title", r#"{"value":null,"layer":"floor"}"#),
    ] {
        refused::<ObservedStep>(&with(NULL_STEP, key, value));
    }
    for layer in ["accessibility_text", "extracted_text"] {
        round_trip::<ObservedStep>(&with(
            NULL_STEP,
            "text",
            &format!(r#"{{"value":"t","layer":"{layer}"}}"#),
        ));
    }
    for class in InputClass::ALL {
        round_trip::<ObservedStep>(&with(
            NULL_STEP,
            "input",
            &format!(r#"{{"value":"{}","layer":"floor"}}"#, class.as_str()),
        ));
    }
    let names: Vec<&str> = InputClass::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        names,
        [
            "printable",
            "editing",
            "navigation",
            "enter",
            "tab",
            "shortcut",
            "other",
            "click",
            "scroll",
            "drag",
            "move"
        ]
    );
    let mut grown = common::entries(NULL_STEP);
    grown.push(("locator".to_owned(), common::raw("null")));
    refused::<ObservedStep>(&common::object(&grown));
    let step = read::<ObservedStep>(FULL_STEP);
    assert_eq!(
        step.site.as_ref().unwrap().layer,
        AddressFieldLayer::AddressField
    );
    assert_eq!(step.app.as_ref().unwrap().layer, FloorLayer::Floor);
    assert_eq!(
        step.text.as_ref().unwrap().layer,
        TextLayer::AccessibilityText
    );
    let debug = format!("{step:?}");
    assert!(
        !debug.contains("Pay bill") && !debug.contains("Amount due"),
        "{debug}"
    );
    let built = ObservedStep {
        app: None,
        title: Some(Tagged {
            value: Text::cut("Pay bill"),
            layer: FloorLayer::Floor,
        }),
        input: Some(Tagged {
            value: InputClass::Click,
            layer: FloorLayer::Floor,
        }),
        site: None,
        text: None,
    };
    assert_eq!(
        serde_json::to_string(&built).unwrap(),
        r#"{"app":null,"title":{"value":"Pay bill","layer":"floor"},"input":{"value":"click","layer":"floor"},"site":null,"text":null}"#
    );
}

#[test]
fn a_text_has_at_most_1024_bytes_and_debug_hides_it() {
    let at = "a".repeat(1024);
    assert_eq!(Text::parse(&at).unwrap().as_str(), at);
    assert_eq!(Text::parse("").unwrap().as_str(), "");
    let error = Text::parse(&"a".repeat(1025)).unwrap_err();
    assert_eq!((error.type_name(), error.rule()), ("Text", "length"));
    let e_acute = "\u{e9}";
    assert_eq!(
        Text::cut(&e_acute.repeat(600)).as_str(),
        e_acute.repeat(512)
    );
    let odd = format!("a{}", e_acute.repeat(600));
    assert_eq!(Text::cut(&odd).as_str().len(), 1023);
    assert_eq!(Text::cut("short").as_str(), "short");
    assert_eq!(Text::cut(&"b".repeat(5000)).as_str().len(), 1024);
    assert_eq!(format!("{:?}", Text::cut("secret title")), "Text");
    refused::<Text>(&format!("\"{}\"", "a".repeat(1025)));
    assert_eq!(read::<Text>("\"x\"").as_str(), "x");
}

#[test]
fn ordinals_and_instants_keep_their_ranges() {
    assert_eq!(Ordinal::new(MAX63).unwrap().get(), MAX63);
    assert_eq!(Ordinal::new(0).unwrap().get(), 0);
    let error = Ordinal::new(MAX63 + 1).unwrap_err();
    assert_eq!((error.type_name(), error.rule()), ("Ordinal", "range"));
    refused::<Ordinal>("9223372036854775808");
    assert_eq!(read::<Ordinal>("7").get(), 7);
    assert!(Ordinal::new(1).unwrap() < Ordinal::new(2).unwrap());
    assert_eq!(read::<Instant>("-9223372036854775808"), Instant(i64::MIN));
    refused::<Instant>("-9223372036854775809");
    refused::<Instant>("1.0");
    assert_eq!(serde_json::to_string(&Instant(-5)).unwrap(), "-5");
}

fn item(id: &str, state: &str, summaries: &str, hint: &str) -> String {
    let workflow = if state == "dismissed" {
        with(&dismissed(), "id", &format!("\"{id}\""))
    } else {
        with(
            &with(PROPOSED, "id", &format!("\"{id}\"")),
            "state",
            &format!("\"{state}\""),
        )
    };
    format!(r#"{{"workflow":{workflow},"summaries":{summaries},"hint":{hint}}}"#)
}

const W1: &str = "10000000000000000000000000000000";
const W2: &str = "20000000000000000000000000000000";
const S1: &str = "\"a0000000000000000000000000000000\"";
const S2: &str = "\"b0000000000000000000000000000000\"";

#[test]
fn a_workflows_page_is_ascending_and_its_next_is_its_last_id() {
    let a = item(
        W1,
        "proposed",
        &format!("[{S1},{S2}]"),
        "\"northwind-power\"",
    );
    let b = item(W2, "dismissed", "[]", "null");
    let page = format!(r#"{{"workflows":[{a},{b}],"next":"{W2}"}}"#);
    assert_strict::<WorkflowsPage>(&page);
    assert_strict::<WorkflowItem>(&a);
    round_trip::<WorkflowsPage>(&format!(r#"{{"workflows":[{a},{b}],"next":null}}"#));
    round_trip::<WorkflowsPage>(r#"{"workflows":[],"next":null}"#);
    refused::<WorkflowsPage>(&format!(r#"{{"workflows":[{b},{a}],"next":null}}"#));
    refused::<WorkflowsPage>(&format!(r#"{{"workflows":[{a},{a}],"next":null}}"#));
    refused::<WorkflowsPage>(&format!(r#"{{"workflows":[{a},{b}],"next":"{W1}"}}"#));
    refused::<WorkflowsPage>(&format!(r#"{{"workflows":[],"next":"{W1}"}}"#));
    refused::<WorkflowItem>(&item(W1, "proposed", &format!("[{S2},{S1}]"), "null"));
    refused::<WorkflowItem>(&item(W1, "proposed", &format!("[{S1},{S1}]"), "null"));
    refused::<WorkflowItem>(&item(W2, "dismissed", &format!("[{S1}]"), "null"));
    refused::<WorkflowItem>(&item(W2, "dismissed", "[]", "\"northwind-power\""));
    round_trip::<WorkflowItem>(&item(W1, "paused", "[]", "null"));
    let mut p = read::<WorkflowsPage>(&page);
    p.workflows.reverse();
    p.next = None;
    assert_write_refused(&p);
    let mut p = read::<WorkflowsPage>(&page);
    p.next = Some(p.workflows[0].workflow.id().clone());
    assert_write_refused(&p);
}

#[test]
fn an_evidence_page_is_ascending_by_summary_and_its_next_is_its_last_id() {
    let e1 = with(&evidence(), "summary", S1);
    let e2 = with(&evidence(), "summary", S2);
    let page = format!(r#"{{"evidence":[{e1},{e2}],"next":{S2}}}"#);
    assert_strict::<EvidencePage>(&page);
    round_trip::<EvidencePage>(&format!(r#"{{"evidence":[{e1}],"next":null}}"#));
    round_trip::<EvidencePage>(r#"{"evidence":[],"next":null}"#);
    refused::<EvidencePage>(&format!(r#"{{"evidence":[{e2},{e1}],"next":null}}"#));
    refused::<EvidencePage>(&format!(r#"{{"evidence":[{e1},{e1}],"next":null}}"#));
    refused::<EvidencePage>(&format!(r#"{{"evidence":[{e1},{e2}],"next":{S1}}}"#));
    refused::<EvidencePage>(&format!(r#"{{"evidence":[],"next":{S1}}}"#));
    let mut p = read::<EvidencePage>(&page);
    p.evidence.reverse();
    p.next = None;
    assert_write_refused(&p);
}

#[test]
fn name_lists_hold_the_names_of_the_contract() {
    let parts: Vec<&str> = Part::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(parts, ["night", "morning", "afternoon", "evening"]);
    let triggers: Vec<&str> = Trigger::ALL.iter().map(|t| t.as_str()).collect();
    assert_eq!(triggers, ["app_context", "schedule", "manual"]);
    let states: Vec<&str> = WorkflowState::ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(states, ["proposed", "approved", "active", "paused"]);
    let layers: Vec<&str> = TextLayer::ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(layers, ["accessibility_text", "extracted_text"]);
}
