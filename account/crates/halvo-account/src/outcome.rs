use crate::answer::{Fault, Unread};
use crate::grammar::Usage;
use crate::origin::PageUrl;
use halvo_wire::types::Period;

/// What one run of the helper came to. It prints one line on standard output, one sentence
/// on standard error, and exits with the code of its kind:
///
/// | code | kind | line |
/// |---|---|---|
/// | 0 | the backend answered with a result of the route | `result=R`, with more pairs for some results; after `delete` it is `result=R credential=removed` or `result=R credential=kept` |
/// | 1 | a failed call: no answer, an answer that was not read, or a page outside the list | `error=failed-call` |
/// | 2 | the backend rejected the request | `error=rejected` |
/// | 3 | no credential is stored, and nothing was sent | `error=no-credential` |
/// | 4 | the store cannot be used, and nothing was sent | `error=store-locked`, `error=store-fault`, `error=store-unreadable`, `error=store-changed` or `error=no-random` |
/// | 4 | the backend answered `deleted` or `no-such-account`, and the stored item stays: at the second read or at the delete the store answered `Locked` or failed, or the second read found a sign-in that this run did not send or an item of another form | `result=R credential=kept error=store-locked`, `result=R credential=kept error=store-fault`, `result=R credential=kept error=store-changed` or `result=R credential=kept error=store-unreadable` |
/// | 5 | a debug build lacks a usable `HALVO_ACCOUNT_BACKEND` or `HALVO_ACCOUNT_STORE_DIR`, and nothing was sent | `error=no-backend`, `error=bad-backend`, `error=no-store-directory` or `error=bad-store-directory` |
/// | 6 | the page that the backend answered could not be opened | `result=R error=not-opened` |
/// | 64 | the arguments are outside the grammar, and nothing was sent | `error=usage` |
#[derive(Debug)]
pub enum Outcome {
    Answered(Answer),
    Failed(Failure),
    Rejected,
    NoCredential,
    Store(StoreFault),
    Kept { result: String, fault: KeptFault },
    Environment(EnvironmentFault),
    NotOpened { result: String },
    Usage(Usage),
}

/// An answer of the backend, as the line on standard output carries it.
#[derive(Debug)]
pub enum Answer {
    /// `result=R`.
    Result(String),
    /// `result=R`, and `url=URL` when `--print-url` asked for it; without it the page was opened.
    /// No test of this crate hands a page to the built binary: each test run of it that names
    /// `subscribe` or `manage` passes `--print-url` first, and the one function of the tests
    /// that starts it refuses a run that does not.
    Page {
        result: String,
        printed: Option<PageUrl>,
    },
    /// `result=cap amount=A period=P spent=S held=H period_end=T`.
    Cap {
        amount: u64,
        period: Period,
        spent: i64,
        held: i64,
        period_end: i64,
    },
    /// `result=R credential=removed` or `result=R credential=kept`, the answer of `delete`.
    Delete { result: String, removed: bool },
}

/// Why a call failed.
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// The transport got no answer.
    NoAnswer,
    /// An answer came and was not read.
    Unread(Unread),
    /// The page that the answer names is not on the compiled list of origins.
    PageRefused,
}

/// Why the store cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreFault {
    /// The store answered `Locked`: a credential may be stored and cannot be used.
    Locked,
    /// The system failed; whether a credential is stored is not known.
    Fault,
    /// The store holds something that is no sign-in of this helper.
    Unreadable,
    /// A credential appeared between the read and the write.
    Changed,
    /// The system gave no random bytes.
    NoRandom,
}

/// Why a delete that the backend answered with `deleted` or `no-such-account` left the stored
/// item in place. `delete` reads the store a second time after the answer and removes the item
/// only when it holds the sign-in that this run sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeptFault {
    /// The store answered `Locked` to the second read or to the delete.
    Locked,
    /// The system failed at the second read or at the delete.
    Fault,
    /// The second read found an item that is no sign-in of this helper.
    Unreadable,
    /// The second read found a sign-in that this run did not send.
    Changed,
}

/// Why a debug build cannot reach its backend or its store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentFault {
    NoBackend,
    BadBackend,
    NoStoreDirectory,
    BadStoreDirectory,
    /// The HTTP client could not be built.
    NoClient,
}

impl StoreFault {
    fn word(self) -> &'static str {
        match self {
            StoreFault::Locked => "store-locked",
            StoreFault::Fault => "store-fault",
            StoreFault::Unreadable => "store-unreadable",
            StoreFault::Changed => "store-changed",
            StoreFault::NoRandom => "no-random",
        }
    }
}

impl KeptFault {
    fn word(self) -> &'static str {
        match self {
            KeptFault::Locked => StoreFault::Locked.word(),
            KeptFault::Fault => StoreFault::Fault.word(),
            KeptFault::Unreadable => StoreFault::Unreadable.word(),
            KeptFault::Changed => StoreFault::Changed.word(),
        }
    }
}

impl EnvironmentFault {
    fn word(self) -> &'static str {
        match self {
            EnvironmentFault::NoBackend => "no-backend",
            EnvironmentFault::BadBackend => "bad-backend",
            EnvironmentFault::NoStoreDirectory => "no-store-directory",
            EnvironmentFault::BadStoreDirectory => "bad-store-directory",
            EnvironmentFault::NoClient => "no-client",
        }
    }
}

