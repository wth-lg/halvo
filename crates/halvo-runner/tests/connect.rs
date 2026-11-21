//! The runner's link to the connect adapters (A11.2) against the variants fixture server: the reads of mail and
//! calendar, the drafts of replies, the rows of answered messages (A7.4 `agent_reply`) and the keys of the run's
//! list in the record of `finish`.

mod support;

use std::time::Duration;

use halvo_connect::{
    reply_senders, Calendar, Graph, Mail, ReplyRule, Route, RunDay, Token as ConnectToken,
};
use halvo_schema::executable::day::day_of;
use halvo_schema::executable::names::{LogEvent, YesNo};
use halvo_schema::executable::{AgentRun, Cause, Outcome, ReplyState};
use halvo_schema::record::Instant;

use support::connect::{
    asked, cleanup_agent, keys_null, replies, reply_agent, reply_row, serve_run, Fixture,
    ReplyStore, ACCOUNT, SENDERS, TOKEN,
};
use support::{now_nanos, offset_of_another_day, Seen};

/// The log lines of a run, each as its event, its step and its cause.
fn lines(seen: &Seen) -> Vec<(LogEvent, Option<u32>, Option<Cause>)> {
    seen.logs()
        .iter()
        .map(|l| (l.event, l.step.map(|s| s.get()), l.cause))
        .collect()
}

/// A start instant before the fixture's messages, which the fixture stamps from its reset on.
fn start() -> Option<Instant> {
    Some(Instant(now_nanos()))
}

fn finish(seen: &Seen) -> AgentRun {
    seen.finish().expect("the run's record")
}

#[test]
fn a_401_on_a_read_ends_the_run_failed() {
    let fixture = Fixture::start();
    let reply = reply_agent(81, &fixture, ACCOUNT, SENDERS);
    let cleanup = cleanup_agent(82, &fixture);
    for sup in [&reply, &cleanup] {
        fixture.reset(1, &[]);
        let mut store = ReplyStore::default();
        let seen = serve_run(sup, "not-the-fixture-token", start(), &mut store);
        let category = sup.config.category;
        assert_eq!(seen.done(), Some(Outcome::Failed), "{category:?}");
        assert!(
            seen.handbacks().is_empty(),
            "{category:?}: no handback, no link"
        );
        assert_eq!(
            lines(&seen),
            [
                (LogEvent::StepFailed, Some(1), Some(Cause::Unauthorized)),
                (LogEvent::Stopped, None, Some(Cause::Adapter)),
            ],
            "{category:?}"
        );
        assert!(keys_null(&finish(&seen)), "{category:?}: the keys are null");
        assert!(asked(&seen).is_empty(), "{category:?}: no rows read");
        assert_eq!(
            fixture.requests(),
            (1, 0, 1),
            "{category:?}: one request, answered 401, and no further request"
        );
    }
}

#[test]
fn an_answered_message_is_not_selected_again() {
    let fixture = Fixture::start();
    let sup = reply_agent(83, &fixture, ACCOUNT, SENDERS);
    let old = Some("draft-of-an-earlier-run");
    let cases: [(&str, Vec<ReplyState>); 6] = [
        ("begun", vec![ReplyState::Drafted, ReplyState::Begun]),
        (
            "committed",
            vec![
                ReplyState::Drafted,
                ReplyState::Begun,
                ReplyState::Committed,
            ],
        ),
        (
            "unknown",
            vec![ReplyState::Drafted, ReplyState::Begun, ReplyState::Unknown],
        ),
        ("withheld", vec![ReplyState::Drafted, ReplyState::Withheld]),
        ("blocked", vec![ReplyState::Drafted]),
        ("blocked_withheld", vec![ReplyState::Drafted]),
    ];
    for (case, own) in cases {
        fixture.reset(1, &[]);
        let mut store = ReplyStore::default();
        for (at, state) in own.into_iter().enumerate() {
            let seq = u32::try_from(at + 1).expect("a seq");
            store
                .rows
                .push(reply_row(&sup, "s1-msg-01", seq, state, old));
        }
        match case {
            "blocked" => store.blocked.push("s1-msg-01".to_owned()),
            "blocked_withheld" => store.blocked_withheld.push("s1-msg-01".to_owned()),
            _ => {}
        }
        let seen = serve_run(&sup, TOKEN, start(), &mut store);
        assert_eq!(
            asked(&seen),
            [vec!["s1-msg-01"], vec!["s1-msg-02", "s1-msg-07"]],
            "{case}: the rows of each page are read"
        );
        let rows = replies(&seen);
        assert_eq!(
            rows.iter()
                .map(|(m, seq, state, _)| (m.as_str(), *seq, *state))
                .collect::<Vec<_>>(),
            [("s1-msg-02", 1, ReplyState::Drafted)],
            "{case}: a row for the other message alone"
        );
        assert_eq!(
            (fixture.drafts(), fixture.requests().1),
            (1, 2),
            "{case}: one createReply and one PATCH, both for the other message"
        );
        let record = finish(&seen);
        let other_task = u32::from(case == "blocked");
        let held_earlier = u32::from(matches!(case, "withheld" | "blocked_withheld"));
        assert_eq!(
            (record.other_task, record.held_earlier),
            (Some(other_task), Some(held_earlier)),
            "{case}"
        );
        assert_eq!(seen.done(), Some(Outcome::Failed), "{case}: the send step");
    }
}

