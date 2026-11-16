use std::num::NonZeroU32;

use halvo_schema::executable::names::{LocatorBy, Mode};
use halvo_schema::executable::step::Locator;
use halvo_schema::executable::text::{OwnText, RunId};
use halvo_schema::kinds::{
    check_entries, check_record, FieldType, KindEntry, Origin, RetentionClass,
};
use halvo_schema::learning::names::{EditCause, Mark, RunCause, EDIT_STATES, RUN_STATES};
use halvo_schema::learning::record::{
    Day, EditId, EditState, LearnEdit, LearnRoutine, LearnRun, LearnedTarget, RunStatus, Verdict,
};
use halvo_schema::learning::{
    Refusal, CLEAN_BOUND, KINDS, LOCATORS_BOUND, RUNS_BOUND, TARGET_BYTES,
};
use halvo_schema::{AgentId, AppId, SeedCategory, WorkflowId};

const RUN_EXAMPLE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":2,"day":20767,"mode":"replay","state":"verified","cause":null,"base":1,"assisted":1,"kept":1,"failed_step":null}"#;

const RUN_STARTED: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":1,"day":20767,"mode":"compile","state":"started","cause":null,"base":null,"assisted":0,"kept":0,"failed_step":null}"#;

const RUN_UNVERIFIED: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":2,"day":20767,"mode":"replay","state":"unverified","cause":"failed","base":2,"assisted":3,"kept":0,"failed_step":5}"#;

const EDIT_EXAMPLE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":1,"base":1,"n":4,"state":"provisional","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0"],"first":20767,"last":20767,"clean":0,"target":{"desc":"button \"Submit payment\"","locators":[{"by":"role","value":"button","name":"Submit payment"}]}}"#;

const EDIT_REVERTED: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":3,"base":2,"n":7,"state":"reverted","cause":"class_mismatch","on":"4b3a29180f7e6d5c4b3a29180f7e6d5c","runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0","8e7d6c5b4a39281706f5e4d3c2b1a09f"],"first":20767,"last":20770,"clean":1,"target":{"desc":"link Pay now","locators":[{"by":"label","value":"Pay now","name":null},{"by":"css","value":"a.pay","name":null}]}}"#;

const EDIT_ACTIVE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":3,"base":1,"n":4,"state":"active","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0","8e7d6c5b4a39281706f5e4d3c2b1a09f","7d6c5b4a39281706f5e4d3c2b1a09f8e"],"first":20767,"last":20771,"clean":2,"target":{"desc":"","locators":[{"by":"role","value":"button","name":"Pay"},{"by":"label","value":"Pay","name":null},{"by":"text","value":"Pay","name":null},{"by":"test_id","value":"pay","name":null}]}}"#;

const EDIT_WITHDRAWN: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":2,"base":1,"n":4,"state":"withdrawn","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0"],"first":20767,"last":20767,"clean":0,"target":{"desc":"x","locators":[{"by":"css","value":"button.pay","name":null}]}}"#;

const ROUTINE_EXAMPLE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":3,"day":20767,"category":"utility_bill_payment","site":{"key":"northwind-power.test","source":"address_field"},"app":"com.google.Chrome","period":30,"seen":3,"first":20705,"last":20765,"next":20795,"missed":0,"mark":"none","since":null}"#;

const ROUTINE_FIRST: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":1,"day":20700,"category":"shop_order","site":null,"app":"com.example.browser","period":null,"seen":0,"first":null,"last":null,"next":null,"missed":0,"mark":"none","since":null}"#;

const ROUTINE_STALE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":9,"day":20900,"category":"email","site":{"key":"abababababababababababababababababababababababababababababababab","source":"title"},"app":"com.example.mail","period":30,"seen":0,"first":20705,"last":20765,"next":20795,"missed":3,"mark":"stale","since":20890}"#;

type Want = (&'static str, Option<&'static str>, &'static str);

fn parts(refusal: &Refusal) -> Want {
    (refusal.type_name(), refusal.key(), refusal.rule())
}

fn changed(base: &str, from: &str, to: &str) -> String {
    assert_eq!(base.matches(from).count(), 1, "{from}");
    base.replacen(from, to, 1)
}

fn workflow() -> WorkflowId {
    WorkflowId::parse("0f1e2d3c4b5a69788796a5b4c3d2e1f0").unwrap()
}

fn agent() -> AgentId {
    AgentId::parse("1a2b3c4d5e6f708192a3b4c5d6e7f809").unwrap()
}

fn run_id(s: &str) -> RunId {
    RunId::parse(s).unwrap()
}

fn day(n: u32) -> Day {
    Day::new(n).unwrap()
}

fn number(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).unwrap()
}

fn locator(by: LocatorBy, value: &str, name: Option<&str>) -> Locator {
    Locator {
        by,
        value: OwnText::parse(value).unwrap(),
        name: name.map(|n| OwnText::parse(n).unwrap()),
    }
}

fn run_refused(line: &str) -> Want {
    match LearnRun::parse(line.as_bytes()) {
        Ok(_) => panic!("taken: {line}"),
        Err(refusal) => parts(&refusal),
    }
}

fn edit_refused(line: &str) -> Want {
    match LearnEdit::parse(line.as_bytes()) {
        Ok(_) => panic!("taken: {line}"),
        Err(refusal) => parts(&refusal),
    }
}

fn routine_refused(line: &str) -> Want {
    match LearnRoutine::parse(line.as_bytes()) {
        Ok(_) => panic!("taken: {line}"),
        Err(refusal) => parts(&refusal),
    }
}

fn learn_edit_stand_in() -> KindEntry {
    KindEntry {
        name: "learn_edit",
        class: RetentionClass::Kept,
        origin: Origin::Own,
        family: "learn",
        workflow_key: Some("workflow"),
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("agent", FieldType::Id),
            ("edit", FieldType::Id),
            ("rev", FieldType::Count),
            ("base", FieldType::Count),
            ("n", FieldType::Count),
            ("state", FieldType::Name),
            ("cause", FieldType::Name),
            ("on", FieldType::Id),
            ("runs", FieldType::Id),
            ("first", FieldType::Day),
            ("last", FieldType::Day),
            ("clean", FieldType::Count),
            ("target", FieldType::OwnText),
        ],
        example: EDIT_EXAMPLE,
    }
}

const FIVE_LOCATORS: &str = r#""locators":[{"by":"role","value":"button","name":"Submit payment"},{"by":"label","value":"Pay","name":null},{"by":"text","value":"Pay","name":null},{"by":"test_id","value":"pay","name":null},{"by":"css","value":"button.pay","name":null}]"#;

const ONE_LOCATOR: &str = r#""locators":[{"by":"role","value":"button","name":"Submit payment"}]"#;

