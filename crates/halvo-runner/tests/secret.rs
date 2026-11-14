mod support;

use halvo_schema::executable::frame::SecretReply;
use halvo_schema::executable::names::{HandbackOutcome, HandbackReason, LogEvent, Unreadable};
use halvo_schema::executable::text::SecretText;
use halvo_schema::executable::{Cause, FromRunner, Outcome, ToRunner, WebOrigin};
use halvo_schema::Category;

use support::{handback_reply, Act, Seen, Supervisor, LONG};

const TOKEN: &str = "Zq7-halvo-marker-token";

fn agent(n: u8, category: Category) -> Supervisor {
    let mut sup = Supervisor::new(n, category, Vec::new());
    sup.origins = vec![WebOrigin::parse("http://mail.agent.test:8080").expect("an origin")];
    sup
}

fn secret_refs(seen: &Seen) -> Vec<String> {
    seen.frames
        .iter()
        .filter_map(|f| match f {
            FromRunner::Secret(s) => Some(s.secret.as_str().to_string()),
            _ => None,
        })
        .collect()
}

/// Answers the secrets in order, then the handbacks in order.
fn serve(sup: &Supervisor, secrets: Vec<SecretReply>, handbacks: Vec<ToRunner>) -> Seen {
    let mut secrets = secrets.into_iter();
    let mut handbacks = handbacks.into_iter();
    sup.serve(
        |frame, _| match frame {
            FromRunner::Secret(_) => Act::With(ToRunner::SecretReply(
                secrets.next().expect("an answer left"),
            )),
            FromRunner::Handback(_) => Act::With(handbacks.next().expect("a result left")),
            _ => Act::Answer,
        },
        LONG,
    )
}

#[test]
fn unreadable_ends_the_run_failed_with_the_cause_secret_and_asks_nothing() {
    for member in [Unreadable::Locked, Unreadable::Fault] {
        let sup = agent(61, Category::Email);
        let seen = serve(&sup, vec![SecretReply::Unreadable(member)], Vec::new());
        assert_eq!(seen.done(), Some(Outcome::Failed), "{member:?}");
        assert_eq!(seen.code(), Some(0));
        assert!(
            seen.handbacks().is_empty(),
            "{member:?}: no handback, no link"
        );
        assert_eq!(secret_refs(&seen), ["graph_mail"], "{member:?}: one read");
        let lines: Vec<(LogEvent, Option<Cause>)> =
            seen.logs().iter().map(|l| (l.event, l.cause)).collect();
        assert_eq!(
            lines,
            [(LogEvent::Stopped, Some(Cause::Secret))],
            "{member:?}: no step line, so no adapter call"
        );
        let finish = seen.finish().expect("finish");
        assert_eq!((finish.handbacks, finish.prompts), (0, 0));
    }
}

#[test]
fn none_opens_account_link_once_with_the_bound_origin() {
    let sup = agent(62, Category::Email);
    let seen = serve(
        &sup,
        vec![SecretReply::None],
        vec![handback_reply(HandbackOutcome::Declined, None)],
    );
    assert_eq!(seen.handbacks(), [HandbackReason::AccountLink]);
    let origin = seen.frames.iter().find_map(|f| match f {
        FromRunner::Handback(h) => Some((h.request.origin.clone(), h.request.endpoint.clone())),
        _ => None,
    });
    assert_eq!(origin, Some((Some(sup.origins[0].clone()), None)));
    assert_eq!(seen.done(), Some(Outcome::Declined));
    assert_eq!(
        seen.logs().last().map(|l| l.cause),
        Some(Some(Cause::Declined))
    );
    assert_eq!(
        seen.finish().map(|f| (f.handbacks, f.prompts)),
        Some((1, 1))
    );
}

#[test]
fn after_a_link_the_secret_is_read_by_its_new_name_and_none_again_asks_no_second_link() {
    let sup = agent(63, Category::Email);
    let seen = serve(
        &sup,
        vec![SecretReply::None, SecretReply::None],
        vec![handback_reply(HandbackOutcome::Done, Some("fixture_token"))],
    );
    assert_eq!(secret_refs(&seen), ["graph_mail", "fixture_token"]);
    assert_eq!(seen.handbacks(), [HandbackReason::AccountLink], "once");
    assert_eq!(seen.done(), Some(Outcome::HandedBack));
}

#[test]
fn unreadable_after_a_link_ends_the_run_failed_with_the_cause_secret() {
    let sup = agent(64, Category::Email);
    let seen = serve(
        &sup,
        vec![
            SecretReply::None,
            SecretReply::Unreadable(Unreadable::Fault),
        ],
        vec![handback_reply(HandbackOutcome::Done, Some("fixture_token"))],
    );
    assert_eq!(seen.handbacks(), [HandbackReason::AccountLink]);
    assert_eq!(seen.done(), Some(Outcome::Failed));
    assert_eq!(
        seen.logs().last().map(|l| l.cause),
        Some(Some(Cause::Secret))
    );
}

#[test]
fn a_link_that_times_out_ends_the_run_handed_back_with_the_cause_timeout() {
    let sup = agent(65, Category::CalendarCleanup);
    let seen = serve(
        &sup,
        vec![SecretReply::None],
        vec![handback_reply(HandbackOutcome::Timeout, None)],
    );
    assert_eq!(secret_refs(&seen), ["graph_calendar"]);
    assert_eq!(seen.done(), Some(Outcome::HandedBack));
    assert_eq!(
        seen.logs().last().map(|l| l.cause),
        Some(Some(Cause::Timeout))
    );
}

/// The test's token as a stored secret; the test fails here when the parse refuses it.
fn token() -> SecretText {
    SecretText::parse(TOKEN).expect("the test's token parses as a stored secret")
}

#[test]
fn a_stored_secret_reaches_the_call_which_has_no_adapter_in_this_build() {
    for secrets in [
        vec![SecretReply::Stored(token())],
        vec![SecretReply::None, SecretReply::Stored(token())],
    ] {
        let linked = secrets.len() == 2;
        let sup = agent(66, Category::Email);
        let seen = serve(
            &sup,
            secrets,
            vec![
                handback_reply(HandbackOutcome::Done, Some("fixture_token")),
                handback_reply(HandbackOutcome::Done, None),
            ],
        );
        let want: &[HandbackReason] = if linked {
            &[HandbackReason::AccountLink, HandbackReason::StepFailed]
        } else {
            &[HandbackReason::StepFailed]
        };
        assert_eq!(
            seen.handbacks(),
            want,
            "the step of the call failed and was handed back"
        );
        let failed = seen
            .logs()
            .into_iter()
            .find(|l| l.event == LogEvent::StepFailed)
            .expect("a failed step");
        assert_eq!(
            (failed.step.map(|s| s.get()), failed.cause),
            (Some(1), Some(Cause::Adapter))
        );
        assert_eq!(seen.done(), Some(Outcome::HandedBack));
        assert!(
            !seen.stderr.contains(TOKEN),
            "standard error holds no secret"
        );
    }
}

/// Since U1.5 no stored secret is empty: `SecretText::parse` refuses the empty text with the rule `empty_secret`,
/// and a frame `secret_reply` reads its value through that parse. This test takes the place of
/// `an_empty_stored_secret_is_a_store_that_could_not_be_read` (CP-127).
#[test]
fn an_empty_stored_secret_is_refused_by_secret_text_parse_with_empty_secret() {
    match SecretText::parse("") {
        Ok(_) => panic!("an empty stored secret is refused"),
        Err(refusal) => assert_eq!(refusal.rule(), "empty_secret"),
    }
}
