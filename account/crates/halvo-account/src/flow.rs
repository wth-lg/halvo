use crate::answer::{read, Fault, Reading, Reply, RouteAnswer, Unread};
use crate::grammar::{Command, Invocation, Show, Usage};
use crate::origin::{page_url, PageUrl, Pages};
use crate::outcome::{Answer, Failure, KeptFault, Outcome, StoreFault};
use crate::sign_in::SignIn;
use halvo_secrets::{Delete, Read, Secret, Store, Write};
use halvo_wire::request::{build, Call, Request};
use halvo_wire::types::{
    CapGetAnswer, CapGetAnswerResult, CapSetAnswer, CapSetAnswerResult, CheckoutAnswer,
    CheckoutAnswerResult, DeleteAnswer, DeleteAnswerResult, ManageAnswer, ManageAnswerResult,
    Spend, StatusAnswer, Url,
};

/// Where the sign-in is kept: the store of `halvo-secrets`, or a stand-in in a test.
pub trait Keep {
    fn read(&self) -> Read;
    fn write(&self, secret: &Secret) -> Write;
    fn delete(&self) -> Delete;
}

impl Keep for Store {
    fn read(&self) -> Read {
        Store::read(self)
    }

    fn write(&self, secret: &Secret) -> Write {
        Store::write(self, secret)
    }

    fn delete(&self) -> Delete {
        Store::delete(self)
    }
}

/// No answer came to a request: it may or may not have reached the backend.
#[derive(Debug)]
pub struct NoAnswer;

/// What sends a request that `halvo_wire::request::build` made and receives the answer.
pub trait Transport {
    fn send(&self, request: &Request) -> Result<Reply, NoAnswer>;
}

/// The page could not be opened.
#[derive(Debug)]
pub struct NotOpened;

/// What opens a page in the person's browser.
pub trait Opener {
    fn open(&self, page: &PageUrl) -> Result<(), NotOpened>;
}

/// The system gave no random bytes.
#[derive(Debug)]
pub struct NoRandom;

/// The source of the random bytes of a new sign-in.
pub trait Random {
    fn fill(&self, bytes: &mut [u8; 48]) -> Result<(), NoRandom>;
}

/// Everything that a command reaches beyond its own arguments.
pub struct Reach<'a> {
    pub keep: &'a dyn Keep,
    pub transport: &'a dyn Transport,
    pub opener: &'a dyn Opener,
    pub random: &'a dyn Random,
    pub pages: &'a Pages,
}

/// Runs one command. With no stored sign-in, every command but `subscribe` sends nothing;
/// `subscribe` makes a sign-in and stores it before it sends, and only when the store
/// answered that none is stored; a read that fails makes none and sends nothing. Each run
/// sends at most one request, built by `halvo_wire::request::build` from the stored
/// sign-in. Only `delete` removes the stored sign-in, and only after the backend answered
/// `deleted` or `no-such-account`: it then reads the store a second time and removes the item
/// only when it still holds the sign-in that this run sent, the same id and the same
/// credential byte for byte. It keeps another sign-in, an item of another form, and whatever
/// stands in a store that answers `Locked` or fails, and says so in its line. The store is
/// called twice between that read and the removal, by one process, and nothing stops another
/// run from storing a sign-in in that span: the store's own delete that compares first is
/// planner point 16 of P7-56, for a unit after 7.7.
pub fn run(invocation: &Invocation, reach: &Reach<'_>) -> Outcome {
    let sign_in = match (stored(reach.keep), &invocation.command) {
        (Ok(Some(sign_in)), _) => sign_in,
        (Ok(None), Command::Subscribe(_)) => match made(reach.keep, reach.random) {
            Ok(sign_in) => sign_in,
            Err(fault) => return Outcome::Store(fault),
        },
        (Ok(None), _) => return Outcome::NoCredential,
        (Err(fault), _) => return Outcome::Store(fault),
    };
    match &invocation.command {
        Command::Status => status(&sign_in, reach),
        Command::Subscribe(cap) => subscribe(
            &sign_in,
            &Call::CheckoutStart(cap.clone()),
            invocation.show,
            reach,
        ),
        Command::SetCap(cap) => set_cap(&sign_in, &Call::CapSet(cap.clone()), reach),
        Command::Cap => cap_get(&sign_in, reach),
        Command::Manage => manage(&sign_in, invocation.show, reach),
        Command::Delete => delete(&sign_in, reach),
    }
}

