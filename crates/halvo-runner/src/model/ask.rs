//! The one way of a run to its hosted model (r3 U7.2a): `Runner::ask`; the door's start, which reads the person's key
//! in the one place of the runner that asks for it and gives it to the door as the first line of its standard input
//! (A12.3, A12.6); and the door's end.

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use halvo_schema::executable::body::{ModelHold, ModelSettle, ModelUsage};
use halvo_schema::executable::frame::SecretReply;
use halvo_schema::executable::names::ModelTier;
use halvo_schema::executable::text::{Name, SecretText};
use halvo_schema::executable::{Cause, ModelRequestId, Outcome, RunnerCall};
use halvo_web::start::{start_on_stdio, Spec, Started};

use super::handed_back;
use super::snapshot::Snapshot;
use crate::clock::random16;
use crate::route::{Call, Picked};
use crate::run::{cause_of, Runner, Stop};

/// The door's program, beside the runner's own executable.
const DOOR: &str = "halvo-model";

/// The reserved name of the person's key for a model's host in the supervisor's `Secrets` (A8.6, r3 U7.6).
const MODEL_KEY: &str = "model_key";

/// The most bytes of a key in the door's form (A12.6).
const KEY_MAX: usize = 4_096;

/// The output limit of every request, the door's own (A12.6), which the hold names.
pub(crate) const OUTPUT_LIMIT: u64 = 2_048;

/// How long the runner waits for the door's exit after it closed the door's standard input, before the system's
/// hard kill.
pub(crate) const DOOR_END: Duration = Duration::from_secs(5);

/// The door's refusals of a request that LEFT for the host and came back with no usage: an answer cut at the output
/// limit (`cut`), one that the host declined (`declined`), one of another form (`answer`), and no whole answer within
/// the door's time limit (`no_answer`). Their hold settles at its worst case (`usage_of`).
const LEFT_WITHOUT_USAGE: [&str; 4] = ["cut", "declined", "answer", "no_answer"];

/// The person's key in the door's form (A12.6): 1 to `KEY_MAX` bytes, each of visible ASCII. Its `Debug` gives its
/// number of bytes alone.
pub(crate) struct Key(String);

impl Key {
    /// The key of a stored value, or none where the door's form refuses it.
    fn of(value: &SecretText) -> Option<Key> {
        let text = value.expose();
        let fits = (1..=KEY_MAX).contains(&text.len())
            && text.bytes().all(|byte| (0x21..=0x7e).contains(&byte));
        fits.then(|| Key(text.to_owned()))
    }

    /// The head of the door's standard input: the key's length in decimal digits and one LF, the key's bytes, one
    /// LF.
    fn head(&self) -> Vec<u8> {
        let mut head = format!("{}\n", self.0.len()).into_bytes();
        head.extend_from_slice(self.0.as_bytes());
        head.push(b'\n');
        head
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key({} bytes)", self.0.len())
    }
}

/// The grammar of a request line (A12.2): an action, or a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grammar {
    Action,
    Text,
}

impl Grammar {
    fn word(self) -> &'static str {
        match self {
            Grammar::Action => "action",
            Grammar::Text => "text",
        }
    }
}

/// The level of a request line (A12.2), which the door sends as the host's effort: the level that the router's pick
/// chose (crate::route::Picked).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Level {
    Low,
    Medium,
    High,
}

impl Level {
    fn word(self) -> &'static str {
        match self {
            Level::Low => "low",
            Level::Medium => "medium",
            Level::High => "high",
        }
    }
}

/// One request of a run to its model: the texts and words of a request line of A12.2 but its id, which `ask` counts,
/// and its level, which `ask` reads from the router's pick (crate::route::Picked).
/// What the texts may hold is the duty of the caller (A12.7), but for the snapshot, which is a `Snapshot` that the
/// mask alone makes, from a page's lines or from a message's text (crate::model::snapshot, r3 U7.2b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelRequest {
    pub(crate) system: String,
    pub(crate) prompt: String,
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) grammar: Grammar,
}

/// The door's line for a request: the model's text with the tokens that the host reported, or a refusal with its
/// word (A12.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    Reply {
        text: String,
        tokens_in: u64,
        tokens_out: u64,
    },
    Refused(String),
}

/// What became of one request line after its hold.
enum Sent {
    /// The door's line of its answer.
    Answered(Answer),
    /// Nothing left: the door was not started, with the end of the run where the read of the key ended it, or the
    /// line could not be written.
    Nothing(Option<Stop>),
    /// The line was written and no line of its answer came back: the request may have left.
    Lost,
}