#[test]
fn the_next_run_makes_no_second_draft() {
    let fixture = Fixture::start();
    let sup = reply_agent(84, &fixture, ACCOUNT, SENDERS);
    fixture.reset(1, &[]);
    let mut store = ReplyStore::default();

    let first = serve_run(&sup, TOKEN, start(), &mut store);
    let rows = replies(&first);
    let drafts: Vec<String> = rows
        .iter()
        .map(|(_, _, _, d)| d.clone().expect("a draft"))
        .collect();
    assert_eq!(
        rows.iter()
            .map(|(m, seq, state, _)| (m.as_str(), *seq, *state))
            .collect::<Vec<_>>(),
        [
            ("s1-msg-01", 1, ReplyState::Drafted),
            ("s1-msg-02", 1, ReplyState::Drafted)
        ]
    );
    assert_eq!((fixture.drafts(), fixture.requests().1), (2, 4));
    assert_eq!(finish(&first).held_recipient, Some(0));

    let second = serve_run(&sup, TOKEN, start(), &mut store);
    assert!(replies(&second).is_empty(), "no row: the drafts stand");
    assert_eq!(
        (fixture.drafts(), fixture.requests().1),
        (2, 6),
        "one PATCH for each draft and no createReply"
    );

    let mut moved = ReplyStore::default();
    moved.rows.push(reply_row(
        &sup,
        "s1-msg-01",
        1,
        ReplyState::Drafted,
        Some("draft-that-is-gone"),
    ));
    moved.rows.push(reply_row(
        &sup,
        "s1-msg-02",
        1,
        ReplyState::Drafted,
        Some(drafts[0].as_str()),
    ));
    let third = serve_run(&sup, TOKEN, start(), &mut moved);
    assert_eq!(
        replies(&third),
        [
            ("s1-msg-01".to_owned(), 2, ReplyState::Lost, None),
            ("s1-msg-02".to_owned(), 2, ReplyState::Lost, None)
        ],
        "a PATCH answered 404, and a draft whose recipient is not the message's sender"
    );
    assert_eq!(
        (fixture.drafts(), fixture.requests().1),
        (2, 8),
        "two PATCHes and no createReply"
    );

    fixture.reset(1, &["draft_copy"]);
    let mut held = ReplyStore::default();
    let fourth = serve_run(&sup, TOKEN, start(), &mut held);
    let rows = replies(&fourth);
    assert_eq!(
        rows.iter()
            .map(|(m, seq, state, _)| (m.as_str(), *seq, *state))
            .collect::<Vec<_>>(),
        [
            ("s1-msg-01", 1, ReplyState::Drafted),
            ("s1-msg-01", 2, ReplyState::Withheld),
            ("s1-msg-02", 1, ReplyState::Drafted),
            ("s1-msg-02", 2, ReplyState::Withheld)
        ]
    );
    assert!(
        rows[0].3.is_some() && rows[0].3 == rows[1].3 && rows[2].3 == rows[3].3,
        "a withheld row keeps its draft"
    );
    assert_eq!(finish(&fourth).held_recipient, Some(2));

    fixture.reset(1, &["draft_not_yet"]);
    let mut not_yet = ReplyStore::default();
    let fifth = serve_run(&sup, TOKEN, start(), &mut not_yet);
    assert_eq!(
        replies(&fifth)
            .iter()
            .map(|(m, seq, state, _)| (m.as_str(), *seq, *state))
            .collect::<Vec<_>>(),
        [
            ("s1-msg-01", 1, ReplyState::Drafted),
            ("s1-msg-02", 1, ReplyState::Drafted)
        ],
        "a draft that is no draft yet stays drafted and is not held"
    );
    assert_eq!(finish(&fifth).held_recipient, Some(0));
}

#[test]
fn a_padded_account_selects_no_message_and_the_plain_one_selects() {
    let fixture = Fixture::start();
    fixture.reset(1, &[]);
    let padded = reply_agent(85, &fixture, &format!(" {ACCOUNT}"), SENDERS);
    let plain = reply_agent(86, &fixture, ACCOUNT, SENDERS);

    let mut store = ReplyStore::default();
    let seen = serve_run(&padded, TOKEN, start(), &mut store);
    assert!(replies(&seen).is_empty(), "no draft for the padded account");
    assert_eq!(fixture.requests().1, 0, "no createReply and no PATCH");
    let record = finish(&seen);
    assert_eq!(
        (record.not_to_account, record.reply_to_other),
        (Some(3), Some(0))
    );

    let seen = serve_run(&plain, TOKEN, start(), &mut store);
    assert_eq!(
        replies(&seen)
            .iter()
            .map(|(m, _, state, _)| (m.as_str(), *state))
            .collect::<Vec<_>>(),
        [
            ("s1-msg-01", ReplyState::Drafted),
            ("s1-msg-02", ReplyState::Drafted)
        ]
    );
    assert_eq!(fixture.requests().1, 4);
    let record = finish(&seen);
    assert_eq!(
        (record.not_to_account, record.reply_to_other),
        (Some(0), Some(1))
    );
}