fn stored(keep: &dyn Keep) -> Result<Option<SignIn>, StoreFault> {
    match keep.read() {
        Read::NotStored => Ok(None),
        Read::Found(secret) => SignIn::from_secret(&secret)
            .map(Some)
            .ok_or(StoreFault::Unreadable),
        Read::Locked(_) => Err(StoreFault::Locked),
        Read::Fault(_) => Err(StoreFault::Fault),
    }
}

fn made(keep: &dyn Keep, random: &dyn Random) -> Result<SignIn, StoreFault> {
    let mut bytes = [0; 48];
    random.fill(&mut bytes).map_err(|_| StoreFault::NoRandom)?;
    let sign_in = SignIn::from_random(&bytes);
    bytes.fill(0);
    match keep.write(&sign_in.to_secret()) {
        Write::Stored => Ok(sign_in),
        Write::Occupied => Err(StoreFault::Changed),
        Write::Locked(_) => Err(StoreFault::Locked),
        Write::Fault(_) => Err(StoreFault::Fault),
    }
}

fn exchange<T: RouteAnswer>(
    sign_in: &SignIn,
    call: &Call,
    transport: &dyn Transport,
) -> Result<Reading<T>, Outcome> {
    let request = build(call, sign_in.account(), sign_in.credential())
        .map_err(|_| Outcome::Usage(Usage::Amount))?;
    let reply = transport
        .send(&request)
        .map_err(|_| Outcome::Failed(Failure::NoAnswer))?;
    read::<T>(call.route(), &reply).map_err(|unread| Outcome::Failed(Failure::Unread(unread)))
}

fn malformed(fault: Fault) -> Outcome {
    Outcome::Failed(Failure::Unread(Unread::Malformed(fault)))
}

fn subscribe(sign_in: &SignIn, call: &Call, show: Show, reach: &Reach<'_>) -> Outcome {
    match exchange::<CheckoutAnswer>(sign_in, call, reach.transport) {
        Err(outcome) => outcome,
        Ok(Reading::Rejected) => Outcome::Rejected,
        Ok(Reading::Answer(answer)) => match answer.result {
            CheckoutAnswerResult::Session => {
                page(answer.result.to_string(), answer.url.as_ref(), show, reach)
            }
            result => Outcome::Answered(Answer::Result(result.to_string())),
        },
    }
}

fn manage(sign_in: &SignIn, show: Show, reach: &Reach<'_>) -> Outcome {
    match exchange::<ManageAnswer>(sign_in, &Call::Manage, reach.transport) {
        Err(outcome) => outcome,
        Ok(Reading::Rejected) => Outcome::Rejected,
        Ok(Reading::Answer(answer)) => match answer.result {
            ManageAnswerResult::Page => {
                page(answer.result.to_string(), answer.url.as_ref(), show, reach)
            }
            result => Outcome::Answered(Answer::Result(result.to_string())),
        },
    }
}

fn page(result: String, url: Option<&Url>, show: Show, reach: &Reach<'_>) -> Outcome {
    let Some(url) = url else {
        return malformed(Fault::Results);
    };
    let Some(page) = page_url(url, reach.pages) else {
        return Outcome::Failed(Failure::PageRefused);
    };
    match show {
        Show::Print => Outcome::Answered(Answer::Page {
            result,
            printed: Some(page),
        }),
        Show::Open => match reach.opener.open(&page) {
            Ok(()) => Outcome::Answered(Answer::Page {
                result,
                printed: None,
            }),
            Err(NotOpened) => Outcome::NotOpened { result },
        },
    }
}