/// THE ONE BRANCH of the settle (the coordinator's way B, log line 22165 (5)): the tokens that the door's reply
/// reports; the worst case that the hold reserved (tokens in = the request line's bytes, tokens out =
/// `OUTPUT_LIMIT`) where the request left and no usage came back, the refusals of `LEFT_WITHOUT_USAGE` and a written
/// line with no answer; and nothing (null) for every other refusal, the door's `status` among them, and for a request
/// of which nothing was sent.
fn usage_of(sent: &Sent, body_bytes: u64) -> Option<ModelUsage> {
    let worst = ModelUsage {
        tokens_in: body_bytes,
        tokens_out: OUTPUT_LIMIT,
    };
    match sent {
        Sent::Answered(Answer::Reply {
            tokens_in,
            tokens_out,
            ..
        }) => Some(ModelUsage {
            tokens_in: *tokens_in,
            tokens_out: *tokens_out,
        }),
        Sent::Answered(Answer::Refused(word)) if LEFT_WITHOUT_USAGE.contains(&word.as_str()) => {
            Some(worst)
        }
        Sent::Lost => Some(worst),
        Sent::Answered(Answer::Refused(_)) | Sent::Nothing(_) => None,
    }
}

/// Where the runner finds the door: the program and, in a unit test, the arguments that stand before the door's
/// own.
#[derive(Clone, Debug)]
pub(crate) struct Place {
    pub(crate) program: PathBuf,
    pub(crate) before: Vec<OsString>,
}

impl Place {
    /// The door beside the runner's own executable: `halvo-model` with the system's suffix; none where the runner's
    /// own path cannot be read.
    fn beside_the_runner() -> Option<Place> {
        let exe = std::env::current_exe().ok()?;
        Some(Place {
            program: exe
                .parent()?
                .join(format!("{DOOR}{}", std::env::consts::EXE_SUFFIX)),
            before: Vec::new(),
        })
    }
}

/// A started door: the child, the end of its standard input, which the runner closes to end it, and its standard
/// output.
pub(crate) struct Door {
    child: Started,
    input: Option<File>,
    output: BufReader<File>,
}

impl Door {
    /// Ends the door: closes its standard input, waits for its exit up to `DOOR_END`, then kills it; it is reaped
    /// when this returns. True where it ended by itself.
    pub(crate) fn end(mut self) -> bool {
        drop(self.input.take());
        let until = Instant::now() + DOOR_END;
        while Instant::now() < until {
            if self.child.exited() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.child.exited() {
            return true;
        }
        self.child.kill();
        false
    }
}

/// The door's whole environment, A6.1's list for the model child: on macOS `HOME` as the runner has it, left out
/// where it has none; on Windows `SystemRoot` by the system's call and `LOCALAPPDATA` as the runner has it, left out
/// where it has none. None where the system's value cannot be read.
fn door_env() -> Option<Vec<(OsString, OsString)>> {
    let mut env = halvo_web::start::guard_env()?;
    #[cfg(target_os = "macos")]
    let name = "HOME";
    #[cfg(windows)]
    let name = "LOCALAPPDATA";
    if let Some(value) = std::env::var_os(name) {
        env.push((name.into(), value));
    }
    Some(env)
}

/// The request line of A12.2 for `request` with the id `id` at the level `level`: one JSON object of the six keys in
/// the contract's order.
pub(super) fn request_line(id: u64, request: &ModelRequest, level: Level) -> String {
    let text = |s: &str| serde_json::Value::String(s.to_owned()).to_string();
    let snapshot = request
        .snapshot
        .as_ref()
        .map_or_else(|| "null".to_owned(), |snapshot| text(snapshot.text()));
    format!(
        "{{\"id\":{id},\"system\":{},\"prompt\":{},\"snapshot\":{snapshot},\"grammar\":\"{}\",\"level\":\"{}\"}}",
        text(&request.system),
        text(&request.prompt),
        request.grammar.word(),
        level.word()
    )
}

/// The answer of the door's line `line` to the request `id`: a reply line of A12.2 (`id`, `text`, `tokens_in`,
/// `tokens_out`, `first_token_ms`, `ms`) or a refusal line (`id`, `refused`); none for any other line, a line of
/// another id among them.
fn answer_of(line: &str, id: u64) -> Option<Answer> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;
    if object.get("id")?.as_u64()? != id {
        return None;
    }
    if object.len() == 2 {
        return Some(Answer::Refused(object.get("refused")?.as_str()?.to_owned()));
    }
    let keys = [
        "id",
        "text",
        "tokens_in",
        "tokens_out",
        "first_token_ms",
        "ms",
    ];
    if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
        return None;
    }
    Some(Answer::Reply {
        text: object["text"].as_str()?.to_owned(),
        tokens_in: object["tokens_in"].as_u64()?,
        tokens_out: object["tokens_out"].as_u64()?,
    })
}

