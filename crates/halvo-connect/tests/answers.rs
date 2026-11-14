mod common;

use std::time::{Duration, Instant};

use common::stand_in::{head, inbox_of, Plan, StandIn};
use halvo_connect::{AnswerFault, CallError, Mail, ReplyRule, Token};

const BOUND: usize = 33_554_432;

fn token() -> Token {
    Token::new("stub-token-1".to_owned()).expect("a token")
}

fn list(stand_in: &StandIn, limit: Duration) -> (Result<usize, CallError>, Duration) {
    let graph = stand_in.graph(limit);
    let rule = ReplyRule::new(
        "me@home.test".to_owned(),
        vec!["ann@people.test".to_owned()],
    );
    let began = Instant::now();
    let result = graph
        .mail_list(&token(), &rule)
        .map(|messages| messages.len());
    (result, began.elapsed())
}

fn answer(head: Vec<u8>, body: Vec<u8>) -> Plan {
    Plan {
        head_after: Duration::ZERO,
        head,
        await_close: false,
        body_after: Duration::ZERO,
        body,
        hold: Duration::ZERO,
    }
}

#[test]
fn one_time_limit_holds_the_head_and_the_body() {
    let limit = Duration::from_secs(2);
    let body = br#"{"value": []}"#.to_vec();
    let stand_in = StandIn::serve(Plan {
        head_after: limit * 7 / 10,
        head: head(Some(body.len())),
        await_close: false,
        body_after: limit * 7 / 10,
        body,
        hold: Duration::ZERO,
    });
    let (result, took) = list(&stand_in, limit);
    assert_eq!(result, Err(CallError::NoAnswer));
    assert!(
        took < limit * 3 / 2,
        "the call took {took:?} with a limit of {limit:?}"
    );
}

#[test]
fn an_answer_held_open_past_the_limit_is_no_answer_within_it() {
    let limit = Duration::from_secs(1);
    for (header, body) in [
        (Vec::new(), Vec::new()),
        (head(Some(100)), br#"{"value": ["#.to_vec()),
    ] {
        let stand_in = StandIn::serve(Plan {
            head_after: Duration::ZERO,
            head: header,
            await_close: false,
            body_after: Duration::ZERO,
            body,
            hold: Duration::from_secs(10),
        });
        let (result, took) = list(&stand_in, limit);
        assert_eq!(result, Err(CallError::NoAnswer));
        assert!(
            took < limit * 3 / 2,
            "the call took {took:?} with a limit of {limit:?}"
        );
    }
}

#[test]
fn an_answer_at_the_bound_is_read_and_one_byte_more_is_refused() {
    let limit = Duration::from_secs(30);
    let at = StandIn::serve(answer(head(Some(BOUND)), inbox_of(BOUND)));
    assert_eq!(list(&at, limit).0, Ok(0), "an answer of {BOUND} bytes");
    let declared = StandIn::serve(answer(head(Some(BOUND + 1)), inbox_of(BOUND + 1)));
    assert_eq!(
        list(&declared, limit).0,
        Err(CallError::Answer(AnswerFault::Size)),
        "a declared length one byte over the bound"
    );
    let undeclared = StandIn::serve(answer(head(None), inbox_of(BOUND + 1)));
    assert_eq!(
        list(&undeclared, limit).0,
        Err(CallError::Answer(AnswerFault::Size)),
        "a body one byte over the bound with no declared length"
    );
}

#[test]
fn an_answer_of_64_mib_is_refused_before_it_is_read_whole() {
    let size = 64 << 20;
    let stand_in = StandIn::serve(Plan {
        head_after: Duration::ZERO,
        head: head(Some(size)),
        await_close: true,
        body_after: Duration::ZERO,
        body: inbox_of(size),
        hold: Duration::ZERO,
    });
    assert_eq!(
        list(&stand_in, Duration::from_secs(30)).0,
        Err(CallError::Answer(AnswerFault::Size))
    );
    let served = stand_in
        .served(Duration::from_secs(40))
        .expect("the stand-in ends");
    assert!(
        served.closed_before_body,
        "the client closed the connection within 10 s and before one byte of the body was written"
    );
}

#[test]
fn a_declared_length_over_the_bound_is_refused_before_its_body_comes() {
    let stand_in = StandIn::serve(Plan {
        head_after: Duration::ZERO,
        head: head(Some(BOUND + 1)),
        await_close: false,
        body_after: Duration::from_secs(10),
        body: inbox_of(BOUND + 1),
        hold: Duration::ZERO,
    });
    let (result, took) = list(&stand_in, Duration::from_secs(5));
    assert_eq!(result, Err(CallError::Answer(AnswerFault::Size)));
    assert!(
        took < Duration::from_secs(2),
        "the refusal waited for the body: {took:?}"
    );
}

#[test]
fn an_answer_whose_body_ends_before_its_declared_length_is_no_answer() {
    let stand_in = StandIn::serve(answer(head(Some(100)), br#"{"value": []}"#.to_vec()));
    assert_eq!(
        list(&stand_in, Duration::from_secs(5)).0,
        Err(CallError::NoAnswer)
    );
}
