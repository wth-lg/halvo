use super::{AnswerFault, BuildError, CallError, Effect};

#[test]
fn effect_of_each_error() {
    let rows = [
        (CallError::NotSent, Effect::NotMade),
        (CallError::Unauthorized, Effect::NotMade),
        (CallError::Status(429), Effect::NotMade),
        (CallError::Status(400), Effect::Unknown),
        (CallError::Status(403), Effect::Unknown),
        (CallError::Status(404), Effect::Unknown),
        (CallError::Status(409), Effect::Unknown),
        (CallError::Status(428), Effect::Unknown),
        (CallError::Status(430), Effect::Unknown),
        (CallError::Status(499), Effect::Unknown),
        (CallError::Status(100), Effect::Unknown),
        (CallError::Status(302), Effect::Unknown),
        (CallError::Status(399), Effect::Unknown),
        (CallError::Status(500), Effect::Unknown),
        (CallError::Status(503), Effect::Unknown),
        (CallError::NoAnswer, Effect::Unknown),
        (CallError::Answer(AnswerFault::Form), Effect::Unknown),
        (CallError::Answer(AnswerFault::Paged), Effect::Unknown),
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
            Effect::NotMade
        } else {
            Effect::Unknown
        };
        assert_eq!(CallError::Status(status).effect(), expected, "{status}");
    }
}

#[test]
fn texts() {
    let rows = [
        (CallError::NotSent, "the request did not leave"),
        (CallError::Unauthorized, "the provider answered 401"),
        (
            CallError::Status(503),
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
            CallError::Answer(AnswerFault::Paged),
            "the answer goes on in another page",
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