/// Starts the door at `place` for `tier` (A12.3): its standard input and output on two pipes, its standard error on
/// the null device and nothing else of the runner in it, the environment of `door_env`, the working directory of
/// halvo-web's `system_dir`, the arguments `--tier <tier> --ceiling <tier> --consent recorded`; then the key's head
/// as the first bytes of its standard input. None where a part of the start fails; a door that was started and does
/// not take the key is ended first. A stand-in of a unit test is this test binary, whose harness writes its header
/// on standard output before the stand-in runs: the start reads past that header for a stand-in alone.
fn start(place: Place, tier: ModelTier, key: &Key) -> Option<Door> {
    let dir = halvo_web::start::system_dir()?;
    let env = door_env()?;
    #[cfg(test)]
    let stand_in = !place.before.is_empty();
    let mut args = place.before;
    for word in [
        "--tier",
        tier.as_str(),
        "--ceiling",
        tier.as_str(),
        "--consent",
        "recorded",
    ] {
        args.push(word.into());
    }
    let spec = Spec {
        program: &place.program,
        args,
        env,
        dir: &dir,
    };
    let started = {
        let held = halvo_web::start::lock();
        start_on_stdio(&held, &spec).ok()?
    };
    let mut door = Door {
        child: started.child,
        input: Some(started.to_child),
        output: BufReader::new(started.from_child),
    };
    let taken = door.input.as_mut().is_some_and(|input| {
        input
            .write_all(&key.head())
            .and_then(|()| input.flush())
            .is_ok()
    });
    if !taken {
        door.end();
        return None;
    }
    #[cfg(test)]
    if stand_in {
        let mut header = String::new();
        while door.output.read_line(&mut header).is_ok_and(|n| n > 0) {
            if header.trim_end() == "running 1 test" {
                break;
            }
            header.clear();
        }
    }
    Some(door)
}

impl Runner {
    /// Asks the run's hosted model one request (r3 U7.2a). (1) Holds its cost in the person's count by the body
    /// `model_hold`, with an id that the runner mints, the run's tier, the request line's bytes and `OUTPUT_LIMIT`;
    /// a hold that the store refuses with `over_cap` (the person's cap) ends the run `handed_back` with `over_cap`,
    /// one refused with `config` (the person took the agreement back) with `no_agreement`, any other with `failed`,
    /// `no_cap` among them, which only a build whose own cap line does not read gives; each before the door is
    /// started and before any line is written to it, and with no settle. (2) Where the run has no
    /// door, starts it (`start_door`): at most once a run, since a start that fails ends the run. (3) Writes the
    /// request line and reads one line. (4) Settles the hold by the body `model_settle` at `usage_of`, the one branch
    /// of the settle, also where the request failed or the key's read ended the run. A run that holds no hosted
    /// model asks nothing and ends `handed_back` with `no_agreement`. A request that got no answer ends the run
    /// `failed` with `step_failed` after its settle. `ask` takes the router's pick for the request (`picked`, which
    /// crate::route's `pick` alone makes, so that a caller that did not pick does not compile) and reads the request
    /// line's level from it; a pick of a tier other than the run's hosted tier, and a pick for a kind of call whose
    /// grammar is not the request's (a compile asks in the grammar `action`, a writer in the grammar `text`), end the
    /// run `failed` with `step_failed` before the hold, and the hold and the door keep the run's hosted tier.
    pub(crate) fn ask(&mut self, picked: &Picked, request: &ModelRequest) -> Result<Answer, Stop> {
        let Some(hosted) = self.hosted.as_mut() else {
            return Err(handed_back(Cause::NoAgreement));
        };
        let grammar_of_the_call = match picked.call() {
            Call::Compile => Grammar::Action,
            Call::Write => Grammar::Text,
        };
        if picked.tier() != hosted.tier || request.grammar != grammar_of_the_call {
            return Err(Stop::End(Outcome::Failed, Some(Cause::StepFailed)));
        }
        let id = hosted.next_line;
        hosted.next_line += 1;
        let tier = hosted.tier;
        let line = request_line(id, request, picked.level());
        let body_bytes = line.len() as u64;
        let held = ModelRequestId::from_random(random16());
        let hold = RunnerCall::ModelHold(ModelHold {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            request: held.clone(),
            tier,
            body_bytes,
            output_limit: OUTPUT_LIMIT,
        });
        if let Err(error) = self.store(hold)? {
            return Err(match error.as_str() {
                "over_cap" => handed_back(Cause::OverCap),
                "config" => handed_back(Cause::NoAgreement),
                _ => Stop::End(Outcome::Failed, Some(cause_of(error))),
            });
        }
        let sent = self.exchange(&line, id);
        let settle = RunnerCall::ModelSettle(ModelSettle {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            request: held,
            usage: usage_of(&sent, body_bytes),
        });
        if let Err(error) = self.store(settle)? {
            return Err(Stop::End(Outcome::Failed, Some(cause_of(error))));
        }
        match sent {
            Sent::Answered(answer) => Ok(answer),
            Sent::Nothing(Some(stop)) => Err(stop),
            Sent::Nothing(None) | Sent::Lost => {
                Err(Stop::End(Outcome::Failed, Some(Cause::StepFailed)))
            }
        }
    }

