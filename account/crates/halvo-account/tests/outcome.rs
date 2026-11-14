use halvo_account::answer::{Fault, Unread};
use halvo_account::grammar::Usage;
use halvo_account::origin::{page_url, Pages};
use halvo_account::outcome::{Answer, EnvironmentFault, Failure, KeptFault, Outcome, StoreFault};
use halvo_wire::types::Period;
use std::collections::BTreeSet;

fn page() -> halvo_account::origin::PageUrl {
    page_url("http://127.0.0.1:1/x", &Pages::Loopback).expect("a loopback page")
}

fn table() -> Vec<(Outcome, u8, String, &'static str)> {
    vec![
        (
            Outcome::Answered(Answer::Result("active".into())),
            0,
            "result=active".into(),
            "answered",
        ),
        (
            Outcome::Answered(Answer::Page {
                result: "session".into(),
                printed: Some(page()),
            }),
            0,
            "result=session url=http://127.0.0.1:1/x".into(),
            "answered",
        ),
        (
            Outcome::Answered(Answer::Page {
                result: "page".into(),
                printed: None,
            }),
            0,
            "result=page".into(),
            "answered",
        ),
        (
            Outcome::Answered(Answer::Cap {
                amount: 5,
                period: Period::Week,
                spent: 1,
                held: 2,
                period_end: 3,
            }),
            0,
            "result=cap amount=5 period=week spent=1 held=2 period_end=3".into(),
            "answered",
        ),
        (
            Outcome::Answered(Answer::Delete {
                result: "deleted".into(),
                removed: true,
            }),
            0,
            "result=deleted credential=removed".into(),
            "answered",
        ),
        (
            Outcome::Answered(Answer::Delete {
                result: "pending".into(),
                removed: false,
            }),
            0,
            "result=pending credential=kept".into(),
            "answered",
        ),
        (
            Outcome::Failed(Failure::NoAnswer),
            1,
            "error=failed-call".into(),
            "no answer",
        ),
        (
            Outcome::Failed(Failure::Unread(Unread::Status(500))),
            1,
            "error=failed-call".into(),
            "status",
        ),
        (
            Outcome::Failed(Failure::Unread(Unread::Malformed(Fault::IntegerForm))),
            1,
            "error=failed-call".into(),
            "integer",
        ),
        (
            Outcome::Failed(Failure::Unread(Unread::Malformed(Fault::Shape))),
            1,
            "error=failed-call".into(),
            "malformed",
        ),
        (
            Outcome::Failed(Failure::PageRefused),
            1,
            "error=failed-call".into(),
            "list of origins",
        ),
        (Outcome::Rejected, 2, "error=rejected".into(), "rejected"),
        (
            Outcome::NoCredential,
            3,
            "error=no-credential".into(),
            "no credential",
        ),
        (
            Outcome::Store(StoreFault::Locked),
            4,
            "error=store-locked".into(),
            "refused the sign-in",
        ),
        (
            Outcome::Store(StoreFault::Fault),
            4,
            "error=store-fault".into(),
            "not known",
        ),
        (
            Outcome::Store(StoreFault::Unreadable),
            4,
            "error=store-unreadable".into(),
            "no sign-in",
        ),
        (
            Outcome::Store(StoreFault::Changed),
            4,
            "error=store-changed".into(),
            "another run",
        ),
        (
            Outcome::Store(StoreFault::NoRandom),
            4,
            "error=no-random".into(),
            "random",
        ),
        (
            Outcome::Kept {
                result: "deleted".into(),
                fault: KeptFault::Locked,
            },
            4,
            "result=deleted credential=kept error=store-locked".into(),
            "what it holds was kept",
        ),
        (
            Outcome::Kept {
                result: "no-such-account".into(),
                fault: KeptFault::Fault,
            },
            4,
            "result=no-such-account credential=kept error=store-fault".into(),
            "whether a sign-in is still stored is not known",
        ),
        (
            Outcome::Kept {
                result: "deleted".into(),
                fault: KeptFault::Unreadable,
            },
            4,
            "result=deleted credential=kept error=store-unreadable".into(),
            "an item that is no sign-in of this helper; it was kept",
        ),
        (
            Outcome::Kept {
                result: "deleted".into(),
                fault: KeptFault::Changed,
            },
            4,
            "result=deleted credential=kept error=store-changed".into(),
            "a sign-in that this run did not send; it was kept",
        ),
        (
            Outcome::Environment(EnvironmentFault::NoBackend),
            5,
            "error=no-backend".into(),
            "HALVO_ACCOUNT_BACKEND is unset",
        ),
        (
            Outcome::Environment(EnvironmentFault::BadBackend),
            5,
            "error=bad-backend".into(),
            "no origin by address",
        ),
        (
            Outcome::Environment(EnvironmentFault::NoStoreDirectory),
            5,
            "error=no-store-directory".into(),
            "HALVO_ACCOUNT_STORE_DIR is unset",
        ),
        (
            Outcome::Environment(EnvironmentFault::BadStoreDirectory),
            5,
            "error=bad-store-directory".into(),
            "no absolute directory",
        ),
        (
            Outcome::Environment(EnvironmentFault::NoClient),
            5,
            "error=no-client".into(),
            "HTTP client",
        ),
        (
            Outcome::NotOpened {
                result: "page".into(),
            },
            6,
            "result=page error=not-opened".into(),
            "could not be opened",
        ),
        (
            Outcome::Usage(Usage::Unknown),
            64,
            "error=usage".into(),
            "usage: halvo-account [--print-url]",
        ),
    ]
}

#[test]
fn each_kind_of_outcome_has_its_code_its_line_and_its_own_sentence() {
    let mut sentences = BTreeSet::new();
    let rows = table();
    for (outcome, code, line, word) in &rows {
        assert_eq!(outcome.code(), *code, "{outcome:?}");
        assert_eq!(&outcome.line(), line, "{outcome:?}");
        assert!(
            outcome.note().contains(word),
            "{outcome:?}: {}",
            outcome.note()
        );
        sentences.insert(outcome.note());
    }
    let answered = rows
        .iter()
        .filter(|(outcome, ..)| matches!(outcome, Outcome::Answered(_)))
        .count();
    let failed = rows
        .iter()
        .filter(|(outcome, ..)| matches!(outcome, Outcome::Failed(Failure::Unread(_))))
        .count();
    assert_eq!(sentences.len(), rows.len() - (answered - 1) - (failed - 3));
}
