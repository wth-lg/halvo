use super::{AnswerFault, BuildError, CallError, Effect, NotMade};

#[test]
fn effect_of_each_error() {
    let rows = [
        (CallError::NotSent, Effect::NotMade(NotMade::NotSent)),
        (
            CallError::Unauthorized,
            Effect::NotMade(NotMade::Unauthorized),
        ),
        (
            CallError::Status(429, None),
            Effect::NotMade(NotMade::Throttled),
        ),
        (CallError::Status(400, None), Effect::Unknown),
        (CallError::Status(403, None), Effect::Unknown),
        (CallError::Status(404, None), Effect::Unknown),
        (CallError::Status(409, None), Effect::Unknown),
        (CallError::Status(428, None), Effect::Unknown),
        (CallError::Status(430, None), Effect::Unknown),
        (CallError::Status(499, None), Effect::Unknown),
        (CallError::Status(100, None), Effect::Unknown),
        (CallError::Status(302, None), Effect::Unknown),
        (CallError::Status(399, None), Effect::Unknown),
        (CallError::Status(500, None), Effect::Unknown),
        (CallError::Status(503, None), Effect::Unknown),
        (CallError::NoAnswer, Effect::Unknown),
        (CallError::Answer(AnswerFault::Form), Effect::Unknown),
        (CallError::Answer(AnswerFault::Link), Effect::Unknown),
        (CallError::Answer(AnswerFault::Page), Effect::Unknown),
        (CallError::Answer(AnswerFault::Draft), Effect::Unknown),
        (CallError::Answer(AnswerFault::Repeated), Effect::Unknown),
        (CallError::Answer(AnswerFault::Id), Effect::Unknown),
        (CallError::Answer(AnswerFault::Item), Effect::Unknown),
        (CallError::Answer(AnswerFault::Size), Effect::Unknown),
    ];
    for (error, effect) in rows {
        assert_eq!(error.effect(), effect, "{error:?}");
    }
}

#[test]
fn of_the_answered_statuses_only_429_makes_nothing() {
    for status in 0..=u16::MAX {
        let expected = if status == 429 {
            Effect::NotMade(NotMade::Throttled)
        } else {
            Effect::Unknown
        };
        assert_eq!(
            CallError::Status(status, None).effect(),
            expected,
            "{status}"
        );
    }
}

#[test]
fn a_request_that_did_not_leave_is_not_made_as_not_sent() {
    assert_eq!(
        CallError::NotSent.effect(),
        Effect::NotMade(NotMade::NotSent)
    );
}

#[test]
fn an_answer_401_is_not_made_as_unauthorized() {
    assert_eq!(
        CallError::Unauthorized.effect(),
        Effect::NotMade(NotMade::Unauthorized)
    );
}

#[test]
fn an_answer_429_is_not_made_as_throttled() {
    assert_eq!(
        CallError::Status(429, None).effect(),
        Effect::NotMade(NotMade::Throttled)
    );
}

#[test]
fn every_other_answered_status_stays_unknown() {
    for status in (0..=u16::MAX).filter(|status| *status != 429) {
        assert_eq!(
            CallError::Status(status, None).effect(),
            Effect::Unknown,
            "{status}"
        );
    }
}

#[test]
fn texts() {
    let rows = [
        (CallError::NotSent, "the request did not leave"),
        (CallError::Unauthorized, "the provider answered 401"),
        (
            CallError::Status(503, None),
            "the provider answered the status 503",
        ),
        (
            CallError::NoAnswer,
            "no whole answer came within the time limit",
        ),
        (
            CallError::Answer(AnswerFault::Form),
            "the answer is not of the form of the face",
        ),
        (
            CallError::Answer(AnswerFault::Link),
            "the answer links to a page of another place",
        ),
        (
            CallError::Answer(AnswerFault::Page),
            "the answer holds more items than the page size",
        ),
        (
            CallError::Answer(AnswerFault::Draft),
            "the answer of createReply is not a draft",
        ),
        (
            CallError::Answer(AnswerFault::Repeated),
            "the answer holds one id twice",
        ),
        (
            CallError::Answer(AnswerFault::Id),
            "the answer holds an id that is empty, . or ..",
        ),
        (
            CallError::Answer(AnswerFault::Item),
            "the answer names another item than the request",
        ),
        (
            CallError::Answer(AnswerFault::Size),
            "the answer is longer than 33554432 bytes",
        ),
    ];
    for (error, text) in rows {
        assert_eq!(error.to_string(), text);
    }
    assert_eq!(
        BuildError::Cleartext.to_string(),
        "an http origin needs the loopback route"
    );
    assert_eq!(
        BuildError::Client.to_string(),
        "the TLS configuration or the HTTP client could not be built"
    );
    assert_eq!(
        BuildError::Address.to_string(),
        "the loopback route maps a host name, and the host of this origin is an address"
    );
}