    /// The door's answer to the request line `line` of the id `id`, the door started first where the run has none.
    fn exchange(&mut self, line: &str, id: u64) -> Sent {
        if self
            .hosted
            .as_ref()
            .is_some_and(|hosted| hosted.door.is_none())
        {
            match self.start_door() {
                Ok(door) => {
                    if let Some(hosted) = self.hosted.as_mut() {
                        hosted.door = Some(door);
                    }
                }
                Err(stop) => return Sent::Nothing(Some(stop)),
            }
        }
        let Some(door) = self.hosted.as_mut().and_then(|hosted| hosted.door.as_mut()) else {
            return Sent::Nothing(None);
        };
        let Some(input) = door.input.as_mut() else {
            return Sent::Nothing(None);
        };
        let written = input
            .write_all(line.as_bytes())
            .and_then(|()| input.write_all(b"\n"))
            .and_then(|()| input.flush());
        if written.is_err() {
            return Sent::Nothing(None);
        }
        let mut reply = String::new();
        match door.output.read_line(&mut reply) {
            Ok(n) if n > 0 => {
                answer_of(reply.trim_end_matches('\n'), id).map_or(Sent::Lost, Sent::Answered)
            }
            _ => Sent::Lost,
        }
    }

    /// The door's start (A12.3): first the person's key (`model_key`), then the door at its place with that key as
    /// the first line of its standard input. A start that fails ends the run `failed` with `step_failed`.
    fn start_door(&mut self) -> Result<Door, Stop> {
        let key = self.model_key()?;
        let tier = self.hosted.as_ref().map(|hosted| hosted.tier);
        let failed = Stop::End(Outcome::Failed, Some(Cause::StepFailed));
        let (Some(place), Some(tier)) = (self.place(), tier) else {
            return Err(failed);
        };
        start(place, tier, &key).ok_or(failed)
    }

    /// THE ONE PLACE of the runner that asks for the person's key for a model's host, which only the door's start
    /// calls (the coordinator's log line 22165 (3)): the frame `secret` for the reserved name `model_key`. None stored
    /// ends the run `handed_back` with `no_model_key`; an unreadable store, and a stored value that the door's form
    /// refuses (A12.6), end it `failed` with `secret` (A8.6).
    fn model_key(&mut self) -> Result<Key, Stop> {
        let unreadable = || Stop::End(Outcome::Failed, Some(Cause::Secret));
        let name = Name::parse(MODEL_KEY).map_err(|_| unreadable())?;
        match self.secret(&name)? {
            SecretReply::Stored(value) => Key::of(&value).ok_or_else(unreadable),
            SecretReply::None => Err(handed_back(Cause::NoModelKey)),
            SecretReply::Unreadable(_) => Err(unreadable()),
        }
    }

    /// Where this run finds its door: the stand-in of a unit test where it gives one, else beside the runner.
    fn place(&self) -> Option<Place> {
        #[cfg(test)]
        if let Some(place) = &self.stand_in_door {
            return Some(place.clone());
        }
        Place::beside_the_runner()
    }
}