fn status(sign_in: &SignIn, reach: &Reach<'_>) -> Outcome {
    match exchange::<StatusAnswer>(sign_in, &Call::Status, reach.transport) {
        Err(outcome) => outcome,
        Ok(Reading::Rejected) => Outcome::Rejected,
        Ok(Reading::Answer(answer)) => Outcome::Answered(Answer::Result(answer.result.to_string())),
    }
}

fn cap_get(sign_in: &SignIn, reach: &Reach<'_>) -> Outcome {
    match exchange::<CapGetAnswer>(sign_in, &Call::CapGet, reach.transport) {
        Err(outcome) => outcome,
        Ok(Reading::Rejected) => Outcome::Rejected,
        Ok(Reading::Answer(answer)) => match answer.result {
            CapGetAnswerResult::Cap => cap_line(answer.cap.as_ref(), answer.spend.as_ref()),
            result => Outcome::Answered(Answer::Result(result.to_string())),
        },
    }
}

fn set_cap(sign_in: &SignIn, call: &Call, reach: &Reach<'_>) -> Outcome {
    match exchange::<CapSetAnswer>(sign_in, call, reach.transport) {
        Err(outcome) => outcome,
        Ok(Reading::Rejected) => Outcome::Rejected,
        Ok(Reading::Answer(answer)) => match answer.result {
            CapSetAnswerResult::Cap => cap_line(answer.cap.as_ref(), answer.spend.as_ref()),
            result => Outcome::Answered(Answer::Result(result.to_string())),
        },
    }
}

fn cap_line(cap: Option<&halvo_wire::types::Cap>, spend: Option<&Spend>) -> Outcome {
    match (cap, spend) {
        (Some(cap), Some(spend)) => Outcome::Answered(Answer::Cap {
            amount: cap.amount.get(),
            period: cap.period,
            spent: spend.spent,
            held: spend.held,
            period_end: spend.period_end,
        }),
        _ => malformed(Fault::Results),
    }
}

fn delete(sign_in: &SignIn, reach: &Reach<'_>) -> Outcome {
    let answer = match exchange::<DeleteAnswer>(sign_in, &Call::AccountDelete, reach.transport) {
        Err(outcome) => return outcome,
        Ok(Reading::Rejected) => return Outcome::Rejected,
        Ok(Reading::Answer(answer)) => answer,
    };
    let result = answer.result.to_string();
    match answer.result {
        DeleteAnswerResult::Deleted | DeleteAnswerResult::NoSuchAccount => {
            remove_if_sent(sign_in, result, reach.keep)
        }
        DeleteAnswerResult::Pending
        | DeleteAnswerResult::NotFinished
        | DeleteAnswerResult::CredentialMismatch => Outcome::Answered(Answer::Delete {
            result,
            removed: false,
        }),
    }
}

/// The removal after `deleted` or `no-such-account`: a second read, then a delete only when the
/// store holds the 101 bytes of the sign-in that was sent. With nothing stored, the credential
/// is gone and nothing is deleted. The span between this read and the delete is the one that
/// `run` names.
fn remove_if_sent(sent: &SignIn, result: String, keep: &dyn Keep) -> Outcome {
    let held = match keep.read() {
        Read::NotStored => {
            return Outcome::Answered(Answer::Delete {
                result,
                removed: true,
            })
        }
        Read::Found(secret) => secret,
        Read::Locked(_) => {
            return Outcome::Kept {
                result,
                fault: KeptFault::Locked,
            }
        }
        Read::Fault(_) => {
            return Outcome::Kept {
                result,
                fault: KeptFault::Fault,
            }
        }
    };
    if held.expose() != sent.to_secret().expose() {
        let fault = match SignIn::from_secret(&held) {
            Some(_) => KeptFault::Changed,
            None => KeptFault::Unreadable,
        };
        return Outcome::Kept { result, fault };
    }
    match keep.delete() {
        Delete::Deleted | Delete::NotStored => Outcome::Answered(Answer::Delete {
            result,
            removed: true,
        }),
        Delete::Locked(_) => Outcome::Kept {
            result,
            fault: KeptFault::Locked,
        },
        Delete::Fault(_) => Outcome::Kept {
            result,
            fault: KeptFault::Fault,
        },
    }
}
