use halvo_schema::learning::body::{LearnedRead, Refinement, Refinements};
use halvo_schema::learning::names::RefinementKind;
use halvo_schema::learning::record::Day;
use halvo_schema::learning::Refusal;
use halvo_schema::request::{
    decode, decode_reply, write_reply, CoordinatorBody, CoordinatorRole, CoordinatorWrite, Decoded,
    ReplyResult,
};
use halvo_schema::{AgentId, RequestId, WorkflowId};
use serde_json::value::RawValue;

const BODY: &str = r#"{"op":"list_refinements","workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809"}"#;

const REPLY: &str = r#"{"refinements":[{"day":20760,"what":"routine_stale"},{"day":20760,"what":"target_kept"},{"day":20767,"what":"target_learned"},{"day":20767,"what":"target_learned"},{"day":20767,"what":"target_revoked"},{"day":20770,"what":"target_kept"}]}"#;

type Want = (&'static str, Option<&'static str>, &'static str);

fn parts(refusal: &Refusal) -> Want {
    (refusal.type_name(), refusal.key(), refusal.rule())
}

fn changed(base: &str, from: &str, to: &str) -> String {
    assert_eq!(base.matches(from).count(), 1, "{from}");
    base.replacen(from, to, 1)
}

fn day(n: u32) -> Day {
    Day::new(n).unwrap()
}

fn body_refused(line: &str) -> Want {
    match LearnedRead::parse(line.as_bytes()) {
        Ok(_) => panic!("taken: {line}"),
        Err(refusal) => parts(&refusal),
    }
}

fn reply_refused(line: &str) -> Want {
    match Refinements::parse(line.as_bytes()) {
        Ok(_) => panic!("taken: {line}"),
        Err(refusal) => parts(&refusal),
    }
}

fn reply_of<B: CoordinatorBody<Reply = Refinements>>() {}

#[test]
fn the_body_is_a_coordinator_body_whose_reply_is_the_list_of_refinements() {
    reply_of::<LearnedRead>();
    let body = LearnedRead::parse(BODY.as_bytes()).unwrap();
    assert_eq!(
        body,
        LearnedRead {
            workflow: WorkflowId::parse("0f1e2d3c4b5a69788796a5b4c3d2e1f0").unwrap(),
            agent: AgentId::parse("1a2b3c4d5e6f708192a3b4c5d6e7f809").unwrap(),
        }
    );
    assert_eq!(body.to_json().unwrap(), BODY);
    assert_eq!(serde_json::to_string(&body).unwrap(), BODY);
    assert_eq!(serde_json::from_str::<LearnedRead>(BODY).unwrap(), body);
    assert_eq!(LearnedRead::OP, "list_refinements");
}

#[test]
fn a_coordinator_write_with_the_body_is_decoded_and_read_back() {
    let id = RequestId::from_random([0x2c; 16]);
    let body = LearnedRead::parse(BODY.as_bytes()).unwrap();
    let bytes = CoordinatorWrite::new(&CoordinatorRole::claim(), id.clone(), body.clone())
        .to_bytes()
        .unwrap();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        format!(
            r#"{{"v":1,"id":"{}","request":"coordinator_write","body":{BODY}}}"#,
            id.as_str()
        )
    );
    let Ok(Decoded::Raw(raw)) = decode(&bytes) else {
        panic!("not a raw request");
    };
    assert_eq!(raw.name(), "coordinator_write");
    assert_eq!(raw.id(), &id);
    assert_eq!(
        LearnedRead::parse(raw.body().get().as_bytes()).unwrap(),
        body
    );
}

