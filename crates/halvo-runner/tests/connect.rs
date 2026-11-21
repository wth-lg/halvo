//! The runner's link to the connect adapters (A11.2) against the variants fixture server: the reads of mail and
//! calendar, the drafts of replies, the rows of answered messages (A7.4 `agent_reply`), the keys of the run's
//! list in the record of `finish`, and the sends of replies, which the tests' build alone links.

mod support;

use std::time::Duration;

use halvo_connect::{
    reply_senders, Calendar, Graph, Mail, ReplyRule, Route, RunDay, Token as ConnectToken,
};
use halvo_schema::executable::day::day_of;
use halvo_schema::executable::frame::Cancel;
use halvo_schema::executable::names::{AllowedClass, Answer, Autonomy, LogEvent, YesNo};
use halvo_schema::executable::{
    AgentRun, Cause, EffectState, FromRunner, Outcome, ReplyState, ToRunner,
};
use halvo_schema::record::Instant;

use support::connect::{
    asked, bound_to, cleanup_agent, decision, effect_rows, is_reply_row, keys_null, replies,
    reply_agent, reply_row, reply_rows_of, secret, serve_run, serve_run_holding, serve_run_then,
    start_row, Fixture, ReplyStore, ACCOUNT, SENDERS, TOKEN,
};
use support::{is_effect, is_op, load_with, now_nanos, offset_of_another_day, set_slot, Act, Seen};

