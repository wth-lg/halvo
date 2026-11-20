use halvo_schema::executable::body::{
    CoordinatorCall, CoordinatorReply, Overlap, OverlapsReply, CHECKPOINTS_REPLY_MAX, OVERLAPS_MAX,
};
use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::text::OwnText;
use halvo_schema::executable::{Refusal, RefusalCause};
use halvo_schema::request::{
    self, write_reply, CoordinatorRole, CoordinatorWrite, Decoded, ReplyResult, REPLY_MAX,
};
use halvo_schema::{AgentId, RequestId, WorkflowId};
use serde_json::value::RawValue;

const W: &str = "30303030303030303030303030303030";
const A: &str = "40404040404040404040404040404040";
const ACCOUNT: &str = r#"{"name":"account_address","value":["me@mail.test"]}"#;
const SENDERS: &str = r#"{"name":"reply_senders","value":["ann@people.test, bob@people.test"]}"#;

fn body(slots: &str) -> String {
    format!(r#"{{"op":"list_overlaps","workflow":"{W}","agent":"{A}","slots":{slots}}}"#)
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

fn swap(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from} in {text}");
    text.replacen(from, to, 1)
}

#[test]
fn the_body_list_overlaps_reads_writes_back_and_travels_in_a_coordinator_write() {
    let slots = [
        "null".to_owned(),
        "[]".to_owned(),
        format!("[{SENDERS}]"),
        format!("[{ACCOUNT},{SENDERS}]"),
    ];
    for slots in slots {
        let text = body(&slots);
        let call =
            CoordinatorCall::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(call.op(), "list_overlaps");
        assert_eq!(call.to_json().unwrap(), text);
        let write = CoordinatorWrite::new(
            &CoordinatorRole::claim(),
            RequestId::from_random([9; 16]),
            call.clone(),
        );
        let Ok(Decoded::Raw(raw)) = request::decode(&write.to_bytes().unwrap()) else {
            panic!("{text}")
        };
        assert_eq!(
            CoordinatorCall::parse(raw.body().get().as_bytes()).unwrap(),
            call
        );
    }
}

#[test]
fn the_body_list_overlaps_names_each_slot_once_and_holds_exactly_its_four_keys() {
    expect(
        CoordinatorCall::parse(body(&format!("[{SENDERS},{ACCOUNT},{SENDERS}]")).as_bytes()),
        "ListOverlaps",
        "repeated_name",
        "slots",
    );
    for key in ["workflow", "agent", "slots"] {
        let value = match key {
            "workflow" => format!(r#","workflow":"{W}""#),
            "agent" => format!(r#","agent":"{A}""#),
            _ => r#","slots":null"#.to_owned(),
        };
        expect(
            CoordinatorCall::parse(swap(&body("null"), &value, "").as_bytes()),
            "ListOverlaps",
            "missing_key",
            key,
        );
    }
    expect(
        CoordinatorCall::parse(
            swap(
                &body("null"),
                r#""slots":null"#,
                r#""slots":null,"origins":[]"#,
            )
            .as_bytes(),
        ),
        "ListOverlaps",
        "unknown_key",
        "-",
    );
    expect(
        CoordinatorCall::parse(
            swap(
                &body("null"),
                &format!(r#""workflow":"{W}""#),
                r#""workflow":null"#,
            )
            .as_bytes(),
        ),
        "WorkflowId",
        "not_a_string",
        "workflow",
    );
    expect(
        CoordinatorCall::parse(
            swap(
                &body("null"),
                &format!(r#""agent":"{A}""#),
                r#""agent":null"#,
            )
            .as_bytes(),
        ),
        "AgentId",
        "not_a_string",
        "agent",
    );
    for slots in ["{}", "0", r#""reply_senders""#] {
        expect(
            CoordinatorCall::parse(body(slots).as_bytes()),
            "list",
            "not_a_list",
            "slots",
        );
    }
    expect(
        CoordinatorCall::parse(body(r#"[{"name":"Reply","value":[]}]"#).as_bytes()),
        "Name",
        "name_form",
        "name",
    );
}

fn other_workflow(n: usize) -> String {
    format!("{:032x}", 0x1000 + n)
}

fn other_agent(n: usize) -> String {
    format!("{:032x}", 0x2000 + n)
}

fn entry_of(workflow: &str, agent: &str, senders: &[&str]) -> String {
    let senders: Vec<String> = senders.iter().map(|s| format!("\"{s}\"")).collect();
    format!(
        r#"{{"workflow":"{workflow}","agent":"{agent}","senders":[{}]}}"#,
        senders.join(",")
    )
}

fn entry(n: usize) -> String {
    entry_of(&other_workflow(n), &other_agent(n), &["ann@people.test"])
}

fn reply_of(entries: &[String], more: &str) -> String {
    format!(r#"{{"overlaps":[{}],"more":"{more}"}}"#, entries.join(","))
}

fn reply(count: usize, more: &str) -> String {
    reply_of(&(0..count).map(entry).collect::<Vec<_>>(), more)
}

fn reads(text: &str) -> CoordinatorReply {
    let reply = CoordinatorReply::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
    assert_eq!(reply.to_json().unwrap(), text);
    reply
}

fn list_call(slots: &str) -> CoordinatorCall {
    CoordinatorCall::parse(body(slots).as_bytes()).unwrap()
}

fn refused_whole(entries: &[(String, String, Vec<&str>)], type_name: &str, rule: &str, key: &str) {
    let texts: Vec<String> = entries.iter().map(|(w, a, s)| entry_of(w, a, s)).collect();
    let text = reply_of(&texts, "no");
    expect(
        CoordinatorReply::parse(text.as_bytes()),
        type_name,
        rule,
        key,
    );
    expect(OverlapsReply::parse(text.as_bytes()), type_name, rule, key);
    let through_serde = serde_json::from_str::<CoordinatorReply>(&text)
        .unwrap_err()
        .to_string();
    assert!(
        through_serde.contains(&format!("malformed: {type_name}: {key}: {rule}")),
        "{through_serde}"
    );
    let typed = OverlapsReply {
        overlaps: entries
            .iter()
            .map(|(w, a, s)| Overlap {
                workflow: WorkflowId::parse(w).unwrap(),
                agent: AgentId::parse(a).unwrap(),
                senders: s.iter().map(|x| OwnText::parse(x).unwrap()).collect(),
            })
            .collect(),
        more: YesNo::No,
    };
    expect(typed.to_json(), type_name, rule, key);
    expect(
        CoordinatorReply::Overlaps(typed).to_json(),
        type_name,
        rule,
        key,
    );
}

#[test]
fn a_reply_of_16_entries_reads_with_more_yes_and_with_more_no() {
    let call = list_call("null");
    for more in ["yes", "no"] {
        let reply = reads(&reply(16, more));
        assert!(reply.answers(&call), "{more}");
    }
}

#[test]
fn a_reply_of_fewer_entries_reads_with_more_no_and_each_entry_names_one_or_more_senders() {
    let call = list_call(&format!("[{SENDERS}]"));
    for count in [0, 1, 2, 15] {
        assert!(reads(&reply(count, "no")).answers(&call), "{count}");
    }
    let entries = [
        entry_of(
            &other_workflow(1),
            &other_agent(9),
            &["bob@people.test", "ann@people.test"],
        ),
        entry_of(&other_workflow(2), &other_agent(3), &["ANN@People.Test"]),
    ];
    assert!(reads(&reply_of(&entries, "no")).answers(&call));
}

#[test]
fn a_reply_of_17_entries_is_refused_with_more_yes_and_with_more_no() {
    for more in ["yes", "no"] {
        expect(
            CoordinatorReply::parse(reply(17, more).as_bytes()),
            "OverlapsReply",
            "overlaps_max",
            "overlaps",
        );
    }
}

#[test]
fn a_reply_whose_entries_are_out_of_workflow_id_order_or_name_a_workflow_twice_is_refused() {
    let mut swapped: Vec<String> = (0..16).map(entry).collect();
    swapped.swap(14, 15);
    let mut first_last: Vec<String> = (0..3).map(entry).collect();
    first_last.rotate_left(1);
    let twice = [
        entry(4),
        entry_of(&other_workflow(4), &other_agent(5), &["ann@people.test"]),
    ];
    for text in [
        reply_of(&swapped, "yes"),
        reply_of(&first_last, "no"),
        reply_of(&twice, "no"),
    ] {
        expect(
            CoordinatorReply::parse(text.as_bytes()),
            "OverlapsReply",
            "workflow_order",
            "overlaps",
        );
    }
    let first_pair_out_of_order = [(1, 1), (0, 0), (2, 2)];
    let first_pair_one_workflow = [(1, 1), (1, 5), (2, 2)];
    for pairs in [first_pair_out_of_order, first_pair_one_workflow] {
        let entries: Vec<(String, String, Vec<&str>)> = pairs
            .iter()
            .map(|&(w, a)| (other_workflow(w), other_agent(a), vec!["ann@people.test"]))
            .collect();
        refused_whole(&entries, "OverlapsReply", "workflow_order", "overlaps");
    }
}

#[test]
fn a_reply_that_names_an_agent_twice_is_refused() {
    let entries = [
        entry(1),
        entry_of(&other_workflow(2), &other_agent(1), &["ann@people.test"]),
    ];
    expect(
        CoordinatorReply::parse(reply_of(&entries, "no").as_bytes()),
        "OverlapsReply",
        "repeated_name",
        "overlaps",
    );
    let apart = [
        (other_workflow(1), other_agent(1), vec!["ann@people.test"]),
        (other_workflow(2), other_agent(2), vec!["ann@people.test"]),
        (other_workflow(3), other_agent(1), vec!["ann@people.test"]),
    ];
    refused_whole(&apart, "OverlapsReply", "repeated_name", "overlaps");
}

#[test]
fn more_yes_with_fewer_than_16_entries_is_refused() {
    for count in [0, 1, 15] {
        expect(
            CoordinatorReply::parse(reply(count, "yes").as_bytes()),
            "OverlapsReply",
            "more_short",
            "more",
        );
    }
}

#[test]
fn an_entry_is_refused_with_no_sender_with_a_text_that_is_no_sender_and_with_one_sender_twice() {
    let one = |senders: &[&str]| {
        reply_of(
            &[entry_of(&other_workflow(1), &other_agent(1), senders)],
            "no",
        )
    };
    expect(
        CoordinatorReply::parse(one(&[]).as_bytes()),
        "Overlap",
        "no_sender",
        "senders",
    );
    let long = format!("{}@people.test", "a".repeat(309));
    assert_eq!(long.len(), 321);
    for text in [
        "Ann Berg",
        "ann@people.test, bob@people.test",
        " ann@people.test",
        "ann@people.test ",
        "ann",
        "@people.test",
        "ann@",
        "ann@people..test",
        "ann@.people.test",
        "ann@people.test.",
        "ann<ann@people.test>",
        long.as_str(),
    ] {
        expect(
            CoordinatorReply::parse(one(&["bob@people.test", text]).as_bytes()),
            "Overlap",
            "not_a_sender",
            "senders",
        );
    }
    expect(
        CoordinatorReply::parse(one(&["\u{e9}lan@people.test"]).as_bytes()),
        "Overlap",
        "not_a_sender",
        "senders",
    );
    for pair in [
        ["ann@people.test", "ann@people.test"],
        ["ann@people.test", "ANN@people.TEST"],
    ] {
        expect(
            CoordinatorReply::parse(one(&pair).as_bytes()),
            "Overlap",
            "sender_twice",
            "senders",
        );
    }
    let apart = [(
        other_workflow(1),
        other_agent(1),
        vec!["ann@people.test", "bob@people.test", "ANN@people.TEST"],
    )];
    refused_whole(&apart, "Overlap", "sender_twice", "senders");
    let no_sender_first = [(
        other_workflow(1),
        other_agent(1),
        vec!["Ann Berg", "bob@people.test"],
    )];
    refused_whole(&no_sender_first, "Overlap", "not_a_sender", "senders");
    let at_most = format!("{}@people.test", "a".repeat(308));
    assert!(CoordinatorReply::parse(one(&[at_most.as_str()]).as_bytes()).is_ok());
}

#[test]
fn a_reply_holds_exactly_its_two_keys_and_an_entry_its_three() {
    let text = reply(1, "no");
    expect(
        CoordinatorReply::parse(swap(&text, r#","more":"no""#, "").as_bytes()),
        "OverlapsReply",
        "missing_key",
        "more",
    );
    expect(
        CoordinatorReply::parse(
            swap(&text, r#""more":"no""#, r#""more":"no","next":null"#).as_bytes(),
        ),
        "OverlapsReply",
        "unknown_key",
        "-",
    );
    expect(
        CoordinatorReply::parse(swap(&text, r#""more":"no""#, r#""more":"maybe""#).as_bytes()),
        "YesNo",
        "unknown_name",
        "more",
    );
    expect(
        CoordinatorReply::parse(
            swap(&text, r#""overlaps":["#, r#""overlaps":{"a":"#)
                .replace("}],", "}},")
                .as_bytes(),
        ),
        "list",
        "not_a_list",
        "overlaps",
    );
    let workflow = format!(r#""workflow":"{}","#, other_workflow(0));
    expect(
        CoordinatorReply::parse(swap(&text, &workflow, "").as_bytes()),
        "Overlap",
        "missing_key",
        "workflow",
    );
    expect(
        CoordinatorReply::parse(
            swap(&text, r#""senders":["#, r#""run":null,"senders":["#).as_bytes(),
        ),
        "Overlap",
        "unknown_key",
        "overlaps",
    );
    expect(
        CoordinatorReply::parse(
            swap(
                &text,
                r#""senders":["ann@people.test"]"#,
                r#""senders":"ann@people.test""#,
            )
            .as_bytes(),
        ),
        "list",
        "not_a_list",
        "senders",
    );
}

#[test]
fn the_reply_answers_list_overlaps_alone_and_never_names_the_asking_workflow_or_agent() {
    let calls = [
        list_call("null"),
        list_call(&format!("[{ACCOUNT},{SENDERS}]")),
    ];
    let set = CoordinatorCall::parse(
        format!(r#"{{"op":"set_bindings","workflow":"{W}","agent":"{A}","slots":[{SENDERS}]}}"#)
            .as_bytes(),
    )
    .unwrap();
    let checkpoints = CoordinatorCall::parse(
        br#"{"op":"list_checkpoints","workflow":null,"agent":null,"after":null}"#,
    )
    .unwrap();
    let overlaps = reads(&reply(2, "no"));
    for call in &calls {
        assert!(overlaps.answers(call));
    }
    assert!(!overlaps.answers(&set) && !overlaps.answers(&checkpoints));
    for other in [
        r#"{"rev":1}"#,
        r#"{"state":"active"}"#,
        r#"{"ok":"yes"}"#,
        r#"{"stored":1}"#,
        r#"{"checkpoints":[],"next":null}"#,
    ] {
        let reply = CoordinatorReply::parse(other.as_bytes()).unwrap();
        assert!(calls.iter().all(|call| !reply.answers(call)), "{other}");
    }
    let own_agent = [
        entry(0),
        entry_of(&other_workflow(1), A, &["ann@people.test"]),
    ];
    let own_workflow = [entry_of(W, &other_agent(1), &["ann@people.test"])];
    for entries in [&own_agent[..], &own_workflow[..]] {
        let reply = reads(&reply_of(entries, "no"));
        assert!(calls.iter().all(|call| !reply.answers(call)));
    }
    let own_agent_first = [
        entry_of(&other_workflow(0), A, &["ann@people.test"]),
        entry(1),
    ];
    let own_workflow_first = [
        entry_of(W, &other_agent(1), &["ann@people.test"]),
        entry_of(&"f".repeat(32), &other_agent(2), &["ann@people.test"]),
    ];
    for entries in [&own_agent_first[..], &own_workflow_first[..]] {
        let reply = reads(&reply_of(entries, "no"));
        assert!(calls.iter().all(|call| !reply.answers(call)));
    }
    for call in &calls {
        let CoordinatorCall::ListOverlaps(asked) = call else {
            panic!("{}", call.op())
        };
        for entries in [&own_agent[..], &own_workflow[..]] {
            let reply = OverlapsReply::parse(reply_of(entries, "no").as_bytes()).unwrap();
            expect(
                reply.answers(asked),
                "OverlapsReply",
                "own_agent",
                "overlaps",
            );
        }
        let other = OverlapsReply::parse(reply(2, "no").as_bytes()).unwrap();
        assert_eq!(other.answers(asked), Ok(()));
    }
}

const LONG_PAD: usize = 280;

fn sender(first: char, n: usize, pad: usize) -> String {
    format!("{first}{n:06}{}@people.test", "x".repeat(pad))
}

fn reply_of_len(len: usize) -> String {
    let build = |long: usize, short: usize, pad: usize| -> String {
        let entries: Vec<String> = (0..16)
            .map(|n| {
                let mut list: Vec<String> = (0..long).map(|i| sender('s', i, LONG_PAD)).collect();
                if n == 15 {
                    list.extend(
                        (0..short).map(|i| sender('t', i, if i + 1 == short { pad } else { 0 })),
                    );
                }
                let list: Vec<&str> = list.iter().map(String::as_str).collect();
                entry_of(&other_workflow(n), &other_agent(n), &list)
            })
            .collect();
        reply_of(&entries, "yes")
    };
    assert_eq!(sender('s', 0, LONG_PAD).len(), 299);
    let long = (len / 16) / (LONG_PAD + 22) - 1;
    let short = (len - build(long, 0, 0).len()) / 22;
    let pad = len - build(long, short, 0).len();
    assert!(pad < 22, "{pad}");
    let text = build(long, short, pad);
    assert_eq!(text.len(), len);
    text
}

fn refused_by_the_bound(input: &[u8]) {
    let refusal = CoordinatorReply::parse(input).unwrap_err();
    assert_eq!(
        (refusal.cause(), refusal.type_name(), refusal.rule()),
        (RefusalCause::Length, "CoordinatorReply", "reply_bytes"),
        "{} bytes",
        input.len()
    );
}

#[test]
fn a_reply_at_the_bound_of_a_list_reply_reads_and_one_byte_more_is_refused_on_the_bytes_given() {
    let exact = reply_of_len(CHECKPOINTS_REPLY_MAX);
    assert!(reads(&exact).answers(&list_call("null")));
    let raw = RawValue::from_string(exact.clone()).unwrap();
    let id = RequestId::from_random([0x40; 16]);
    let enveloped = write_reply(Some(&id), Ok(&ReplyResult::Raw(raw))).unwrap();
    assert_eq!(enveloped.len(), REPLY_MAX);
    assert!(serde_json::from_str::<CoordinatorReply>(&exact).is_ok());
    let over = reply_of_len(CHECKPOINTS_REPLY_MAX + 1);
    refused_by_the_bound(over.as_bytes());
    let inside = exact.replacen(r#","more""#, r#", "more""#, 1);
    for padded in [format!(" {exact}"), format!("{exact}\n"), inside.clone()] {
        assert_eq!(padded.len(), CHECKPOINTS_REPLY_MAX + 1);
        refused_by_the_bound(padded.as_bytes());
    }
    let through_serde = serde_json::from_str::<CoordinatorReply>(&inside).unwrap_err();
    assert!(
        through_serde
            .to_string()
            .contains("length: CoordinatorReply: -: reply_bytes"),
        "{through_serde}"
    );
}

fn refused_by_the_writer(result: Result<String, Refusal>) {
    let refusal = result.expect_err("refused by the writer");
    assert_eq!(
        (refusal.cause(), refusal.type_name(), refusal.rule()),
        (RefusalCause::Length, "OverlapsReply", "json_length")
    );
}

#[test]
fn the_writer_refuses_a_list_that_would_not_fit_its_bound_and_never_writes_it_cut() {
    let exact = reply_of_len(CHECKPOINTS_REPLY_MAX);
    let mut reply = OverlapsReply::parse(exact.as_bytes()).unwrap();
    assert_eq!(reply.to_json().unwrap(), exact);
    let last = reply.overlaps[15].senders.last_mut().unwrap();
    *last = OwnText::parse(&format!("x{}", last.as_str())).unwrap();
    assert!(reply.rules().is_ok());
    refused_by_the_writer(reply.to_json());
    refused_by_the_writer(CoordinatorReply::Overlaps(reply.clone()).to_json());
    assert!(serde_json::to_string(&reply).is_err());
    let senders: Vec<OwnText> = (0..60)
        .map(|n| OwnText::parse(&sender('s', n, LONG_PAD)).unwrap())
        .collect();
    let wide = OverlapsReply {
        overlaps: (0..16)
            .map(|n| Overlap {
                workflow: WorkflowId::parse(&other_workflow(n)).unwrap(),
                agent: AgentId::parse(&other_agent(n)).unwrap(),
                senders: senders.clone(),
            })
            .collect(),
        more: YesNo::No,
    };
    assert!(wide.rules().is_ok());
    refused_by_the_writer(wide.to_json());
    refused_by_the_writer(CoordinatorReply::Overlaps(wide).to_json());
    for padded in [format!(" {exact}"), format!("{exact}\n")] {
        let refusal = OverlapsReply::parse(padded.as_bytes()).unwrap_err();
        assert_eq!(
            (refusal.cause(), refusal.type_name(), refusal.rule()),
            (RefusalCause::Length, "OverlapsReply", "reply_bytes")
        );
    }
    assert_eq!(OVERLAPS_MAX, 16);
}