impl Outcome {
    /// The exit code of the table above.
    pub fn code(&self) -> u8 {
        match self {
            Outcome::Answered(_) => 0,
            Outcome::Failed(_) => 1,
            Outcome::Rejected => 2,
            Outcome::NoCredential => 3,
            Outcome::Store(_) | Outcome::Kept { .. } => 4,
            Outcome::Environment(_) => 5,
            Outcome::NotOpened { .. } => 6,
            Outcome::Usage(_) => 64,
        }
    }

    /// The line on standard output, without its line end.
    pub fn line(&self) -> String {
        match self {
            Outcome::Answered(Answer::Result(result)) => format!("result={result}"),
            Outcome::Answered(Answer::Page { result, printed }) => match printed {
                Some(url) => format!("result={result} url={}", url.as_str()),
                None => format!("result={result}"),
            },
            Outcome::Answered(Answer::Cap {
                amount,
                period,
                spent,
                held,
                period_end,
            }) => format!(
                "result=cap amount={amount} period={period} spent={spent} held={held} period_end={period_end}"
            ),
            Outcome::Answered(Answer::Delete { result, removed }) => format!(
                "result={result} credential={}",
                if *removed { "removed" } else { "kept" }
            ),
            Outcome::Failed(_) => "error=failed-call".to_string(),
            Outcome::Rejected => "error=rejected".to_string(),
            Outcome::NoCredential => "error=no-credential".to_string(),
            Outcome::Store(fault) => format!("error={}", fault.word()),
            Outcome::Kept { result, fault } => {
                format!("result={result} credential=kept error={}", fault.word())
            }
            Outcome::Environment(fault) => format!("error={}", fault.word()),
            Outcome::NotOpened { result } => format!("result={result} error=not-opened"),
            Outcome::Usage(_) => "error=usage".to_string(),
        }
    }

    /// The sentence on standard error.
    pub fn note(&self) -> &'static str {
        match self {
            Outcome::Answered(_) => "the backend answered",
            Outcome::Failed(Failure::NoAnswer) => "the call failed: no answer came; nothing more was sent",
            Outcome::Failed(Failure::Unread(Unread::Status(_))) => "the call failed: the answer has a status other than 200 or 4xx; nothing more was sent",
            Outcome::Failed(Failure::Unread(Unread::Malformed(Fault::IntegerForm))) => "the call failed: the answer writes an integer otherwise than as digits; nothing more was sent",
            Outcome::Failed(Failure::Unread(Unread::Malformed(_))) => "the call failed: the answer is malformed; nothing more was sent",
            Outcome::Failed(Failure::PageRefused) => "the call failed: the page is not on the list of origins that this build opens; nothing was opened",
            Outcome::Rejected => "the backend rejected the request; nothing more was sent",
            Outcome::NoCredential => "no credential is stored; nothing was sent",
            Outcome::Store(StoreFault::Locked) => "the store answered locked and refused the sign-in; nothing was sent and nothing was stored",
            Outcome::Store(StoreFault::Fault) => "the store failed; whether a sign-in is stored is not known; nothing was sent",
            Outcome::Store(StoreFault::Unreadable) => "the store holds something that is no sign-in of this helper; nothing was sent",
            Outcome::Store(StoreFault::Changed) => "a sign-in was stored by another run; nothing was sent",
            Outcome::Store(StoreFault::NoRandom) => "the system gave no random bytes; nothing was stored and nothing was sent",
            Outcome::Kept { fault: KeptFault::Locked, .. } => "the backend answered, and the store answered locked; what it holds was kept",
            Outcome::Kept { fault: KeptFault::Fault, .. } => "the backend answered, and the store failed; whether a sign-in is still stored is not known",
            Outcome::Kept { fault: KeptFault::Unreadable, .. } => "the backend answered, and the store now holds an item that is no sign-in of this helper; it was kept",
            Outcome::Kept { fault: KeptFault::Changed, .. } => "the backend answered, and the store now holds a sign-in that this run did not send; it was kept",
            Outcome::Environment(EnvironmentFault::NoBackend) => "HALVO_ACCOUNT_BACKEND is unset; nothing was sent",
            Outcome::Environment(EnvironmentFault::BadBackend) => "HALVO_ACCOUNT_BACKEND is no origin by address, such as http://127.0.0.1:8787; nothing was sent",
            Outcome::Environment(EnvironmentFault::NoStoreDirectory) => "HALVO_ACCOUNT_STORE_DIR is unset; nothing was stored, read or sent",
            Outcome::Environment(EnvironmentFault::BadStoreDirectory) => "HALVO_ACCOUNT_STORE_DIR is no absolute directory without a .. step; nothing was stored, read or sent",
            Outcome::Environment(EnvironmentFault::NoClient) => "the HTTP client could not be built; nothing was sent",
            Outcome::NotOpened { .. } => "the page could not be opened",
            Outcome::Usage(_) if cfg!(debug_assertions) => "usage: halvo-account [--print-url] status | subscribe AMOUNT PERIOD | set-cap AMOUNT PERIOD | cap | manage | delete; nothing was sent",
            Outcome::Usage(_) => "usage: halvo-account status | subscribe AMOUNT PERIOD | set-cap AMOUNT PERIOD | cap | manage | delete; nothing was sent",
        }
    }
}