/// The two senders of seed 1 as its list spells them.
const ADA: &str = "ada.vell@people.agent.test";
const BO: &str = "bo.tarrin@people.agent.test";

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
        assert_eq!(
            seen.done(),
            Some(Outcome::Declined),
            "{case}: the send step's card answered reject_once"
        );
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
fn a_run_cut_at_its_first_reply_row_leaves_one_draft_at_most_for_a_second_draft() {
    let fixture = Fixture::start();
    let sup = reply_agent(91, &fixture, ACCOUNT, SENDERS);
    fixture.reset(1, &[]);
    let mut store = ReplyStore::default();
    let mut at_first_row = None;
    let cut = serve_run_then(&sup, TOKEN, start(), &mut store, |frame| {
        if !is_op(frame, "reply_row") {
            return None;
        }
        at_first_row = Some(fixture.drafts());
        Some(Act::Close)
    });
    assert_eq!(cut.done(), None, "the run was cut");
    assert!(store.rows.is_empty(), "no row was stored");
    assert_eq!(
        at_first_row,
        Some(1),
        "one draft stands in the mail host when the first reply_row comes"
    );
    let next = serve_run(&sup, TOKEN, start(), &mut store);
    assert_eq!(replies(&next).len(), 2, "the next run drafts both messages");
    assert!(
        fixture.drafts() <= 3,
        "at most three drafts for the two messages, found {}",
        fixture.drafts()
    );
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

/// The `drafted` row of the first draft step of a message, whose draft every later row of the reply keeps.
fn first_draft(seen: &Seen, message: &str) -> String {
    let rows = reply_rows_of(seen, message);
    assert_eq!(
        rows.first()
            .map(|(seq, state, _, effect)| (*seq, *state, *effect)),
        Some((1, ReplyState::Drafted, None)),
        "{message}: the drafted row of the draft step"
    );
    rows[0].2.clone().expect("a draft")
}

/// A5.1, A7.4 `agent_reply`: the runner stores the `begun` row of a reply and holds its answer before the
/// effect's `begin` row; while the store does not answer it, no `begin` row is sent and no request of a send
/// reaches the fixture.
#[test]
fn no_byte_of_a_send_leaves_before_the_begun_row_is_answered() {
    let fixture = Fixture::start();
    fixture.reset(1, &[]);
    let sup = reply_agent(92, &fixture, ACCOUNT, SENDERS);
    let mut store = ReplyStore::default();
    let seen = serve_run_holding(
        &sup,
        TOKEN,
        start(),
        &mut store,
        |frame| {
            matches!(frame, FromRunner::Confirm(_)).then(|| decision(frame, |_| Answer::AllowOnce))
        },
        |frame| is_reply_row(frame, ReplyState::Begun),
    );
    let draft = first_draft(&seen, "s1-msg-01");
    assert_eq!(
        reply_rows_of(&seen, "s1-msg-01"),
        [
            (1, ReplyState::Drafted, Some(draft.clone()), None),
            (2, ReplyState::Begun, Some(draft), Some(0))
        ],
        "the begun row of the first reply was sent and not answered"
    );
    assert_eq!(
        effect_rows(&seen),
        [
            (0, EffectState::Held, None),
            (1, EffectState::Held, None),
            (0, EffectState::Approved, None),
            (1, EffectState::Approved, None)
        ],
        "no begin row"
    );
    assert!(fixture.sent().is_empty(), "no mail left");
    assert_eq!(
        fixture.requests().1,
        4,
        "two createReply and two PATCH, and no request of a send"
    );
}

/// A5.1, A9.3, A6.3: a reply leaves after `allow_once` and not after `reject_once`, and the card and the bound
/// `to` hold the sender as the list spelled it, not as the slot `reply_senders` spells it.
#[test]
fn a_reply_leaves_after_allow_once_and_not_after_reject_once() {
    let fixture = Fixture::start();
    fixture.reset(1, &[]);
    let senders = "ADA.VELL@People.Agent.Test,bo.tarrin@people.agent.test";
    let sup = reply_agent(93, &fixture, ACCOUNT, senders);
    let mut store = ReplyStore::default();
    let mut shown = Vec::new();
    let seen = serve_run_then(&sup, TOKEN, start(), &mut store, |frame| {
        let FromRunner::Confirm(c) = frame else {
            return None;
        };
        shown = c
            .request
            .effects
            .iter()
            .map(|e| bound_to(&e.bound))
            .collect();
        Some(decision(frame, |e| {
            if bound_to(&e.bound) == ADA {
                Answer::AllowOnce
            } else {
                Answer::RejectOnce
            }
        }))
    });
    assert_eq!(
        shown,
        [ADA, BO],
        "one card, each `to` as the list spelled it"
    );
    assert_eq!(
        effect_rows(&seen),
        [
            (0, EffectState::Held, None),
            (1, EffectState::Held, None),
            (1, EffectState::Rejected, None),
            (0, EffectState::Approved, None),
            (0, EffectState::Releasing, None),
            (0, EffectState::Committed, None)
        ]
    );
    assert_eq!(
        fixture.sent(),
        [(ADA.to_owned(), "s1-msg-01".to_owned())],
        "the allowed reply alone left, to the sender as the list spelled it"
    );
    let draft = first_draft(&seen, "s1-msg-01");
    assert_eq!(
        reply_rows_of(&seen, "s1-msg-01"),
        [
            (1, ReplyState::Drafted, Some(draft.clone()), None),
            (2, ReplyState::Begun, Some(draft.clone()), Some(0)),
            (3, ReplyState::Committed, Some(draft), Some(0))
        ]
    );
    let other = first_draft(&seen, "s1-msg-02");
    assert_eq!(
        reply_rows_of(&seen, "s1-msg-02"),
        [(1, ReplyState::Drafted, Some(other), None)],
        "the rejected reply stays drafted"
    );
    assert_eq!(seen.done(), Some(Outcome::Declined));
}

/// A5.1, the coordinator's ruling of 2026-11-21T20:16:11Z point (3): a send whose effect the policy clears does not
/// leave without an `approved` row whose digest the runner reads again; the runner sends no `begin`, ends the
/// effect, and the run ends `refused` with the cause `approval`.
#[test]
fn a_cleared_send_does_not_leave_without_an_approved_row() {
    let fixture = Fixture::start();
    fixture.reset(1, &[]);
    let mut sup = reply_agent(94, &fixture, ACCOUNT, SENDERS);
    sup.config.autonomy = Autonomy::Unattended;
    sup.config.allow = vec![AllowedClass::Send];
    sup.config.caps.untrusted = YesNo::No;
    let mut store = ReplyStore::default();
    let seen = serve_run(&sup, TOKEN, start(), &mut store);
    assert_eq!(
        effect_rows(&seen),
        [
            (0, EffectState::Cleared, None),
            (1, EffectState::Cleared, None),
            (0, EffectState::Rejected, Some(Cause::Ended))
        ],
        "both cleared, the first ended, no begin row"
    );
    assert!(!seen.tags().contains(&"confirm"), "no card");
    assert_eq!(
        replies(&seen)
            .iter()
            .map(|(m, _, state, _)| (m.as_str(), *state))
            .collect::<Vec<_>>(),
        [
            ("s1-msg-01", ReplyState::Drafted),
            ("s1-msg-02", ReplyState::Drafted)
        ],
        "no begun row"
    );
    assert!(fixture.sent().is_empty(), "no mail left");
    assert_eq!(fixture.requests().1, 4, "no request of a send");
    assert_eq!(seen.done(), Some(Outcome::Refused));
    assert_eq!(
        lines(&seen).last(),
        Some(&(LogEvent::Stopped, None, Some(Cause::Approval)))
    );
}

/// A5.2, A7.4 `agent_reply`, the coordinator's ruling CP-226 item 8 (log line 12190): a send that was not made goes
/// back to `drafted` with the same draft and that effect, for each cause that a send can meet: `not_sent` (a
/// `cancel` read under the lock before the act), `throttled` (429), after which the run goes on and the second
/// approved reply of the call leaves, and `unauthorized` (401), which also stores `step_failed` with
/// `unauthorized` and ends the run `failed` with `adapter`, as on a read.
#[test]
fn a_send_that_was_not_made_goes_back_to_drafted_with_its_draft() {
    let fixture = Fixture::start();
    let sup = reply_agent(95, &fixture, ACCOUNT, SENDERS);
    for cause in [Cause::NotSent, Cause::Throttled, Cause::Unauthorized] {
        fixture.reset(1, &[]);
        let mut store = ReplyStore::default();
        let (mut secrets, mut begins) = (0, 0);
        let seen = serve_run_then(&sup, TOKEN, start(), &mut store, |frame| {
            if matches!(frame, FromRunner::Secret(_)) {
                secrets += 1;
                let send_step = secrets == 3;
                return (cause == Cause::Unauthorized && send_step)
                    .then(|| secret("not-the-fixture-token"));
            }
            if matches!(frame, FromRunner::Confirm(_)) {
                return Some(decision(frame, |_| Answer::AllowOnce));
            }
            if !is_effect(frame, EffectState::Releasing) {
                return None;
            }
            begins += 1;
            match (cause, begins) {
                (Cause::NotSent, 1) => Some(Act::Then(
                    ToRunner::Cancel(Cancel {}),
                    Box::new(Act::Answer),
                )),
                (Cause::Throttled, 1) => {
                    fixture.throttle(1);
                    None
                }
                _ => None,
            }
        });
        let first: Vec<_> = effect_rows(&seen)
            .into_iter()
            .filter(|(at, _, _)| *at == 0)
            .map(|(_, state, row_cause)| (state, row_cause))
            .collect();
        assert_eq!(
            first,
            [
                (EffectState::Held, None),
                (EffectState::Approved, None),
                (EffectState::Releasing, None),
                (EffectState::NotMade, Some(cause))
            ],
            "{cause:?}"
        );
        let draft = first_draft(&seen, "s1-msg-01");
        assert_eq!(
            reply_rows_of(&seen, "s1-msg-01"),
            [
                (1, ReplyState::Drafted, Some(draft.clone()), None),
                (2, ReplyState::Begun, Some(draft.clone()), Some(0)),
                (3, ReplyState::Drafted, Some(draft), Some(0))
            ],
            "{cause:?}: back to drafted with the same draft and that effect"
        );
        let second = reply_rows_of(&seen, "s1-msg-02");
        let last = lines(&seen).last().cloned();
        match cause {
            Cause::NotSent => {
                assert!(fixture.sent().is_empty(), "{cause:?}: no mail left");
                assert_eq!(second.len(), 1, "{cause:?}: the second reply stays drafted");
                assert_eq!(seen.done(), Some(Outcome::Declined), "{cause:?}");
                assert_eq!(last, Some((LogEvent::Stopped, None, Some(Cause::Declined))));
            }
            Cause::Throttled => {
                assert_eq!(
                    fixture.sent(),
                    [(BO.to_owned(), "s1-msg-02".to_owned())],
                    "{cause:?}: the second approved reply of the call left after the first was not made"
                );
                assert_eq!(
                    second
                        .iter()
                        .map(|(_, state, _, effect)| (*state, *effect))
                        .collect::<Vec<_>>(),
                    [
                        (ReplyState::Drafted, None),
                        (ReplyState::Begun, Some(1)),
                        (ReplyState::Committed, Some(1))
                    ],
                    "{cause:?}"
                );
                assert_eq!(seen.done(), Some(Outcome::Failed), "{cause:?}");
                assert_eq!(last, Some((LogEvent::Stopped, None, Some(Cause::Adapter))));
            }
            _ => {
                assert!(fixture.sent().is_empty(), "{cause:?}: no mail left");
                assert_eq!(fixture.requests().2, 1, "{cause:?}: one answer 401");
                assert_eq!(second.len(), 1, "{cause:?}: the second reply stays drafted");
                let lines = lines(&seen);
                assert_eq!(
                    lines[lines.len() - 2..],
                    [
                        (LogEvent::StepFailed, Some(3), Some(Cause::Unauthorized)),
                        (LogEvent::Stopped, None, Some(Cause::Adapter))
                    ],
                    "{cause:?}: the sign of a 401 on a read"
                );
                assert_eq!(seen.done(), Some(Outcome::Failed), "{cause:?}");
            }
        }
    }
}

/// A5.1, A6.5: a sender taken off `reply_senders` while the effect is `held`, then Allow: at the load before its
/// `begin` the reply rule no longer selects the message, so the effect is ended (`rejected` with `ended`), the
/// send step's log gets `stopped` with `not_selected`, no `begun` row and no `begin` row follow, the reply row
/// stays `drafted`, and the run goes on with the other reply that the person allowed.
#[test]
fn a_sender_taken_off_the_list_while_held_ends_the_effect_and_keeps_the_draft() {
    let fixture = Fixture::start();
    fixture.reset(1, &[]);
    let sup = reply_agent(96, &fixture, ACCOUNT, SENDERS);
    let at = start();
    let mut store = ReplyStore::default();
    let mut held = false;
    let seen = serve_run_then(&sup, TOKEN, at, &mut store, |frame| {
        if matches!(frame, FromRunner::Confirm(_)) {
            held = true;
            return Some(decision(frame, |_| Answer::AllowOnce));
        }
        (held && is_op(frame, "load")).then(|| {
            load_with(&sup, frame, |l| {
                l.start = at.map(|at| start_row(&sup, at));
                set_slot(l, "reply_senders", BO);
            })
        })
    });
    assert_eq!(
        effect_rows(&seen),
        [
            (0, EffectState::Held, None),
            (1, EffectState::Held, None),
            (0, EffectState::Approved, None),
            (1, EffectState::Approved, None),
            (0, EffectState::Rejected, Some(Cause::Ended)),
            (1, EffectState::Releasing, None),
            (1, EffectState::Committed, None)
        ],
        "the first ended with no begin row, the second let through"
    );
    let draft = first_draft(&seen, "s1-msg-01");
    assert_eq!(
        reply_rows_of(&seen, "s1-msg-01"),
        [(1, ReplyState::Drafted, Some(draft), None)],
        "no begun row: the reply stays drafted"
    );
    assert!(
        lines(&seen).contains(&(LogEvent::Stopped, Some(3), Some(Cause::NotSelected))),
        "the send step's stopped row with not_selected"
    );
    assert_eq!(fixture.sent(), [(BO.to_owned(), "s1-msg-02".to_owned())]);
    assert_eq!(seen.done(), Some(Outcome::Completed));
}