#[test]
fn refusal_edit_extra_key() {
    let cases: [(&str, String, Want); 5] = [
        (
            "a key args in the target",
            changed(EDIT_EXAMPLE, r#""locators":"#, r#""args":[],"locators":"#),
            ("LearnedTarget", Some("target"), "unknown_key"),
        ),
        (
            "a key effect in the target",
            changed(
                EDIT_EXAMPLE,
                r#""locators":"#,
                r#""effect":"none","locators":"#,
            ),
            ("LearnedTarget", Some("target"), "unknown_key"),
        ),
        (
            "an empty runs",
            changed(
                EDIT_EXAMPLE,
                r#""runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0"]"#,
                r#""runs":[]"#,
            ),
            ("LearnEdit", Some("runs"), "runs_bound"),
        ),
        (
            "five locators",
            changed(EDIT_EXAMPLE, ONE_LOCATOR, FIVE_LOCATORS),
            ("LearnedTarget", Some("locators"), "locators_bound"),
        ),
        (
            "first 0",
            changed(EDIT_EXAMPLE, r#""first":20767"#, r#""first":0"#),
            ("Day", Some("first"), "zero"),
        ),
    ];
    let stand_in = learn_edit_stand_in();
    assert_eq!(check_entries(&[&stand_in]), Vec::<String>::new());
    assert_eq!(check_record(&stand_in, EDIT_EXAMPLE), Ok(()));
    assert!(LearnEdit::parse(EDIT_EXAMPLE.as_bytes()).is_ok());
    for (what, line, want) in cases {
        let refusal = LearnEdit::parse(line.as_bytes()).expect_err(what);
        assert_eq!(parts(&refusal), want, "{what}");
        let error = serde_json::from_str::<LearnEdit>(&line).expect_err(what);
        assert!(
            error.to_string().starts_with(&refusal.to_string()),
            "{what}: {error}"
        );
        assert_eq!(
            check_record(&stand_in, &line),
            Ok(()),
            "{what}: the generic check of the crate takes the line"
        );
    }
}

#[test]
fn the_active_reverted_and_withdrawn_rows_pass_the_check_of_the_registered_entry() {
    let registered = KINDS.iter().find(|k| k.name == "learn_edit").unwrap();
    for line in [EDIT_ACTIVE, EDIT_REVERTED, EDIT_WITHDRAWN] {
        assert_eq!(check_record(registered, line), Ok(()), "{line}");
    }
}

#[test]
fn the_stand_in_of_the_edit_kind_is_the_registered_entry() {
    let registered: Vec<&KindEntry> = KINDS.iter().filter(|k| k.name == "learn_edit").collect();
    assert_eq!(registered, [&learn_edit_stand_in()]);
}

#[test]
fn each_kind_reads_each_state_of_its_contract_and_writes_the_same_bytes() {
    for line in [RUN_EXAMPLE, RUN_STARTED, RUN_UNVERIFIED] {
        let run = LearnRun::parse(line.as_bytes()).unwrap();
        assert_eq!(run.to_json().unwrap(), line);
        let read: LearnRun = serde_json::from_str(line).unwrap();
        assert_eq!(read, run);
        assert_eq!(serde_json::to_string(&run).unwrap(), line);
    }
    for line in [EDIT_EXAMPLE, EDIT_REVERTED, EDIT_ACTIVE, EDIT_WITHDRAWN] {
        let edit = LearnEdit::parse(line.as_bytes()).unwrap();
        assert_eq!(edit.to_json().unwrap(), line);
        let read: LearnEdit = serde_json::from_str(line).unwrap();
        assert_eq!(read, edit);
        assert_eq!(serde_json::to_string(&edit).unwrap(), line);
    }
    for line in [ROUTINE_EXAMPLE, ROUTINE_FIRST, ROUTINE_STALE] {
        let routine = LearnRoutine::parse(line.as_bytes()).unwrap();
        assert_eq!(routine.to_json().unwrap(), line);
        let read: LearnRoutine = serde_json::from_str(line).unwrap();
        assert_eq!(read, routine);
        assert_eq!(serde_json::to_string(&routine).unwrap(), line);
    }
}

#[test]
fn the_parsed_values_hold_what_the_lines_say() {
    let run = LearnRun::parse(RUN_EXAMPLE.as_bytes()).unwrap();
    assert_eq!(
        run,
        LearnRun {
            workflow: workflow(),
            agent: agent(),
            run: run_id("9f8e7d6c5b4a39281706f5e4d3c2b1a0"),
            day: day(20767),
            mode: Mode::Replay,
            status: RunStatus::Ended {
                verdict: Verdict::Verified,
                base: Some(number(1)),
                assisted: 1,
                kept: 1,
                failed_step: None,
            },
        }
    );
    assert_eq!(run.rev(), 2);
    let started = LearnRun::parse(RUN_STARTED.as_bytes()).unwrap();
    assert_eq!(started.status, RunStatus::Started);
    assert_eq!(started.rev(), 1);
    assert_eq!(started.mode, Mode::Compile);
    let unverified = LearnRun::parse(RUN_UNVERIFIED.as_bytes()).unwrap();
    assert_eq!(
        unverified.status,
        RunStatus::Ended {
            verdict: Verdict::Unverified(RunCause::Failed),
            base: Some(number(2)),
            assisted: 3,
            kept: 0,
            failed_step: Some(number(5)),
        }
    );

    let edit = LearnEdit::parse(EDIT_EXAMPLE.as_bytes()).unwrap();
    assert_eq!(
        edit,
        LearnEdit {
            workflow: workflow(),
            agent: agent(),
            edit: EditId::parse("5c4b3a29180f7e6d5c4b3a29180f7e6d").unwrap(),
            rev: number(1),
            base: number(1),
            n: number(4),
            state: EditState::Provisional,
            on: None,
            runs: vec![run_id("9f8e7d6c5b4a39281706f5e4d3c2b1a0")],
            first: day(20767),
            last: day(20767),
            clean: 0,
            target: LearnedTarget {
                desc: OwnText::parse("button \"Submit payment\"").unwrap(),
                locators: vec![locator(LocatorBy::Role, "button", Some("Submit payment"))],
            },
        }
    );
    let reverted = LearnEdit::parse(EDIT_REVERTED.as_bytes()).unwrap();
    assert_eq!(
        reverted.state,
        EditState::Reverted(EditCause::ClassMismatch)
    );
    assert_eq!(
        reverted.on,
        Some(EditId::parse("4b3a29180f7e6d5c4b3a29180f7e6d5c").unwrap())
    );
    assert_eq!(
        LearnEdit::parse(EDIT_ACTIVE.as_bytes()).unwrap().state,
        EditState::Active
    );
    assert_eq!(
        LearnEdit::parse(EDIT_WITHDRAWN.as_bytes()).unwrap().state,
        EditState::Withdrawn
    );

    let routine = LearnRoutine::parse(ROUTINE_EXAMPLE.as_bytes()).unwrap();
    assert_eq!(routine.workflow, workflow());
    assert_eq!(routine.agent, agent());
    assert_eq!(routine.rev, number(3));
    assert_eq!(routine.day, day(20767));
    assert_eq!(routine.category, SeedCategory::UtilityBillPayment);
    assert_eq!(routine.app, AppId::parse("com.google.Chrome").unwrap());
    assert_eq!(routine.period, Some(number(30)));
    assert_eq!(routine.seen, 3);
    assert_eq!(routine.first, Some(day(20705)));
    assert_eq!(routine.last, Some(day(20765)));
    assert_eq!(routine.next, Some(day(20795)));
    assert_eq!(routine.missed, 0);
    assert_eq!(routine.mark, Mark::None);
    assert_eq!(routine.since, None);
    assert!(routine.site.is_some());
    let first = LearnRoutine::parse(ROUTINE_FIRST.as_bytes()).unwrap();
    assert_eq!(first.site, None);
    assert_eq!(first.period, None);
    let stale = LearnRoutine::parse(ROUTINE_STALE.as_bytes()).unwrap();
    assert_eq!(stale.mark, Mark::Stale);
    assert_eq!(stale.since, Some(day(20890)));
}

#[test]
fn the_hand_made_list_is_refused_by_each_parser() {
    let run_cases: Vec<(&str, String, Want)> = vec![
        (
            "a JSON array where the record stands, its values by position",
            r#"[1,"0f1e2d3c4b5a69788796a5b4c3d2e1f0","1a2b3c4d5e6f708192a3b4c5d6e7f809","9f8e7d6c5b4a39281706f5e4d3c2b1a0",2,20767,"replay","verified",null,1,1,1,null]"#.to_owned(),
            ("LearnRun", None, "not_an_object"),
        ),
        (
            "a key twice",
            changed(RUN_EXAMPLE, r#""kept":1,"#, r#""kept":1,"kept":1,"#),
            ("LearnRun", None, "repeated_key"),
        ),
        (
            "a key set to null where it must be absent",
            changed(RUN_EXAMPLE, r#""kept":1,"#, r#""kept":1,"edit":null,"#),
            ("LearnRun", None, "unknown_key"),
        ),
        (
            "a number as a string",
            changed(RUN_EXAMPLE, r#""day":20767"#, r#""day":"20767""#),
            ("Day", Some("day"), "not_a_count"),
        ),
        (
            "an unknown key",
            changed(RUN_EXAMPLE, r#""kept":1,"#, r#""kept":1,"zz":1,"#),
            ("LearnRun", None, "unknown_key"),
        ),
        (
            "a key of the routine's object",
            changed(RUN_EXAMPLE, r#""kept":1,"#, r#""kept":1,"seen":1,"#),
            ("LearnRun", None, "unknown_key"),
        ),
        (
            "a key missing",
            changed(RUN_EXAMPLE, r#""kept":1,"#, ""),
            ("LearnRun", Some("kept"), "missing_key"),
        ),
    ];
    for (what, line, want) in run_cases {
        assert_eq!(run_refused(&line), want, "{what}");
    }

    let edit_cases: Vec<(&str, String, Want)> = vec![
        (
            "a JSON array where the record stands",
            format!("[{EDIT_EXAMPLE}]"),
            ("LearnEdit", None, "not_an_object"),
        ),
        (
            "a JSON array where the target stands, its values by position",
            changed(
                EDIT_EXAMPLE,
                r#"{"desc":"button \"Submit payment\"","locators":[{"by":"role","value":"button","name":"Submit payment"}]}"#,
                r#"["button",[["role","button","Submit payment"]]]"#,
            ),
            ("LearnedTarget", Some("target"), "not_an_object"),
        ),
        (
            "a JSON array where a locator stands",
            changed(
                EDIT_EXAMPLE,
                r#"{"by":"role","value":"button","name":"Submit payment"}"#,
                r#"["role","button","Submit payment"]"#,
            ),
            ("Locator", Some("locators"), "not_an_object"),
        ),
        (
            "a key twice",
            changed(EDIT_EXAMPLE, r#""clean":0,"#, r#""clean":0,"clean":0,"#),
            ("LearnEdit", None, "repeated_key"),
        ),
        (
            "a key twice in the target",
            changed(EDIT_EXAMPLE, r#""locators":"#, r#""desc":"x","locators":"#),
            ("LearnedTarget", Some("target"), "repeated_key"),
        ),
        (
            "a key set to null where it must be absent, in the target",
            changed(
                EDIT_EXAMPLE,
                r#""locators":"#,
                r#""frame":null,"locators":"#,
            ),
            ("LearnedTarget", Some("target"), "unknown_key"),
        ),
        (
            "a number as a string",
            changed(EDIT_EXAMPLE, r#""n":4"#, r#""n":"4""#),
            ("NonZeroU32", Some("n"), "not_a_count"),
        ),
        (
            "an unknown key",
            changed(EDIT_EXAMPLE, r#""clean":0,"#, r#""clean":0,"zz":0,"#),
            ("LearnEdit", None, "unknown_key"),
        ),
        (
            "a key of the run's object",
            changed(
                EDIT_EXAMPLE,
                r#""clean":0,"#,
                r#""clean":0,"mode":"replay","#,
            ),
            ("LearnEdit", None, "unknown_key"),
        ),
        (
            "a key of a web target in the target",
            changed(
                EDIT_EXAMPLE,
                r#""locators":"#,
                r#""credential":"no","locators":"#,
            ),
            ("LearnedTarget", Some("target"), "unknown_key"),
        ),
        (
            "a key of a step in a locator",
            changed(
                EDIT_EXAMPLE,
                r#""name":"Submit payment"}"#,
                r#""name":"Submit payment","verb":"click"}"#,
            ),
            ("Locator", Some("locators"), "unknown_key"),
        ),
        (
            "the target missing its locators",
            changed(EDIT_EXAMPLE, &format!(",{ONE_LOCATOR}"), ""),
            ("LearnedTarget", Some("locators"), "missing_key"),
        ),
    ];
    for (what, line, want) in edit_cases {
        assert_eq!(edit_refused(&line), want, "{what}");
    }

    let routine_cases: Vec<(&str, String, Want)> = vec![
        (
            "a JSON array where the record stands",
            format!("[{ROUTINE_EXAMPLE}]"),
            ("LearnRoutine", None, "not_an_object"),
        ),
        (
            "a JSON array where the site stands, its values by position",
            changed(
                ROUTINE_EXAMPLE,
                r#"{"key":"northwind-power.test","source":"address_field"}"#,
                r#"["northwind-power.test","address_field"]"#,
            ),
            ("SiteValue", Some("site"), "not_an_object"),
        ),
        (
            "a key twice",
            changed(
                ROUTINE_EXAMPLE,
                r#""missed":0,"#,
                r#""missed":0,"missed":0,"#,
            ),
            ("LearnRoutine", None, "repeated_key"),
        ),
        (
            "a key twice in the site",
            changed(
                ROUTINE_EXAMPLE,
                r#""source":"address_field""#,
                r#""source":"address_field","key":"northwind-power.test""#,
            ),
            ("SiteValue", Some("site"), "repeated_key"),
        ),
        (
            "a key set to null where it must be absent",
            changed(
                ROUTINE_EXAMPLE,
                r#""missed":0,"#,
                r#""missed":0,"state":null,"#,
            ),
            ("LearnRoutine", None, "unknown_key"),
        ),
        (
            "a number as a string",
            changed(ROUTINE_EXAMPLE, r#""seen":3"#, r#""seen":"3""#),
            ("u32", Some("seen"), "not_a_count"),
        ),
        (
            "an unknown key",
            changed(ROUTINE_EXAMPLE, r#""missed":0,"#, r#""missed":0,"zz":0,"#),
            ("LearnRoutine", None, "unknown_key"),
        ),
        (
            "a key of the edit's object",
            changed(
                ROUTINE_EXAMPLE,
                r#""missed":0,"#,
                r#""missed":0,"clean":0,"#,
            ),
            ("LearnRoutine", None, "unknown_key"),
        ),
        (
            "a key of the workflow's site in the site",
            changed(
                ROUTINE_EXAMPLE,
                r#"{"key":"northwind-power.test","source":"address_field"}"#,
                r#"{"domain":"northwind-power.test","source":"address_field"}"#,
            ),
            ("SiteValue", Some("site"), "site_form"),
        ),
    ];
    for (what, line, want) in routine_cases {
        assert_eq!(routine_refused(&line), want, "{what}");
    }
}

#[test]
fn the_run_parser_refuses_each_form_that_the_contract_refuses() {
    let cases: Vec<(&str, String, Want)> = vec![
        (
            "rev 0",
            changed(RUN_EXAMPLE, r#""rev":2"#, r#""rev":0"#),
            ("NonZeroU32", Some("rev"), "zero"),
        ),
        (
            "rev 3",
            changed(RUN_EXAMPLE, r#""rev":2"#, r#""rev":3"#),
            ("LearnRun", Some("rev"), "rev"),
        ),
        (
            "rev 1 verified",
            changed(RUN_EXAMPLE, r#""rev":2"#, r#""rev":1"#),
            ("LearnRun", Some("state"), "state_of_rev"),
        ),
        (
            "rev 2 started",
            changed(RUN_STARTED, r#""rev":1"#, r#""rev":2"#),
            ("LearnRun", Some("state"), "state_of_rev"),
        ),
        (
            "verified with a cause",
            changed(RUN_EXAMPLE, r#""cause":null"#, r#""cause":"failed""#),
            ("LearnRun", Some("cause"), "cause_of_state"),
        ),
        (
            "unverified without a cause",
            changed(RUN_UNVERIFIED, r#""cause":"failed""#, r#""cause":null"#),
            ("LearnRun", Some("cause"), "cause_of_state"),
        ),
        (
            "started with a cause",
            changed(RUN_STARTED, r#""cause":null"#, r#""cause":"limit""#),
            ("LearnRun", Some("cause"), "cause_of_state"),
        ),
        (
            "started with a base",
            changed(RUN_STARTED, r#""base":null"#, r#""base":1"#),
            ("LearnRun", Some("base"), "start_values"),
        ),
        (
            "started with assisted 1",
            changed(RUN_STARTED, r#""assisted":0"#, r#""assisted":1"#),
            ("LearnRun", Some("assisted"), "start_values"),
        ),
        (
            "started with kept 1",
            changed(RUN_STARTED, r#""kept":0"#, r#""kept":1"#),
            ("LearnRun", Some("kept"), "start_values"),
        ),
        (
            "started with a failed step",
            changed(RUN_STARTED, r#""failed_step":null"#, r#""failed_step":1"#),
            ("LearnRun", Some("failed_step"), "start_values"),
        ),
        (
            "day 0",
            changed(RUN_EXAMPLE, r#""day":20767"#, r#""day":0"#),
            ("Day", Some("day"), "zero"),
        ),
        (
            "base 0",
            changed(RUN_EXAMPLE, r#""base":1"#, r#""base":0"#),
            ("NonZeroU32", Some("base"), "zero"),
        ),
        (
            "failed step 0",
            changed(RUN_UNVERIFIED, r#""failed_step":5"#, r#""failed_step":0"#),
            ("NonZeroU32", Some("failed_step"), "zero"),
        ),
        (
            "a mode outside the list",
            changed(RUN_EXAMPLE, r#""mode":"replay""#, r#""mode":"learn""#),
            ("Mode", Some("mode"), "unknown_name"),
        ),
        (
            "a state outside the list",
            changed(RUN_EXAMPLE, r#""state":"verified""#, r#""state":"done""#),
            ("RunState", Some("state"), "unknown_name"),
        ),
        (
            "an outcome name as a cause",
            changed(
                RUN_UNVERIFIED,
                r#""cause":"failed""#,
                r#""cause":"completed""#,
            ),
            ("RunCause", Some("cause"), "unknown_name"),
        ),
        (
            "a log cause as a cause",
            changed(
                RUN_UNVERIFIED,
                r#""cause":"failed""#,
                r#""cause":"timeout""#,
            ),
            ("RunCause", Some("cause"), "unknown_name"),
        ),
        (
            "v 2",
            changed(RUN_EXAMPLE, r#""v":1"#, r#""v":2"#),
            ("LearnRun", Some("v"), "version"),
        ),
        (
            "v 1.0",
            changed(RUN_EXAMPLE, r#""v":1"#, r#""v":1.0"#),
            ("LearnRun", Some("v"), "version"),
        ),
        (
            "a workflow id in upper case",
            changed(
                RUN_EXAMPLE,
                "0f1e2d3c4b5a69788796a5b4c3d2e1f0",
                "0F1E2D3C4B5A69788796A5B4C3D2E1F0",
            ),
            ("WorkflowId", Some("workflow"), "hex_digit"),
        ),
        (
            "an agent id of 31 digits",
            changed(
                RUN_EXAMPLE,
                "1a2b3c4d5e6f708192a3b4c5d6e7f809",
                "1a2b3c4d5e6f708192a3b4c5d6e7f80",
            ),
            ("AgentId", Some("agent"), "length"),
        ),
        (
            "a run id that is no string",
            changed(RUN_EXAMPLE, r#""9f8e7d6c5b4a39281706f5e4d3c2b1a0""#, "9"),
            ("RunId", Some("run"), "not_a_string"),
        ),
        (
            "a boolean count",
            changed(RUN_EXAMPLE, r#""kept":1"#, r#""kept":true"#),
            ("u32", Some("kept"), "not_a_count"),
        ),
        (
            "a count with a fraction",
            changed(RUN_EXAMPLE, r#""assisted":1"#, r#""assisted":1.0"#),
            ("u32", Some("assisted"), "not_a_count"),
        ),
        (
            "a count with an exponent",
            changed(RUN_EXAMPLE, r#""assisted":1"#, r#""assisted":1e0"#),
            ("u32", Some("assisted"), "not_a_count"),
        ),
        (
            "a negative count",
            changed(RUN_EXAMPLE, r#""assisted":1"#, r#""assisted":-1"#),
            ("u32", Some("assisted"), "not_a_count"),
        ),
        (
            "a count above u32",
            changed(RUN_EXAMPLE, r#""assisted":1"#, r#""assisted":4294967296"#),
            ("u32", Some("assisted"), "range"),
        ),
        (
            "a day above u32",
            changed(RUN_EXAMPLE, r#""day":20767"#, r#""day":4294967296"#),
            ("Day", Some("day"), "range"),
        ),
        (
            "broken JSON",
            changed(RUN_EXAMPLE, r#""kept":1,"#, r#""kept":1,,"#),
            ("LearnRun", None, "json"),
        ),
        (
            "text after the object",
            format!("{RUN_EXAMPLE}x"),
            ("LearnRun", None, "json"),
        ),
    ];
    for (what, line, want) in cases {
        assert_eq!(run_refused(&line), want, "{what}");
    }
    assert_eq!(
        run_refused(
            &RUN_EXAMPLE.as_bytes()[..10]
                .iter()
                .map(|b| *b as char)
                .collect::<String>()
        ),
        ("LearnRun", None, "json")
    );
    let mut invalid = RUN_EXAMPLE.as_bytes().to_vec();
    invalid.push(0xff);
    assert_eq!(
        LearnRun::parse(&invalid).map_err(|r| parts(&r)),
        Err(("LearnRun", None, "json"))
    );
}

#[test]
fn the_edit_parser_refuses_each_form_that_the_contract_refuses() {
    let four = r#""runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0","8e7d6c5b4a39281706f5e4d3c2b1a09f","7d6c5b4a39281706f5e4d3c2b1a09f8e","6c5b4a39281706f5e4d3c2b1a09f8e7d"]"#;
    let three = r#""runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0","8e7d6c5b4a39281706f5e4d3c2b1a09f","7d6c5b4a39281706f5e4d3c2b1a09f8e"]"#;
    let cases: Vec<(&str, String, Want)> = vec![
        (
            "a reverted row without a cause",
            changed(
                EDIT_REVERTED,
                r#""cause":"class_mismatch""#,
                r#""cause":null"#,
            ),
            ("LearnEdit", Some("cause"), "cause_of_state"),
        ),
        (
            "a provisional row with a cause",
            changed(EDIT_EXAMPLE, r#""cause":null"#, r#""cause":"step_failed""#),
            ("LearnEdit", Some("cause"), "cause_of_state"),
        ),
        (
            "an active row with a cause",
            changed(EDIT_ACTIVE, r#""cause":null"#, r#""cause":"origin""#),
            ("LearnEdit", Some("cause"), "cause_of_state"),
        ),
        (
            "a withdrawn row with a cause",
            changed(EDIT_WITHDRAWN, r#""cause":null"#, r#""cause":"origin""#),
            ("LearnEdit", Some("cause"), "cause_of_state"),
        ),
        (
            "a cause that does not count",
            changed(
                EDIT_REVERTED,
                r#""cause":"class_mismatch""#,
                r#""cause":"timeout""#,
            ),
            ("EditCause", Some("cause"), "unknown_name"),
        ),
        (
            "a run cause as an edit cause",
            changed(
                EDIT_REVERTED,
                r#""cause":"class_mismatch""#,
                r#""cause":"failed""#,
            ),
            ("EditCause", Some("cause"), "unknown_name"),
        ),
        (
            "a state outside the list",
            changed(
                EDIT_EXAMPLE,
                r#""state":"provisional""#,
                r#""state":"candidate""#,
            ),
            ("EditState", Some("state"), "unknown_name"),
        ),
        (
            "four runs",
            changed(EDIT_ACTIVE, three, four),
            ("LearnEdit", Some("runs"), "runs_bound"),
        ),
        (
            "a run twice",
            changed(
                EDIT_ACTIVE,
                "7d6c5b4a39281706f5e4d3c2b1a09f8e",
                "9f8e7d6c5b4a39281706f5e4d3c2b1a0",
            ),
            ("LearnEdit", Some("runs"), "repeated_run"),
        ),
        (
            "runs that is no list",
            changed(
                EDIT_EXAMPLE,
                r#"["9f8e7d6c5b4a39281706f5e4d3c2b1a0"]"#,
                r#""9f8e7d6c5b4a39281706f5e4d3c2b1a0""#,
            ),
            ("list", Some("runs"), "not_a_list"),
        ),
        (
            "a run id that is no id",
            changed(
                EDIT_EXAMPLE,
                r#"["9f8e7d6c5b4a39281706f5e4d3c2b1a0"]"#,
                r#"["run"]"#,
            ),
            ("RunId", Some("runs"), "hex32"),
        ),
        (
            "clean 3",
            changed(EDIT_ACTIVE, r#""clean":2"#, r#""clean":3"#),
            ("LearnEdit", Some("clean"), "clean_bound"),
        ),
        (
            "clean 2 with one run",
            changed(EDIT_EXAMPLE, r#""clean":0"#, r#""clean":2"#),
            ("LearnEdit", Some("clean"), "clean_of_runs"),
        ),
        (
            "clean 0 with two runs",
            changed(EDIT_REVERTED, r#""clean":1"#, r#""clean":0"#),
            ("LearnEdit", Some("clean"), "clean_of_runs"),
        ),
        (
            "first above last",
            changed(EDIT_REVERTED, r#""first":20767"#, r#""first":20771"#),
            ("LearnEdit", Some("first"), "first_after_last"),
        ),
        (
            "last 0",
            changed(EDIT_EXAMPLE, r#""last":20767"#, r#""last":0"#),
            ("Day", Some("last"), "zero"),
        ),
        (
            "rev 0",
            changed(EDIT_EXAMPLE, r#""rev":1"#, r#""rev":0"#),
            ("NonZeroU32", Some("rev"), "zero"),
        ),
        (
            "base 0",
            changed(EDIT_EXAMPLE, r#""base":1"#, r#""base":0"#),
            ("NonZeroU32", Some("base"), "zero"),
        ),
        (
            "n 0",
            changed(EDIT_EXAMPLE, r#""n":4"#, r#""n":0"#),
            ("NonZeroU32", Some("n"), "zero"),
        ),
        (
            "an edit id of 33 digits",
            changed(
                EDIT_EXAMPLE,
                "5c4b3a29180f7e6d5c4b3a29180f7e6d",
                "5c4b3a29180f7e6d5c4b3a29180f7e6d0",
            ),
            ("EditId", Some("edit"), "hex32"),
        ),
        (
            "an on that is no id",
            changed(
                EDIT_REVERTED,
                "4b3a29180f7e6d5c4b3a29180f7e6d5c",
                "4b3a29180f7e6d5c4b3a29180f7e6d5g",
            ),
            ("EditId", Some("on"), "hex32"),
        ),
        (
            "an empty locator list",
            changed(EDIT_EXAMPLE, ONE_LOCATOR, r#""locators":[]"#),
            ("LearnedTarget", Some("locators"), "locators_bound"),
        ),
        (
            "a locator twice",
            changed(
                EDIT_REVERTED,
                r#"{"by":"css","value":"a.pay","name":null}"#,
                r#"{"by":"label","value":"Pay now","name":null}"#,
            ),
            ("LearnedTarget", Some("locators"), "repeated_locator"),
        ),
        (
            "a role locator without its name",
            changed(EDIT_EXAMPLE, r#""name":"Submit payment""#, r#""name":null"#),
            ("Locator", Some("name"), "name_of_by"),
        ),
        (
            "a label locator with a name",
            changed(
                EDIT_REVERTED,
                r#"{"by":"label","value":"Pay now","name":null}"#,
                r#"{"by":"label","value":"Pay now","name":"Pay"}"#,
            ),
            ("Locator", Some("name"), "name_of_by"),
        ),
        (
            "a locator by outside the list",
            changed(EDIT_REVERTED, r#""by":"css""#, r#""by":"xpath""#),
            ("LocatorBy", Some("by"), "unknown_name"),
        ),
        (
            "a desc of 1,025 bytes",
            changed(
                EDIT_WITHDRAWN,
                r#""desc":"x""#,
                &format!(r#""desc":"{}""#, "x".repeat(1025)),
            ),
            ("OwnText", Some("desc"), "own_text_length"),
        ),
        (
            "a desc that is no string",
            changed(EDIT_WITHDRAWN, r#""desc":"x""#, r#""desc":1"#),
            ("OwnText", Some("desc"), "not_a_string"),
        ),
        (
            "the target missing its desc",
            changed(EDIT_WITHDRAWN, r#""desc":"x","#, ""),
            ("LearnedTarget", Some("desc"), "missing_key"),
        ),
        (
            "the target null",
            changed(
                EDIT_WITHDRAWN,
                r#"{"desc":"x","locators":[{"by":"css","value":"button.pay","name":null}]}"#,
                "null",
            ),
            ("LearnedTarget", Some("target"), "not_an_object"),
        ),
        (
            "a key verb in the target",
            changed(
                EDIT_WITHDRAWN,
                r#""desc":"x","#,
                r#""desc":"x","verb":"click","#,
            ),
            ("LearnedTarget", Some("target"), "unknown_key"),
        ),
        (
            "a key class in the target",
            changed(
                EDIT_WITHDRAWN,
                r#""desc":"x","#,
                r#""desc":"x","class":"read","#,
            ),
            ("LearnedTarget", Some("target"), "unknown_key"),
        ),
    ];
    for (what, line, want) in cases {
        assert_eq!(edit_refused(&line), want, "{what}");
    }
}

#[test]
fn the_routine_parser_refuses_each_form_that_the_contract_refuses() {
    let cases: Vec<(&str, String, Want)> = vec![
        (
            "a first day without a last",
            changed(ROUTINE_EXAMPLE, r#""last":20765"#, r#""last":null"#),
            ("LearnRoutine", Some("first"), "first_and_last"),
        ),
        (
            "a last day without a first",
            changed(ROUTINE_EXAMPLE, r#""first":20705"#, r#""first":null"#),
            ("LearnRoutine", Some("first"), "first_and_last"),
        ),
        (
            "a first day above the last",
            changed(ROUTINE_STALE, r#""first":20705"#, r#""first":20766"#),
            ("LearnRoutine", Some("first"), "first_after_last"),
        ),
        (
            "seen above 0 with no days",
            changed(ROUTINE_FIRST, r#""seen":0"#, r#""seen":1"#),
            ("LearnRoutine", Some("seen"), "seen_without_days"),
        ),
        (
            "a next that is not last + period",
            changed(ROUTINE_EXAMPLE, r#""next":20795"#, r#""next":20796"#),
            ("LearnRoutine", Some("next"), "next_of_last_and_period"),
        ),
        (
            "a next without a period",
            changed(ROUTINE_EXAMPLE, r#""period":30"#, r#""period":null"#),
            ("LearnRoutine", Some("next"), "next_of_last_and_period"),
        ),
        (
            "no next with a last and a period",
            changed(ROUTINE_EXAMPLE, r#""next":20795"#, r#""next":null"#),
            ("LearnRoutine", Some("next"), "next_of_last_and_period"),
        ),
        (
            "a next with no days",
            changed(ROUTINE_FIRST, r#""next":null"#, r#""next":20795"#),
            ("LearnRoutine", Some("next"), "next_of_last_and_period"),
        ),
        (
            "a next past the last day of u32",
            changed(
                &changed(
                    &changed(ROUTINE_EXAMPLE, r#""last":20765"#, r#""last":4294967290"#),
                    r#""period":30"#,
                    r#""period":10"#,
                ),
                r#""next":20795"#,
                r#""next":4294967295"#,
            ),
            ("LearnRoutine", Some("next"), "next_of_last_and_period"),
        ),
        (
            "a missed cycle without a next",
            changed(ROUTINE_FIRST, r#""missed":0"#, r#""missed":1"#),
            ("LearnRoutine", Some("missed"), "missed_without_next"),
        ),
        (
            "the mark stale without a since",
            changed(ROUTINE_STALE, r#""since":20890"#, r#""since":null"#),
            ("LearnRoutine", Some("since"), "stale_without_since"),
        ),
        (
            "a missed cycle written on the last day",
            changed(
                &changed(ROUTINE_EXAMPLE, r#""day":20767"#, r#""day":20765"#),
                r#""missed":0"#,
                r#""missed":1"#,
            ),
            ("LearnRoutine", Some("missed"), "missed_on_or_before_last"),
        ),
        (
            "a missed cycle written before the last day",
            changed(
                &changed(ROUTINE_EXAMPLE, r#""day":20767"#, r#""day":20700"#),
                r#""missed":0"#,
                r#""missed":1"#,
            ),
            ("LearnRoutine", Some("missed"), "missed_on_or_before_last"),
        ),
        (
            "the mark stale with no missed cycle",
            changed(ROUTINE_STALE, r#""missed":3"#, r#""missed":0"#),
            ("LearnRoutine", Some("mark"), "stale_without_missed"),
        ),
        (
            "period 0",
            changed(ROUTINE_EXAMPLE, r#""period":30"#, r#""period":0"#),
            ("NonZeroU32", Some("period"), "zero"),
        ),
        (
            "day 0",
            changed(ROUTINE_EXAMPLE, r#""day":20767"#, r#""day":0"#),
            ("Day", Some("day"), "zero"),
        ),
        (
            "first 0",
            changed(ROUTINE_EXAMPLE, r#""first":20705"#, r#""first":0"#),
            ("Day", Some("first"), "zero"),
        ),
        (
            "since 0",
            changed(ROUTINE_STALE, r#""since":20890"#, r#""since":0"#),
            ("Day", Some("since"), "zero"),
        ),
        (
            "next 0",
            changed(ROUTINE_EXAMPLE, r#""next":20795"#, r#""next":0"#),
            ("Day", Some("next"), "zero"),
        ),
        (
            "rev 0",
            changed(ROUTINE_EXAMPLE, r#""rev":3"#, r#""rev":0"#),
            ("NonZeroU32", Some("rev"), "zero"),
        ),
        (
            "a category outside the seed list",
            changed(
                ROUTINE_EXAMPLE,
                r#""category":"utility_bill_payment""#,
                r#""category":"other""#,
            ),
            ("SeedCategory", Some("category"), "unknown_name"),
        ),
        (
            "an app id that is a path",
            changed(
                ROUTINE_EXAMPLE,
                r#""app":"com.google.Chrome""#,
                r#""app":"/Applications/Chrome.app""#,
            ),
            ("AppId", Some("app"), "separator"),
        ),
        (
            "a title site with a domain key",
            changed(
                ROUTINE_EXAMPLE,
                r#""source":"address_field""#,
                r#""source":"title""#,
            ),
            ("SiteValue", Some("site"), "site_form"),
        ),
        (
            "a site with a key of another object",
            changed(
                ROUTINE_EXAMPLE,
                r#""source":"address_field""#,
                r#""source":"address_field","layer":"floor""#,
            ),
            ("SiteValue", Some("site"), "site_form"),
        ),
        (
            "a mark outside the list",
            changed(ROUTINE_EXAMPLE, r#""mark":"none""#, r#""mark":"late""#),
            ("Mark", Some("mark"), "unknown_name"),
        ),
        (
            "a boolean mark",
            changed(ROUTINE_EXAMPLE, r#""mark":"none""#, r#""mark":false"#),
            ("Mark", Some("mark"), "not_a_string"),
        ),
    ];
    for (what, line, want) in cases {
        assert_eq!(routine_refused(&line), want, "{what}");
    }
}

#[test]
fn the_bounds_are_the_constants_and_a_list_at_its_bound_is_taken() {
    assert_eq!(LOCATORS_BOUND, 4);
    assert_eq!(RUNS_BOUND, 3);
    assert_eq!(CLEAN_BOUND, 2);
    let active = LearnEdit::parse(EDIT_ACTIVE.as_bytes()).unwrap();
    assert_eq!(active.target.locators.len(), LOCATORS_BOUND);
    assert_eq!(active.runs.len(), RUNS_BOUND);
    assert_eq!(active.clean, CLEAN_BOUND);
}

#[test]
fn a_closed_list_is_read_and_written_by_its_names_each_once() {
    let run_causes: Vec<&str> = RunCause::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        run_causes,
        [
            "nothing_to_do",
            "declined",
            "handed_back",
            "class_mismatch",
            "refused",
            "limit",
            "effect_unknown",
            "failed",
            "unknown"
        ]
    );
    for cause in RunCause::ALL {
        let line = changed(
            RUN_UNVERIFIED,
            r#""cause":"failed""#,
            &format!(r#""cause":"{}""#, cause.as_str()),
        );
        let run = LearnRun::parse(line.as_bytes()).unwrap();
        assert!(matches!(
            run.status,
            RunStatus::Ended { verdict: Verdict::Unverified(c), .. } if c == *cause
        ));
        assert_eq!(run.to_json().unwrap(), line);
    }
    let edit_causes: Vec<&str> = EditCause::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(edit_causes, ["step_failed", "class_mismatch", "origin"]);
    for cause in EditCause::ALL {
        let line = changed(
            EDIT_REVERTED,
            r#""cause":"class_mismatch""#,
            &format!(r#""cause":"{}""#, cause.as_str()),
        );
        let edit = LearnEdit::parse(line.as_bytes()).unwrap();
        assert_eq!(edit.state, EditState::Reverted(*cause));
        assert_eq!(edit.to_json().unwrap(), line);
    }
    assert_eq!(RUN_STATES, ["started", "verified", "unverified"]);
    assert_eq!(
        EDIT_STATES,
        ["provisional", "active", "reverted", "withdrawn"]
    );
    let states: Vec<&str> = [
        EditState::Provisional,
        EditState::Active,
        EditState::Reverted(EditCause::Origin),
        EditState::Withdrawn,
    ]
    .iter()
    .map(|s| s.name())
    .collect();
    assert_eq!(states, EDIT_STATES);
    let run_states: Vec<&str> = [
        RunStatus::Started,
        RunStatus::Ended {
            verdict: Verdict::Verified,
            base: None,
            assisted: 0,
            kept: 0,
            failed_step: None,
        },
        RunStatus::Ended {
            verdict: Verdict::Unverified(RunCause::Limit),
            base: None,
            assisted: 0,
            kept: 0,
            failed_step: None,
        },
    ]
    .iter()
    .map(|s| s.name())
    .collect();
    assert_eq!(run_states, RUN_STATES);
    let marks: Vec<&str> = Mark::ALL.iter().map(|m| m.as_str()).collect();
    assert_eq!(marks, ["none", "stale"]);
    for name in ["none", "stale"] {
        assert_eq!(Mark::parse(name).unwrap().as_str(), name);
    }
    for name in ["None", "stale ", "", "verified"] {
        assert!(Mark::parse(name).is_err(), "{name}");
        assert!(RunCause::parse(name).is_err(), "{name}");
        assert!(EditCause::parse(name).is_err(), "{name}");
    }
    assert!(RunCause::parse("completed").is_err());
    for mode in Mode::ALL {
        let line = changed(
            RUN_STARTED,
            r#""mode":"compile""#,
            &format!(r#""mode":"{}""#, mode.as_str()),
        );
        assert_eq!(LearnRun::parse(line.as_bytes()).unwrap().mode, *mode);
    }
}

#[test]
fn an_edit_id_is_32_lower_case_hex_digits_and_from_random_writes_its_bytes() {
    let id = EditId::from_random([0x5c; 16]);
    assert_eq!(id.as_str(), "5c".repeat(16));
    assert_eq!(id.to_string(), "5c".repeat(16));
    assert_eq!(EditId::parse(&"5c".repeat(16)).unwrap(), id);
    let ordered = EditId::from_random([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x00, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60,
        0x70,
    ]);
    assert_eq!(ordered.as_str(), "0123456789abcdef0010203040506070");
    for bad in [
        "",
        "5c",
        &"5C".repeat(16),
        &"5c".repeat(17),
        &"g0".repeat(16),
    ] {
        assert_eq!(
            EditId::parse(bad).map_err(|r| parts(&r)),
            Err(("EditId", None, "hex32")),
            "{bad}"
        );
    }
}

#[test]
fn a_refusal_prints_its_type_its_key_and_its_rule() {
    let line = changed(EDIT_EXAMPLE, r#""locators":"#, r#""args":[],"locators":"#);
    let refusal = LearnEdit::parse(line.as_bytes()).unwrap_err();
    assert_eq!(refusal.to_string(), "LearnedTarget: target: unknown_key");
    let whole = LearnRun::parse(b"[]").unwrap_err();
    assert_eq!(whole.to_string(), "LearnRun: -: not_an_object");
    let error = serde_json::from_str::<LearnRun>("[]").unwrap_err();
    assert!(
        error.to_string().starts_with("LearnRun: -: not_an_object"),
        "{error}"
    );
}

#[test]
fn a_day_is_at_least_1() {
    assert_eq!(Day::new(0), None);
    assert_eq!(Day::new(1).unwrap().get(), 1);
    assert_eq!(Day::new(u32::MAX).unwrap().get(), u32::MAX);
    assert!(day(20705) < day(20765));
    assert_eq!(serde_json::to_string(&day(20767)).unwrap(), "20767");
}

#[test]
fn a_writer_refuses_a_value_that_breaks_a_rule_and_the_error_names_it() {
    let base = LearnEdit::parse(EDIT_ACTIVE.as_bytes()).unwrap();
    let mut no_run = base.clone();
    no_run.runs.clear();
    let mut four_runs = base.clone();
    four_runs
        .runs
        .push(run_id("6c5b4a39281706f5e4d3c2b1a09f8e7d"));
    let mut twice = base.clone();
    twice.runs[2] = twice.runs[0].clone();
    let mut clean = base.clone();
    clean.clean = 3;
    let mut clean_of_runs = base.clone();
    clean_of_runs.clean = 1;
    let mut days = base.clone();
    days.first = day(20772);
    let mut no_locator = base.clone();
    no_locator.target.locators.clear();
    let mut five = base.clone();
    five.target
        .locators
        .push(locator(LocatorBy::Css, "#pay", None));
    let mut repeated = base.clone();
    repeated.target.locators[3] = repeated.target.locators[1].clone();
    let mut bad_locator = base.clone();
    bad_locator.target.locators[0].name = None;
    let cases: Vec<(&str, LearnEdit, Want)> = vec![
        ("no run", no_run, ("LearnEdit", Some("runs"), "runs_bound")),
        (
            "clean 1 with three runs",
            clean_of_runs,
            ("LearnEdit", Some("clean"), "clean_of_runs"),
        ),
        (
            "four runs",
            four_runs,
            ("LearnEdit", Some("runs"), "runs_bound"),
        ),
        (
            "a run twice",
            twice,
            ("LearnEdit", Some("runs"), "repeated_run"),
        ),
        (
            "clean 3",
            clean,
            ("LearnEdit", Some("clean"), "clean_bound"),
        ),
        (
            "first above last",
            days,
            ("LearnEdit", Some("first"), "first_after_last"),
        ),
        (
            "no locator",
            no_locator,
            ("LearnedTarget", Some("locators"), "locators_bound"),
        ),
        (
            "five locators",
            five,
            ("LearnedTarget", Some("locators"), "locators_bound"),
        ),
        (
            "a locator twice",
            repeated,
            ("LearnedTarget", Some("locators"), "repeated_locator"),
        ),
        (
            "a role locator without a name",
            bad_locator,
            ("Locator", Some("name"), "name_of_by"),
        ),
    ];
    for (what, edit, want) in cases {
        assert_eq!(edit.to_json().map_err(|r| parts(&r)), Err(want), "{what}");
        let error = serde_json::to_string(&edit).expect_err(what);
        assert!(
            error.to_string().starts_with(&format!("{}: ", want.0)),
            "{what}: {error}"
        );
    }

    let routine = LearnRoutine::parse(ROUTINE_EXAMPLE.as_bytes()).unwrap();
    let mut next = routine.clone();
    next.next = Some(day(20796));
    let mut stale = routine.clone();
    stale.mark = Mark::Stale;
    let mut seen = routine.clone();
    seen.first = None;
    seen.last = None;
    seen.next = None;
    let mut missed = seen.clone();
    missed.seen = 0;
    missed.missed = 1;
    let mut early = routine.clone();
    early.missed = 1;
    early.day = day(20765);
    let mut stale_none = routine.clone();
    stale_none.mark = Mark::Stale;
    stale_none.since = Some(day(20767));
    let cases: Vec<(&str, LearnRoutine, Want)> = vec![
        (
            "missed on the last day",
            early,
            ("LearnRoutine", Some("missed"), "missed_on_or_before_last"),
        ),
        (
            "stale with no missed cycle",
            stale_none,
            ("LearnRoutine", Some("mark"), "stale_without_missed"),
        ),
        (
            "a wrong next",
            next,
            ("LearnRoutine", Some("next"), "next_of_last_and_period"),
        ),
        (
            "stale without since",
            stale,
            ("LearnRoutine", Some("since"), "stale_without_since"),
        ),
        (
            "seen without days",
            seen,
            ("LearnRoutine", Some("seen"), "seen_without_days"),
        ),
        (
            "missed without next",
            missed,
            ("LearnRoutine", Some("missed"), "missed_without_next"),
        ),
    ];
    for (what, value, want) in cases {
        assert_eq!(value.to_json().map_err(|r| parts(&r)), Err(want), "{what}");
        assert!(serde_json::to_string(&value).is_err(), "{what}");
    }
}

#[test]
fn an_error_holds_no_value_of_the_input() {
    let marker = "zqxjmarkerzqxj";
    let lines = [
        changed(
            RUN_EXAMPLE,
            r#""kept":1,"#,
            &format!(r#""kept":1,"{marker}":1,"#),
        ),
        changed(
            RUN_EXAMPLE,
            r#""mode":"replay""#,
            &format!(r#""mode":"{marker}""#),
        ),
        changed(
            RUN_UNVERIFIED,
            r#""cause":"failed""#,
            &format!(r#""cause":"{marker}""#),
        ),
        changed(RUN_EXAMPLE, "9f8e7d6c5b4a39281706f5e4d3c2b1a0", marker),
        changed(
            EDIT_EXAMPLE,
            r#""locators":"#,
            &format!(r#""{marker}":1,"locators":"#),
        ),
        changed(
            EDIT_EXAMPLE,
            r#""by":"role""#,
            &format!(r#""by":"{marker}""#),
        ),
        changed(
            EDIT_EXAMPLE,
            r#""state":"provisional""#,
            &format!(r#""state":"{marker}""#),
        ),
        changed(
            EDIT_EXAMPLE,
            r#""name":"Submit payment""#,
            &format!(r#""name":"{marker}","{marker}":1"#),
        ),
        changed(
            EDIT_WITHDRAWN,
            r#""desc":"x""#,
            &format!(r#""desc":"{}""#, marker.repeat(80)),
        ),
        changed(
            ROUTINE_EXAMPLE,
            r#""category":"utility_bill_payment""#,
            &format!(r#""category":"{marker}""#),
        ),
        changed(
            ROUTINE_EXAMPLE,
            r#""app":"com.google.Chrome""#,
            &format!(r#""app":"/{marker}""#),
        ),
        changed(ROUTINE_EXAMPLE, "northwind-power.test", marker),
        changed(
            ROUTINE_EXAMPLE,
            r#""source":"address_field""#,
            &format!(r#""source":"{marker}""#),
        ),
        changed(
            ROUTINE_EXAMPLE,
            r#""mark":"none""#,
            &format!(r#""mark":"{marker}""#),
        ),
        format!(r#"{{"{marker}":"#),
    ];
    for line in &lines {
        let texts = [
            LearnRun::parse(line.as_bytes())
                .err()
                .map(|r| format!("{r} {r:?}")),
            LearnEdit::parse(line.as_bytes())
                .err()
                .map(|r| format!("{r} {r:?}")),
            LearnRoutine::parse(line.as_bytes())
                .err()
                .map(|r| format!("{r} {r:?}")),
            serde_json::from_str::<LearnRun>(line)
                .err()
                .map(|e| e.to_string()),
            serde_json::from_str::<LearnEdit>(line)
                .err()
                .map(|e| e.to_string()),
            serde_json::from_str::<LearnRoutine>(line)
                .err()
                .map(|e| e.to_string()),
        ];
        for text in texts {
            let text = text.expect(line);
            assert!(!text.contains(marker), "{line}: {text}");
        }
    }
}

#[test]
fn a_record_is_read_with_json_space_and_keys_in_any_order_and_written_in_the_contract_form() {
    let spaced = RUN_EXAMPLE.replace(",\"", ", \"").replace(":", ": ");
    assert_eq!(
        LearnRun::parse(spaced.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        RUN_EXAMPLE
    );
    let moved = changed(RUN_EXAMPLE, r#""v":1,"#, "").replacen('}', r#","v":1}"#, 1);
    assert!(moved.ends_with(r#""failed_step":null,"v":1}"#));
    assert_eq!(
        LearnRun::parse(moved.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        RUN_EXAMPLE
    );
}

const VERSIONS: [(&str, &str); 4] = [
    ("v 2", r#""v":2"#),
    ("v 1.0", r#""v":1.0"#),
    ("v as a string", r#""v":"1""#),
    ("v null", r#""v":null"#),
];

#[test]
fn a_run_row_with_any_v_but_the_integer_1_is_refused() {
    for line in [RUN_EXAMPLE, RUN_STARTED, RUN_UNVERIFIED] {
        for (what, v) in VERSIONS {
            assert_eq!(
                run_refused(&changed(line, r#""v":1"#, v)),
                ("LearnRun", Some("v"), "version"),
                "{what}"
            );
        }
    }
}

#[test]
fn an_edit_row_with_any_v_but_the_integer_1_is_refused() {
    for line in [EDIT_EXAMPLE, EDIT_REVERTED, EDIT_ACTIVE, EDIT_WITHDRAWN] {
        for (what, v) in VERSIONS {
            assert_eq!(
                edit_refused(&changed(line, r#""v":1"#, v)),
                ("LearnEdit", Some("v"), "version"),
                "{what}"
            );
        }
    }
}

#[test]
fn a_routine_row_with_any_v_but_the_integer_1_is_refused() {
    for line in [ROUTINE_EXAMPLE, ROUTINE_FIRST, ROUTINE_STALE] {
        for (what, v) in VERSIONS {
            assert_eq!(
                routine_refused(&changed(line, r#""v":1"#, v)),
                ("LearnRoutine", Some("v"), "version"),
                "{what}"
            );
        }
    }
}

const TARGET: &str = r#"{"desc":"link Pay now","locators":[{"by":"label","value":"Pay now","name":null},{"by":"css","value":"a.pay","name":null}]}"#;

#[test]
fn a_bare_learned_target_is_read_and_written_in_its_one_form() {
    let target = LearnedTarget::parse(TARGET.as_bytes()).unwrap();
    assert_eq!(
        target.locators,
        [
            locator(LocatorBy::Label, "Pay now", None),
            locator(LocatorBy::Css, "a.pay", None)
        ]
    );
    assert_eq!(target.to_json().unwrap(), TARGET);
    assert_eq!(serde_json::to_string(&target).unwrap(), TARGET);
    assert_eq!(
        serde_json::from_str::<LearnedTarget>(TARGET).unwrap(),
        target
    );
}

#[test]
fn a_bare_learned_target_parser_refuses_no_locator_five_one_twice_and_a_role_without_a_name() {
    let label = r#"{"by":"label","value":"Pay now","name":null}"#;
    let cases: Vec<(&str, String, Want)> = vec![
        (
            "no locator",
            r#"{"desc":"x","locators":[]}"#.to_owned(),
            ("LearnedTarget", Some("locators"), "locators_bound"),
        ),
        (
            "five locators",
            format!(r#"{{"desc":"x",{FIVE_LOCATORS}}}"#),
            ("LearnedTarget", Some("locators"), "locators_bound"),
        ),
        (
            "one locator twice",
            format!(r#"{{"desc":"x","locators":[{label},{label}]}}"#),
            ("LearnedTarget", Some("locators"), "repeated_locator"),
        ),
        (
            "a role locator without a name",
            r#"{"desc":"x","locators":[{"by":"role","value":"button","name":null}]}"#.to_owned(),
            ("Locator", Some("name"), "name_of_by"),
        ),
    ];
    for (what, line, want) in cases {
        assert_eq!(
            LearnedTarget::parse(line.as_bytes()).map_err(|r| parts(&r)),
            Err(want),
            "{what}"
        );
        let error = serde_json::from_str::<LearnedTarget>(&line).expect_err(what);
        assert!(
            error.to_string().starts_with(&format!("{}: ", want.0)),
            "{what}: {error}"
        );
    }
}

#[test]
fn a_bare_learned_target_writer_refuses_no_locator_five_one_twice_and_a_role_without_a_name() {
    let base = LearnedTarget::parse(TARGET.as_bytes()).unwrap();
    let mut none = base.clone();
    none.locators.clear();
    let mut five = base.clone();
    five.locators = vec![
        locator(LocatorBy::Role, "button", Some("Pay")),
        locator(LocatorBy::Label, "Pay", None),
        locator(LocatorBy::Text, "Pay", None),
        locator(LocatorBy::TestId, "pay", None),
        locator(LocatorBy::Css, "button.pay", None),
    ];
    let mut twice = base.clone();
    twice.locators[1] = twice.locators[0].clone();
    let mut role = base.clone();
    role.locators[0] = locator(LocatorBy::Role, "button", None);
    let cases: Vec<(&str, LearnedTarget, Want)> = vec![
        (
            "no locator",
            none,
            ("LearnedTarget", Some("locators"), "locators_bound"),
        ),
        (
            "five locators",
            five,
            ("LearnedTarget", Some("locators"), "locators_bound"),
        ),
        (
            "one locator twice",
            twice,
            ("LearnedTarget", Some("locators"), "repeated_locator"),
        ),
        (
            "a role locator without a name",
            role,
            ("Locator", Some("name"), "name_of_by"),
        ),
    ];
    for (what, target, want) in cases {
        assert_eq!(target.rules().map_err(|r| parts(&r)), Err(want), "{what}");
        assert_eq!(target.to_json().map_err(|r| parts(&r)), Err(want), "{what}");
        let error = serde_json::to_string(&target).expect_err(what);
        assert_eq!(
            error.to_string(),
            format!("{}: {}: {}", want.0, want.1.unwrap_or("-"), want.2),
            "{what}"
        );
    }
}

const ROUTINE_ONE_DAY: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":2,"day":20767,"category":"utility_bill_payment","site":{"key":"northwind-power.test","source":"address_field"},"app":"com.google.Chrome","period":null,"seen":1,"first":20767,"last":20767,"next":null,"missed":0,"mark":"none","since":null}"#;

#[test]
fn a_routine_of_one_day_is_read_and_written_back_to_the_same_bytes() {
    let routine = LearnRoutine::parse(ROUTINE_ONE_DAY.as_bytes()).unwrap();
    assert_eq!(routine.seen, 1);
    assert_eq!(routine.first, Some(day(20767)));
    assert_eq!(routine.last, Some(day(20767)));
    assert_eq!(routine.period, None);
    assert_eq!(routine.next, None);
    assert_eq!(routine.to_json().unwrap(), ROUTINE_ONE_DAY);
    assert_eq!(serde_json::to_string(&routine).unwrap(), ROUTINE_ONE_DAY);
    assert_eq!(
        serde_json::from_str::<LearnRoutine>(ROUTINE_ONE_DAY).unwrap(),
        routine
    );
    let entry = halvo_schema::learning::KINDS
        .iter()
        .find(|k| k.name == "learn_routine")
        .unwrap();
    assert_eq!(check_record(entry, ROUTINE_ONE_DAY), Ok(()));
}

#[test]
fn a_state_name_in_another_letter_case_is_refused_and_never_repaired() {
    let runs: [(&str, &str, &str); 4] = [
        (RUN_STARTED, r#""state":"started""#, r#""state":"Started""#),
        (RUN_STARTED, r#""state":"started""#, r#""state":"STARTED""#),
        (
            RUN_EXAMPLE,
            r#""state":"verified""#,
            r#""state":"Verified""#,
        ),
        (
            RUN_UNVERIFIED,
            r#""state":"unverified""#,
            r#""state":"unVerified""#,
        ),
    ];
    for (line, from, to) in runs {
        assert_eq!(
            run_refused(&changed(line, from, to)),
            ("RunState", Some("state"), "unknown_name"),
            "{to}"
        );
    }
    let edits: [(&str, &str, &str); 4] = [
        (
            EDIT_EXAMPLE,
            r#""state":"provisional""#,
            r#""state":"Provisional""#,
        ),
        (EDIT_ACTIVE, r#""state":"active""#, r#""state":"ACTIVE""#),
        (
            EDIT_REVERTED,
            r#""state":"reverted""#,
            r#""state":"Reverted""#,
        ),
        (
            EDIT_WITHDRAWN,
            r#""state":"withdrawn""#,
            r#""state":"withdrawN""#,
        ),
    ];
    for (line, from, to) in edits {
        assert_eq!(
            edit_refused(&changed(line, from, to)),
            ("EditState", Some("state"), "unknown_name"),
            "{to}"
        );
    }
}

#[test]
fn a_name_of_the_other_closed_lists_in_another_letter_case_is_refused() {
    assert_eq!(
        run_refused(&changed(
            RUN_EXAMPLE,
            r#""mode":"replay""#,
            r#""mode":"Replay""#
        )),
        ("Mode", Some("mode"), "unknown_name")
    );
    assert_eq!(
        run_refused(&changed(
            RUN_UNVERIFIED,
            r#""cause":"failed""#,
            r#""cause":"Failed""#
        )),
        ("RunCause", Some("cause"), "unknown_name")
    );
    assert_eq!(
        edit_refused(&changed(
            EDIT_REVERTED,
            r#""cause":"class_mismatch""#,
            r#""cause":"CLASS_MISMATCH""#
        )),
        ("EditCause", Some("cause"), "unknown_name")
    );
    assert_eq!(
        edit_refused(&changed(EDIT_EXAMPLE, r#""by":"role""#, r#""by":"Role""#)),
        ("LocatorBy", Some("by"), "unknown_name")
    );
    assert_eq!(
        routine_refused(&changed(
            ROUTINE_EXAMPLE,
            r#""mark":"none""#,
            r#""mark":"None""#
        )),
        ("Mark", Some("mark"), "unknown_name")
    );
    assert_eq!(
        routine_refused(&changed(
            ROUTINE_EXAMPLE,
            r#""category":"utility_bill_payment""#,
            r#""category":"Utility_bill_payment""#
        )),
        ("SeedCategory", Some("category"), "unknown_name")
    );
}

#[test]
fn a_site_source_in_another_letter_case_is_refused() {
    let sign_in = changed(
        ROUTINE_EXAMPLE,
        r#""source":"address_field""#,
        r#""source":"sign_in""#,
    );
    assert!(LearnRoutine::parse(sign_in.as_bytes()).is_ok());
    let cases: [(&str, &str, &str); 4] = [
        (
            ROUTINE_EXAMPLE,
            r#""source":"address_field""#,
            r#""source":"Address_field""#,
        ),
        (
            ROUTINE_EXAMPLE,
            r#""source":"address_field""#,
            r#""source":"ADDRESS_FIELD""#,
        ),
        (
            sign_in.as_str(),
            r#""source":"sign_in""#,
            r#""source":"Sign_in""#,
        ),
        (ROUTINE_STALE, r#""source":"title""#, r#""source":"Title""#),
    ];
    for (line, from, to) in cases {
        assert_eq!(
            routine_refused(&changed(line, from, to)),
            ("SiteValue", Some("site"), "site_form"),
            "{to}"
        );
    }
}

#[test]
fn a_key_of_each_record_object_in_another_letter_case_is_refused() {
    for (from, to) in [(r#""v":"#, r#""V":"#), (r#""state":"#, r#""State":"#)] {
        assert_eq!(
            run_refused(&changed(RUN_EXAMPLE, from, to)),
            ("LearnRun", None, "unknown_key"),
            "{to}"
        );
    }
    for (from, to) in [
        (r#""target":"#, r#""Target":"#),
        (r#""runs":"#, r#""RUNS":"#),
    ] {
        assert_eq!(
            edit_refused(&changed(EDIT_EXAMPLE, from, to)),
            ("LearnEdit", None, "unknown_key"),
            "{to}"
        );
    }
    assert_eq!(
        edit_refused(&changed(EDIT_EXAMPLE, r#""desc":"#, r#""Desc":"#)),
        ("LearnedTarget", Some("target"), "unknown_key")
    );
    assert_eq!(
        edit_refused(&changed(EDIT_EXAMPLE, r#""by":"#, r#""By":"#)),
        ("Locator", Some("locators"), "unknown_key")
    );
    for (from, to) in [(r#""mark":"#, r#""Mark":"#), (r#""app":"#, r#""APP":"#)] {
        assert_eq!(
            routine_refused(&changed(ROUTINE_EXAMPLE, from, to)),
            ("LearnRoutine", None, "unknown_key"),
            "{to}"
        );
    }
    assert_eq!(
        routine_refused(&changed(ROUTINE_EXAMPLE, r#""key":"#, r#""Key":"#)),
        ("SiteValue", Some("site"), "site_form")
    );
}

fn target_of(bytes: usize) -> String {
    let locator = |c: char, len: usize| {
        format!(
            r#"{{"by":"css","value":"{}","name":null}}"#,
            c.to_string().repeat(len)
        )
    };
    let desc = "d".repeat(1024);
    let mut locators: Vec<String> = Vec::new();
    for c in ['v', 'w', 'x', 'y'] {
        let used = format!(r#"{{"desc":"{desc}","locators":[{}]}}"#, locators.join(",")).len();
        let comma = usize::from(!locators.is_empty());
        let left = bytes - used - comma - locator(c, 0).len();
        locators.push(locator(c, left.min(1024)));
        if left <= 1024 {
            break;
        }
    }
    format!(r#"{{"desc":"{desc}","locators":[{}]}}"#, locators.join(","))
}

fn edit_with_target(target: &str) -> String {
    let example = r#"{"desc":"button \"Submit payment\"","locators":[{"by":"role","value":"button","name":"Submit payment"}]}"#;
    changed(EDIT_EXAMPLE, example, target)
}

fn target_refused() -> Want {
    ("LearnedTarget", None, "target_bytes")
}

fn row_target_refused() -> Want {
    ("LearnedTarget", Some("target"), "target_bytes")
}

#[test]
fn a_target_at_the_byte_bound_is_read_and_written_and_one_byte_more_is_refused_by_each_reader_and_writer(
) {
    assert_eq!(TARGET_BYTES, 2048);
    let at = target_of(TARGET_BYTES);
    assert_eq!(at.len(), 2048);
    let target = LearnedTarget::parse(at.as_bytes()).unwrap();
    assert_eq!(target.to_json().unwrap(), at);
    assert_eq!(serde_json::to_string(&target).unwrap(), at);
    let row = LearnEdit::parse(edit_with_target(&at).as_bytes()).unwrap();
    assert_eq!(row.to_json().unwrap(), edit_with_target(&at));

    let over = target_of(TARGET_BYTES + 1);
    assert_eq!(over.len(), 2049);
    let refusal = LearnedTarget::parse(over.as_bytes()).unwrap_err();
    assert_eq!(parts(&refusal), target_refused());
    assert_eq!(edit_refused(&edit_with_target(&over)), row_target_refused());
    let mut long = target.clone();
    long.locators[0].value = OwnText::parse(&"v".repeat(965)).unwrap();
    assert_eq!(parts(&long.to_json().unwrap_err()), target_refused());
    assert!(serde_json::to_string(&long).is_err());
    let mut long_row = row.clone();
    long_row.target = long;
    assert_eq!(parts(&long_row.to_json().unwrap_err()), target_refused());
    assert!(serde_json::to_string(&long_row).is_err());
}

#[test]
fn a_target_at_the_byte_bound_with_white_space_around_it_is_read() {
    let at = target_of(TARGET_BYTES);
    let spaced = at
        .replacen("{\"desc\":", "{ \"desc\" : ", 1)
        .replacen(",\"locators\":[", " ,\n \"locators\" : [ ", 1)
        .replacen("}]}", "} ]\n}", 1);
    assert!(spaced.len() > 2048);
    let target = LearnedTarget::parse(spaced.as_bytes()).unwrap();
    assert_eq!(target.to_json().unwrap(), at);
    let row = edit_with_target(&spaced);
    assert_eq!(
        LearnEdit::parse(row.as_bytes()).unwrap().to_json().unwrap(),
        edit_with_target(&at)
    );
}

#[test]
fn rows_of_3428_6611_and_9677_bytes_with_targets_above_the_bound_are_refused() {
    let k = |c: char| c.to_string().repeat(1024);
    let role = |value: String, name: Option<String>| serde_json::json!({"by": "role", "value": value, "name": name});
    let one = serde_json::json!({"desc": k('d'), "locators": [role(k('a'), Some(k('n')))]});
    let text = |value: String| serde_json::json!({"by": "text", "value": value, "name": null});
    let four_one_name = serde_json::json!({"desc": k('d'), "locators": [
        role(k('a'), Some(k('n'))), text(k('b')), text(k('c')), text(k('e')),
    ]});
    let four_names = serde_json::json!({"desc": k('d'), "locators": [
        role(k('a'), Some(k('n'))), role(k('b'), Some(k('n'))), role(k('c'), Some(k('n'))), role(k('e'), Some(k('n'))),
    ]});
    let mut answers = Vec::new();
    for (target, bytes) in [(one, 3428), (four_one_name, 6611), (four_names, 9677)] {
        let row = edit_with_target(&target.to_string());
        assert_eq!(row.len(), bytes);
        let answer = LearnEdit::parse(row.as_bytes())
            .map(|_| ())
            .map_err(|r| parts(&r));
        answers.push((bytes, answer));
    }
    assert_eq!(
        answers,
        [
            (3428, Err(row_target_refused())),
            (6611, Err(row_target_refused())),
            (9677, Err(row_target_refused()))
        ]
    );
}

#[test]
fn the_fullest_edit_row_that_the_writer_writes_holds_at_most_2560_bytes() {
    let id = |c: char| c.to_string().repeat(32);
    let at = LearnedTarget::parse(target_of(TARGET_BYTES).as_bytes()).unwrap();
    let longest = EditCause::ALL
        .iter()
        .copied()
        .max_by_key(|cause| cause.as_str().len())
        .unwrap();
    let row = LearnEdit {
        workflow: WorkflowId::parse(&id('a')).unwrap(),
        agent: AgentId::parse(&id('b')).unwrap(),
        edit: EditId::parse(&id('c')).unwrap(),
        rev: NonZeroU32::MAX,
        base: NonZeroU32::MAX,
        n: NonZeroU32::MAX,
        state: EditState::Reverted(longest),
        on: Some(EditId::parse(&id('d')).unwrap()),
        runs: (0..RUNS_BOUND)
            .map(|i| run_id(&format!("{:032x}", i + 1)))
            .collect(),
        first: day(u32::MAX),
        last: day(u32::MAX),
        clean: CLEAN_BOUND,
        target: at,
    };
    let json = row.to_json().unwrap();
    let others = json.len() - TARGET_BYTES;
    assert!(
        json.len() <= 2560,
        "the fullest learn_edit row has {} bytes, {} of them outside the target",
        json.len(),
        others
    );
    assert!(
        others < 512,
        "the keys outside the target take {others} bytes"
    );
    println!(
        "fullest learn_edit row: {} bytes, outside the target {} bytes",
        json.len(),
        others
    );
}