#[test]
fn no_start_or_unread_senders_send_no_list_request() {
    let fixture = Fixture::start();
    let cases = [
        (
            reply_agent(87, &fixture, ACCOUNT, SENDERS),
            None,
            Cause::NoStart,
        ),
        (
            reply_agent(88, &fixture, ACCOUNT, "ada.vell people.agent.test"),
            start(),
            Cause::SendersUnread,
        ),
    ];
    for (sup, at, cause) in cases {
        fixture.reset(1, &[]);
        let mut store = ReplyStore::default();
        let seen = serve_run(&sup, TOKEN, at, &mut store);
        assert_eq!(
            lines(&seen),
            [(LogEvent::Stopped, Some(1), Some(cause))],
            "{cause:?}: the stopped row of the list step"
        );
        assert_eq!(seen.done(), Some(Outcome::Failed), "{cause:?}");
        assert!(keys_null(&finish(&seen)), "{cause:?}: the keys are null");
        assert_eq!(fixture.requests().0, 0, "{cause:?}: no request");
    }
}

#[test]
fn a_clean_up_stores_its_calendar_counts() {
    let fixture = Fixture::start();
    fixture.reset(1, &[]);
    let token = || ConnectToken::new(TOKEN.to_owned()).expect("a token");
    let within = Duration::from_secs(10);

    let mut sup = cleanup_agent(89, &fixture);
    sup.utc_offset_s = offset_of_another_day();
    let seen = serve_run(&sup, TOKEN, None, &mut ReplyStore::default());
    let day = day_of(Instant(now_nanos()), sup.utc_offset_s);
    let graph = Graph::new(&fixture.origin("calendar"), Route::Loopback, within).expect("a client");
    let counts = graph
        .calendar_list(&token(), RunDay::from_unix_seconds(u64::from(day) * 86_400))
        .expect("the adapter's answer")
        .counts();
    assert!(
        counts.organized > 0,
        "the seed holds an event that the person organized"
    );
    let record = finish(&seen);
    assert_eq!(
        (
            record.cut,
            record.no_key,
            record.organized,
            record.incomplete,
            record.more_pages,
            record.earlier_page
        ),
        (
            Some(counts.cut),
            Some(counts.no_key),
            Some(counts.organized),
            Some(counts.incomplete),
            Some(counts.more_pages),
            Some(counts.earlier_page)
        )
    );
    assert_eq!(
        (
            record.before_start,
            record.not_to_account,
            record.reply_to_other,
            record.held_recipient,
            record.other_task,
            record.held_earlier
        ),
        (None, None, None, None, None, None),
        "the keys that a clean-up cannot give"
    );
    assert_eq!(
        lines(&seen),
        [
            (LogEvent::StepOk, Some(1), None),
            (LogEvent::StepFailed, Some(2), Some(Cause::Adapter)),
            (LogEvent::Stopped, None, Some(Cause::Adapter)),
        ],
        "the delete step fails as at the base"
    );

    let reply = reply_agent(90, &fixture, ACCOUNT, SENDERS);
    let at = start();
    let seen = serve_run(&reply, TOKEN, at, &mut ReplyStore::default());
    let rule = ReplyRule::new(
        ACCOUNT.to_owned(),
        reply_senders(SENDERS).expect("the senders"),
        at.expect("a start"),
    );
    let graph = Graph::new(&fixture.origin("mail"), Route::Loopback, within).expect("a client");
    let counts = graph
        .mail_list(&token(), &rule)
        .expect("the adapter's answer")
        .counts();
    let record = finish(&seen);
    assert_eq!(
        (
            record.before_start,
            record.cut,
            record.no_key,
            record.not_to_account,
            record.incomplete,
            record.reply_to_other,
            record.more_pages,
            record.earlier_page
        ),
        (
            Some(counts.before_start),
            Some(counts.cut),
            Some(counts.no_key),
            Some(counts.not_to_account),
            Some(counts.incomplete),
            Some(counts.reply_to_other),
            Some(counts.more_pages),
            Some(counts.earlier_page)
        )
    );
    assert_eq!(
        record.reply_to_other,
        Some(1),
        "the seed's message with another reply-to"
    );
    assert_eq!(
        (record.organized, record.held_recipient, record.cut),
        (None, Some(0), Some(YesNo::No))
    );
}