#[test]
fn the_reply_reads_and_writes_its_entries_in_their_order() {
    let reply = Refinements::parse(REPLY.as_bytes()).unwrap();
    assert_eq!(reply.refinements.len(), 6);
    assert_eq!(
        reply.refinements[0],
        Refinement {
            day: day(20760),
            what: RefinementKind::RoutineStale,
        }
    );
    assert_eq!(reply.to_json().unwrap(), REPLY);
    assert_eq!(serde_json::to_string(&reply).unwrap(), REPLY);
    assert_eq!(serde_json::from_str::<Refinements>(REPLY).unwrap(), reply);
    let empty = Refinements::parse(br#"{"refinements":[]}"#).unwrap();
    assert!(empty.refinements.is_empty());
    assert_eq!(empty.to_json().unwrap(), r#"{"refinements":[]}"#);
    let one = r#"{"refinements":[{"day":20767,"what":"target_learned"}]}"#;
    assert_eq!(
        Refinements::parse(one.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        one
    );
}

#[test]
fn the_reply_goes_through_the_one_writer_and_reader_of_replies() {
    let id = RequestId::from_random([0x2c; 16]);
    let reply = Refinements::parse(REPLY.as_bytes()).unwrap();
    let raw = RawValue::from_string(reply.to_json().unwrap()).unwrap();
    let bytes = write_reply(Some(&id), Ok(&ReplyResult::Raw(raw))).unwrap();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        format!(r#"{{"v":1,"id":"{}","ok":{REPLY}}}"#, id.as_str())
    );
    let read = decode_reply(&bytes).unwrap();
    let Ok(ReplyResult::Raw(raw)) = read.outcome else {
        panic!("not a raw result");
    };
    assert_eq!(Refinements::parse(raw.get().as_bytes()).unwrap(), reply);
}

#[test]
fn the_names_of_what_are_the_closed_list_each_once() {
    let names: Vec<&str> = RefinementKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        names,
        [
            "target_learned",
            "target_kept",
            "target_revoked",
            "routine_stale"
        ]
    );
    for kind in RefinementKind::ALL {
        let line = format!(
            r#"{{"refinements":[{{"day":20767,"what":"{}"}}]}}"#,
            kind.as_str()
        );
        let reply = Refinements::parse(line.as_bytes()).unwrap();
        assert_eq!(reply.refinements[0].what, *kind);
        assert_eq!(reply.to_json().unwrap(), line);
    }
    for name in ["target_withdrawn", "Target_kept", "", "stale", "learned"] {
        assert!(RefinementKind::parse(name).is_err(), "{name}");
    }
}

#[test]
fn the_body_parser_refuses_each_form_outside_the_contract() {
    let cases: Vec<(&str, String, Want)> = vec![
        (
            "a JSON array where the body stands, its values by position",
            r#"["list_refinements","0f1e2d3c4b5a69788796a5b4c3d2e1f0","1a2b3c4d5e6f708192a3b4c5d6e7f809"]"#.to_owned(),
            ("LearnedRead", None, "not_an_object"),
        ),
        ("a key twice", changed(BODY, r#""op":"list_refinements","#, r#""op":"list_refinements","op":"list_refinements","#), ("LearnedRead", None, "repeated_key")),
        ("a key set to null where it must be absent", changed(BODY, r#""op":"list_refinements","#, r#""op":"list_refinements","run":null,"#), ("LearnedRead", None, "unknown_key")),
        ("a number as a string", changed(BODY, r#""op":"list_refinements""#, r#""op":1"#), ("LearnedRead", Some("op"), "not_a_string")),
        ("an unknown key", changed(BODY, r#""op":"list_refinements","#, r#""op":"list_refinements","zz":1,"#), ("LearnedRead", None, "unknown_key")),
        ("a key of another body", changed(BODY, r#""op":"list_refinements","#, r#""op":"list_refinements","mode":"replay","#), ("LearnedRead", None, "unknown_key")),
        ("another op", changed(BODY, "list_refinements", "list_agents"), ("LearnedRead", Some("op"), "unknown_op")),
        ("an op of the agent store", changed(BODY, "list_refinements", "create_agent"), ("LearnedRead", Some("op"), "unknown_op")),
        ("the op in another case", changed(BODY, "list_refinements", "List_refinements"), ("LearnedRead", Some("op"), "unknown_op")),
        ("no op", changed(BODY, r#""op":"list_refinements","#, ""), ("LearnedRead", Some("op"), "missing_key")),
        ("no agent", changed(BODY, r#","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809""#, ""), ("LearnedRead", Some("agent"), "missing_key")),
        ("a workflow id that is no id", changed(BODY, "0f1e2d3c4b5a69788796a5b4c3d2e1f0", "0f1e"), ("WorkflowId", Some("workflow"), "length")),
        ("an agent id as a number", changed(BODY, r#""1a2b3c4d5e6f708192a3b4c5d6e7f809""#, "12"), ("AgentId", Some("agent"), "not_a_string")),
        ("null", "null".to_owned(), ("LearnedRead", None, "not_an_object")),
        ("broken JSON", format!("{BODY},"), ("LearnedRead", None, "json")),
    ];
    for (what, line, want) in cases {
        assert_eq!(body_refused(&line), want, "{what}");
    }
}

#[test]
fn the_reply_parser_refuses_each_form_outside_the_contract() {
    let one = r#"{"refinements":[{"day":20767,"what":"target_learned"}]}"#;
    let cases: Vec<(&str, String, Want)> = vec![
        (
            "a JSON array where the reply stands",
            r#"[[{"day":20767,"what":"target_learned"}]]"#.to_owned(),
            ("Refinements", None, "not_an_object"),
        ),
        (
            "a JSON array where an entry stands, its values by position",
            changed(
                one,
                r#"{"day":20767,"what":"target_learned"}"#,
                r#"[20767,"target_learned"]"#,
            ),
            ("Refinement", Some("refinements"), "not_an_object"),
        ),
        (
            "a key twice",
            changed(
                one,
                r#""what":"target_learned""#,
                r#""what":"target_learned","day":20767"#,
            ),
            ("Refinement", Some("refinements"), "repeated_key"),
        ),
        (
            "a key set to null where it must be absent",
            changed(
                one,
                r#""what":"target_learned""#,
                r#""what":"target_learned","edit":null"#,
            ),
            ("Refinement", Some("refinements"), "unknown_key"),
        ),
        (
            "a number as a string",
            changed(one, r#""day":20767"#, r#""day":"20767""#),
            ("Day", Some("day"), "not_a_count"),
        ),
        (
            "an unknown key",
            changed(one, r#"]}"#, r#"],"next":null}"#),
            ("Refinements", None, "unknown_key"),
        ),
        (
            "a step number in an entry",
            changed(
                one,
                r#""what":"target_learned""#,
                r#""what":"target_learned","n":4"#,
            ),
            ("Refinement", Some("refinements"), "unknown_key"),
        ),
        (
            "a label in an entry",
            changed(
                one,
                r#""what":"target_learned""#,
                r#""what":"target_learned","label":"Pay""#,
            ),
            ("Refinement", Some("refinements"), "unknown_key"),
        ),
        (
            "no list",
            r#"{}"#.to_owned(),
            ("Refinements", Some("refinements"), "missing_key"),
        ),
        (
            "a list that is no list",
            r#"{"refinements":{}}"#.to_owned(),
            ("list", Some("refinements"), "not_a_list"),
        ),
        (
            "an entry without its day",
            changed(one, r#""day":20767,"#, ""),
            ("Refinement", Some("day"), "missing_key"),
        ),
        (
            "day 0",
            changed(one, r#""day":20767"#, r#""day":0"#),
            ("Day", Some("day"), "zero"),
        ),
        (
            "a name outside the list",
            changed(one, "target_learned", "target_withdrawn"),
            ("RefinementKind", Some("what"), "unknown_name"),
        ),
        (
            "a name that is no string",
            changed(one, r#""target_learned""#, "1"),
            ("RefinementKind", Some("what"), "not_a_string"),
        ),
        (
            "entries out of the order of day",
            changed(
                REPLY,
                r#"{"day":20770,"what":"target_kept"}"#,
                r#"{"day":20766,"what":"target_kept"}"#,
            ),
            ("Refinements", Some("refinements"), "order"),
        ),
        (
            "entries out of the byte order of what",
            changed(
                REPLY,
                r#"{"day":20767,"what":"target_revoked"}"#,
                r#"{"day":20767,"what":"target_kept"}"#,
            ),
            ("Refinements", Some("refinements"), "order"),
        ),
        (
            "routine_stale twice",
            changed(
                REPLY,
                r#"{"day":20770,"what":"target_kept"}"#,
                r#"{"day":20770,"what":"routine_stale"}"#,
            ),
            ("Refinements", Some("refinements"), "repeated_stale"),
        ),
        (
            "routine_stale before an entry of an earlier day",
            r#"{"refinements":[{"day":20767,"what":"routine_stale"},{"day":20760,"what":"target_kept"}]}"#.to_owned(),
            ("Refinements", Some("refinements"), "order"),
        ),
        (
            "routine_stale after an entry of a later day",
            r#"{"refinements":[{"day":20767,"what":"target_kept"},{"day":20760,"what":"routine_stale"}]}"#.to_owned(),
            ("Refinements", Some("refinements"), "order"),
        ),
        (
            "routine_stale after a target entry of the same day",
            r#"{"refinements":[{"day":20767,"what":"target_kept"},{"day":20767,"what":"routine_stale"}]}"#.to_owned(),
            ("Refinements", Some("refinements"), "order"),
        ),
        (
            "broken JSON",
            format!("{one} x"),
            ("Refinements", None, "json"),
        ),
    ];
    for (what, line, want) in cases {
        assert_eq!(reply_refused(&line), want, "{what}");
    }
}

#[test]
fn a_writer_refuses_a_reply_out_of_order_or_with_routine_stale_twice() {
    let entry = |n: u32, what: RefinementKind| Refinement { day: day(n), what };
    let cases = [
        (
            vec![
                entry(20767, RefinementKind::TargetKept),
                entry(20766, RefinementKind::TargetKept),
            ],
            "order",
        ),
        (
            vec![
                entry(20767, RefinementKind::TargetLearned),
                entry(20767, RefinementKind::TargetKept),
            ],
            "order",
        ),
        (
            vec![
                entry(20767, RefinementKind::RoutineStale),
                entry(20768, RefinementKind::RoutineStale),
            ],
            "repeated_stale",
        ),
        (
            vec![
                entry(20767, RefinementKind::TargetKept),
                entry(20760, RefinementKind::RoutineStale),
            ],
            "order",
        ),
    ];
    for (refinements, rule) in cases {
        let reply = Refinements { refinements };
        assert_eq!(
            reply.to_json().map_err(|r| parts(&r)),
            Err(("Refinements", Some("refinements"), rule))
        );
        assert!(serde_json::to_string(&reply).is_err());
    }
}

#[test]
fn an_error_of_the_body_and_the_reply_holds_no_value_of_the_input() {
    let marker = "zqxjmarkerzqxj";
    let lines = [
        changed(BODY, "list_refinements", marker),
        changed(
            BODY,
            r#""op":"list_refinements","#,
            &format!(r#""op":"list_refinements","{marker}":1,"#),
        ),
        changed(BODY, "0f1e2d3c4b5a69788796a5b4c3d2e1f0", marker),
        format!(r#"{{"refinements":[{{"day":20767,"what":"{marker}"}}]}}"#),
        format!(r#"{{"refinements":[{{"day":20767,"what":"target_kept","{marker}":1}}]}}"#),
        format!(r#"{{"{marker}":["#),
    ];
    for line in &lines {
        let texts = [
            LearnedRead::parse(line.as_bytes())
                .err()
                .map(|r| format!("{r} {r:?}")),
            Refinements::parse(line.as_bytes())
                .err()
                .map(|r| format!("{r} {r:?}")),
            serde_json::from_str::<LearnedRead>(line)
                .err()
                .map(|e| e.to_string()),
            serde_json::from_str::<Refinements>(line)
                .err()
                .map(|e| e.to_string()),
        ];
        for text in texts {
            let text = text.expect(line);
            assert!(!text.contains(marker), "{line}: {text}");
        }
    }
}
