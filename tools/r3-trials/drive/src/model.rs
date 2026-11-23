use std::ffi::OsString;
use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use halvo_cap::{ChildSpec, Exit, Limit, Outcome};
use halvo_route::count::{Count, HoldId, Usage};
use halvo_route::prices::{Prices, MICRO_PER_MINOR};
use halvo_route::tier::Tier;
use halvo_route_wire::{Body, Effort, Message, Role, WireTier, MAX_BODY_BYTES, MAX_OUTPUT_TOKENS};
use serde_json::Value;

/// The cap of A13.1 for every model child of the trial.
pub const LIMIT_BYTES: u64 = 8_589_934_592;

/// How to start the child. On the machine: `r3-model --weights FILE --settings FILE` under the cap,
/// `args` being those four words. Hosted: the door `halvo-model` (A12.6), `program` and `args`
/// being its whole command (under the egress check: `sh scripts/egress/check.sh LIST -- DOOR ...`),
/// with no cap, no weights and no settings. Its standard error is kept in a file of the run's own
/// directory.
#[derive(Clone, Debug)]
pub struct Spec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub stderr: PathBuf,
    pub hosted: Option<Hosted>,
}

/// What the hosted form adds: the host's key, the count that holds the ceiling, and the directory
/// that the door's command starts in.
#[derive(Clone, Debug)]
pub struct Hosted {
    pub key: Key,
    pub spend: Arc<Mutex<Spend>>,
    pub dir: PathBuf,
    /// The door's starts of the round, which the egress line holds its own count against.
    pub starts: Arc<AtomicU64>,
}

/// The most requests of one round (the coordinator's ruling of log line 18072): 192 in compiles (six
/// of at most `CALLS_MAX` asks), 18 in recoveries (two asks at each of the three stored steps of a
/// right walk that the variant changed_page_2 turns, in three `bill` and three `order` cells) and 6
/// for replies (two bodies in three cells). docs/r3/trials/driver-hosted.md states it.
pub const REQUESTS_MAX: u64 = 216;

/// The most bytes of a key (A12.6).
pub const KEY_MAX: usize = 4_096;

/// The host's key, read once from the file that the command names. It goes to the door alone: its
/// `Debug` gives its number of bytes and it has no `Display`.
#[derive(Clone)]
pub struct Key(Arc<Vec<u8>>);

impl Key {
    /// The key of a file's text: its bytes without one line end at the end, 1 to `KEY_MAX` bytes of
    /// visible ASCII (0x21 to 0x7E), the form that the door takes. The error names no byte of it.
    pub fn of(text: &[u8]) -> Result<Key, &'static str> {
        let text = text.strip_suffix(b"\n").unwrap_or(text);
        let text = text.strip_suffix(b"\r").unwrap_or(text);
        if text.is_empty() || text.len() > KEY_MAX {
            return Err("the key file holds no key of 1 to 4096 bytes");
        }
        if !text.iter().all(|b| (0x21..=0x7e).contains(b)) {
            return Err("the key file holds a byte that is no visible ASCII");
        }
        Ok(Key(Arc::new(text.to_vec())))
    }

    /// The head of the door's input: the key's length line, the key, one LF.
    fn head(&self) -> Vec<u8> {
        let mut out = format!("{}\n", self.0.len()).into_bytes();
        out.extend_from_slice(&self.0);
        out.push(b'\n');
        out
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key({} bytes)", self.0.len())
    }
}

/// The count of r6 U2.5 (`halvo_route::count`) with the round's ceiling as its cap, the tier of the
/// round and the build's prices: a hold before each request and its settle at the usage that the
/// door's reply reports.
#[derive(Debug)]
pub struct Spend {
    count: Count,
    prices: &'static Prices,
    tier: Tier,
    ceiling_minor: u64,
    holds: u64,
    bound: u64,
}

impl Spend {
    /// A count of nothing spent whose cap is `ceiling_minor` minor units, for a hosted tier.
    pub fn new(tier: Tier, ceiling_minor: u64) -> Result<Spend, &'static str> {
        if tier == Tier::OnDevice {
            return Err("the tier on_device has no host");
        }
        let count = Count::new(u64::MAX)
            .set_cap(ceiling_minor)
            .map_err(|_| "the ceiling is no amount of 1 to 100000000 minor units")?;
        Ok(Spend {
            count,
            prices: Prices::release(),
            tier,
            ceiling_minor,
            holds: 0,
            bound: REQUESTS_MAX,
        })
    }

    /// The count with `bound` in the place of `REQUESTS_MAX`: a test's own lower bound.
    #[cfg(test)]
    pub fn bounded(self, bound: u64) -> Spend {
        Spend { bound, ..self }
    }

    /// Reserves the hold of one request whose closed body has `body_bytes` bytes, or refuses it:
    /// `Fault::Bound` when the round has handed the door its bound of requests, `Fault::Ceiling`
    /// when the hold does not fit under the cap. A refused hold changes nothing.
    fn hold(&mut self, body_bytes: u64) -> Result<HoldId, Fault> {
        if self.holds >= self.bound {
            return Err(Fault::Bound);
        }
        let mut id = [0u8; 16];
        id[8..].copy_from_slice(&(self.holds + 1).to_be_bytes());
        let id = HoldId(id);
        let next = self
            .count
            .reserve(
                id,
                self.prices,
                self.tier,
                body_bytes,
                u64::from(MAX_OUTPUT_TOKENS),
                u64::MAX,
            )
            .map_err(|_| Fault::Ceiling)?;
        self.count = next;
        self.holds += 1;
        Ok(id)
    }

    /// Settles the hold `id`: at the reported tokens, at 0 for a request that did not leave, or at
    /// its whole amount when the request may have left and no usage came back.
    fn settle(&mut self, id: HoldId, end: &Settle) {
        let next = match end {
            Settle::Reported(tokens_in, tokens_out) => self.count.settle(
                id,
                self.prices,
                Usage::Reported {
                    tokens_in: *tokens_in,
                    tokens_out: *tokens_out,
                },
            ),
            Settle::Unsent => self.count.settle(id, self.prices, Usage::Unsent),
            Settle::Whole => self.count.expire(u64::MAX),
        };
        if let Ok(next) = next {
            self.count = next;
        }
    }

    /// The tier of the round.
    pub fn tier(&self) -> Tier {
        self.tier
    }

    /// `spend tier=<tier> requests=<n> spent_micro=<n> ceiling_minor=<n>`: the requests written to
    /// the door and the count's sum of what was spent.
    pub fn line(&self) -> String {
        format!(
            "spend tier={} requests={} spent_micro={} ceiling_minor={}",
            self.tier.as_str(),
            self.holds,
            self.count.account().spent,
            self.ceiling_minor
        )
    }

    /// Whether what was spent is at or under the ceiling.
    pub fn within(&self) -> bool {
        u128::from(self.count.account().spent)
            <= u128::from(self.ceiling_minor) * u128::from(MICRO_PER_MINOR)
    }

    /// The count's row (`Count::row`), for the round's directory.
    pub fn row(&self) -> String {
        self.count.row()
    }
}

/// How a hold ends.
enum Settle {
    Reported(u64, u64),
    Unsent,
    Whole,
}

/// The door's causes of a refusal after which the request may have left: the request went out and
/// no usage came back.
const LEFT: [&str; 3] = ["no_answer", "status", "answer"];

/// The user's text of the door: the prompt, then the snapshot between the two marker lines of A9.5,
/// as the door makes it (halvo-model's `user_text`).
fn user_text(prompt: &str, snapshot: Option<&str>) -> String {
    match snapshot {
        Some(data) => format!("{prompt}\n<<<DATA\n{data}\nDATA>>>"),
        None => prompt.to_owned(),
    }
}

/// The bytes of the closed body of T11.3 that the door makes of one request, at most
/// `MAX_BODY_BYTES`: a body above it is cut by the door to fit or refused unsent.
pub fn body_bytes(tier: Tier, system: &str, prompt: &str, snapshot: Option<&str>) -> u64 {
    let wire = match tier {
        Tier::LowCostHosted => WireTier::LowCostHosted,
        _ => WireTier::Frontier,
    };
    let body = Body {
        tier: wire,
        effort: Effort::Medium,
        max_output_tokens: MAX_OUTPUT_TOKENS,
        messages: vec![
            Message {
                role: Role::System,
                content: system.to_owned(),
            },
            Message {
                role: Role::User,
                content: user_text(prompt, snapshot),
            },
        ],
    };
    body.to_bytes()
        .map_or(MAX_BODY_BYTES as u64, |b| b.len() as u64)
}

/// One reply line of A12.2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

/// One request of A12.2 as its line, keys in the order of the contract.
pub fn request_line(
    id: u64,
    system: &str,
    prompt: &str,
    snapshot: Option<&str>,
    grammar: &str,
) -> String {
    let s = |t: &str| serde_json::to_string(t).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        "{{\"id\":{id},\"system\":{},\"prompt\":{},\"snapshot\":{},\"grammar\":\"{grammar}\",\"level\":\"medium\"}}",
        s(system),
        s(prompt),
        snapshot.map_or("null".to_string(), s)
    )
}

/// Parses one reply line of A12.2 for the request `id`.
pub fn parse_reply(line: &str, id: u64) -> Result<Reply, String> {
    let v: Value = serde_json::from_str(line).map_err(|_| "the reply is no JSON".to_string())?;
    if v["id"].as_u64() != Some(id) {
        return Err("the reply has another id".to_string());
    }
    let count = |k: &str| v[k].as_u64().ok_or_else(|| format!("the reply has no {k}"));
    Ok(Reply {
        text: v["text"]
            .as_str()
            .ok_or("the reply has no text")?
            .to_string(),
        tokens_in: count("tokens_in")?,
        tokens_out: count("tokens_out")?,
    })
}

/// The cause of the door's refusal line `{"id":<id>,"refused":"<cause>"}` (A12.6), or none.
pub fn refusal(line: &str, id: u64) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    let object = v.as_object()?;
    (object.len() == 2 && v["id"].as_u64() == Some(id))
        .then(|| v["refused"].as_str().map(str::to_owned))?
}

/// Why the child gave no reply.
#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    /// The model lock is held by another loader (A12.2: `REFUSED lock: held`).
    LockHeld,
    /// The cap stopped the child (A13.1): the run is a miss.
    Capped,
    /// The next hold does not fit under the ceiling: nothing of the request was written.
    Ceiling,
    /// The round has handed the door its bound of requests: nothing of the request was written.
    Bound,
    /// The door refused the request with this cause (A12.6).
    Refused(String),
    /// The child ended or answered out of form; the text says how.
    Child(String),
}

/// How a child ended: its exit and whether the cap stopped it (never, for the door).
#[derive(Debug)]
pub struct Ended {
    pub exit: Exit,
    pub capped: bool,
}

/// How the child runs: under the cap, or, for the door, as a plain process.
enum Running {
    Capped(JoinHandle<Result<Outcome, String>>),
    Plain(std::process::Child),
}

/// A running child. Dropping it closes the child's standard input, so the child ends at the end of
/// its input (A12.2, A12.6) even when the trial ends by a panic.
pub struct Child {
    input: Option<PipeWriter>,
    output: BufReader<PipeReader>,
    running: Option<Running>,
    stderr: PathBuf,
    hosted: Option<Hosted>,
    next: u64,
}

fn stderr_text(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

impl Child {
    /// Starts the child. On the machine: under the cap, and reads its ready line. Hosted: the
    /// door's command, to which it writes the key first; the door writes no ready line.
    pub fn start(spec: &Spec) -> Result<Child, Fault> {
        let fault = |e: std::io::Error| Fault::Child(e.to_string());
        let (child_in, input) = std::io::pipe().map_err(fault)?;
        let (output, child_out) = std::io::pipe().map_err(fault)?;
        let err = if spec.hosted.is_some() {
            // The door runs under the sh of scripts/egress/check.sh, which on Windows (MSYS) writes
            // nothing to a handle opened for appending alone: open for writing, at the end, which
            // keeps what the file holds.
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&spec.stderr)
                .and_then(|mut f| f.seek(SeekFrom::End(0)).map(|_| f))
        } else {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&spec.stderr)
        }
        .map_err(fault)?;
        if let Some(hosted) = &spec.hosted {
            let process = std::process::Command::new(&spec.program)
                .args(&spec.args)
                .current_dir(&hosted.dir)
                .stdin(Stdio::from(child_in))
                .stdout(Stdio::from(child_out))
                .stderr(Stdio::from(err))
                .spawn()
                .map_err(fault)?;
            hosted.starts.fetch_add(1, Ordering::SeqCst);
            let mut started = Child {
                input: Some(input),
                output: BufReader::new(output),
                running: Some(Running::Plain(process)),
                stderr: spec.stderr.clone(),
                hosted: Some(hosted.clone()),
                next: 1,
            };
            let head = hosted.key.head();
            let written = started
                .input
                .as_mut()
                .map(|w| w.write_all(&head).and_then(|()| w.flush()));
            if !matches!(written, Some(Ok(()))) {
                let outcome = started.finish();
                return Err(Fault::Child(format!("the door took no key: {outcome:?}")));
            }
            return Ok(started);
        }
        let mut child = ChildSpec::new(spec.program.clone(), spec.args.clone());
        child.stdin = Stdio::from(child_in);
        child.stdout = Stdio::from(child_out);
        child.stderr = Stdio::from(err);
        let limit = Limit::new(LIMIT_BYTES).ok_or(Fault::Child("limit".into()))?;
        let capped =
            std::thread::spawn(move || halvo_cap::run(child, limit).map_err(|e| e.to_string()));
        let mut started = Child {
            input: Some(input),
            output: BufReader::new(output),
            running: Some(Running::Capped(capped)),
            stderr: spec.stderr.clone(),
            hosted: None,
            next: 1,
        };
        let mut ready = String::new();
        let read = started.output.read_line(&mut ready).map_err(fault)?;
        if read == 0 || !ready.starts_with("ready ") {
            let outcome = started.finish();
            if stderr_text(&started.stderr).contains("REFUSED lock: held") {
                return Err(Fault::LockHeld);
            }
            return Err(match outcome {
                Ok(o) if o.capped => Fault::Capped,
                Ok(o) => Fault::Child(format!("no ready line, exit {:?}", o.exit)),
                Err(e) => Fault::Child(e),
            });
        }
        Ok(started)
    }

    /// Sends one request and reads its reply. Hosted, the hold comes first: when it does not fit
    /// under the ceiling, no byte of the request is written; after the reply the hold settles.
    pub fn ask(
        &mut self,
        system: &str,
        prompt: &str,
        snapshot: Option<&str>,
        grammar: &str,
    ) -> Result<Reply, Fault> {
        let hold = match &self.hosted {
            Some(h) => {
                let mut spend = h.spend.lock().map_err(|_| Fault::Child("count".into()))?;
                let bytes = body_bytes(spend.tier(), system, prompt, snapshot);
                Some(spend.hold(bytes)?)
            }
            None => None,
        };
        let id = self.next;
        self.next += 1;
        let line = request_line(id, system, prompt, snapshot, grammar);
        let sent = self
            .input
            .as_mut()
            .map(|w| writeln!(w, "{line}").and_then(|()| w.flush()));
        let mut reply = String::new();
        let read = match sent {
            Some(Ok(())) => self.output.read_line(&mut reply).unwrap_or(0),
            _ => 0,
        };
        let reply = reply.trim_end();
        let parsed = if read == 0 {
            Err(match self.finish() {
                Ok(o) if o.capped => Fault::Capped,
                Ok(o) => Fault::Child(format!("the child ended, exit {:?}", o.exit)),
                Err(e) => Fault::Child(e),
            })
        } else if let Some(cause) = self.hosted.as_ref().and(refusal(reply, id)) {
            Err(Fault::Refused(cause))
        } else {
            parse_reply(reply, id).map_err(Fault::Child)
        };
        if let (Some(h), Some(hold)) = (&self.hosted, hold) {
            let end = match &parsed {
                Ok(r) => Settle::Reported(r.tokens_in, r.tokens_out),
                Err(Fault::Refused(cause)) if !LEFT.contains(&cause.as_str()) => Settle::Unsent,
                Err(_) => Settle::Whole,
            };
            if let Ok(mut spend) = h.spend.lock() {
                spend.settle(hold, &end);
            }
        }
        parsed
    }

    /// Closes the child's input and waits for its end: how it ended.
    pub fn finish(&mut self) -> Result<Ended, String> {
        self.input.take();
        match self.running.take() {
            Some(Running::Capped(h)) => h
                .join()
                .map_err(|_| "the cap thread panicked".to_string())?
                .map(|o| Ended {
                    exit: o.exit,
                    capped: o.capped,
                }),
            Some(Running::Plain(mut p)) => {
                let status = p.wait().map_err(|e| e.to_string())?;
                Ok(Ended {
                    exit: status.code().map_or(Exit::Signal(0), Exit::Code),
                    capped: false,
                })
            }
            None => Err("finished before".to_string()),
        }
    }

    /// Ends the run of the child: whether the cap stopped it.
    pub fn end(mut self) -> Result<bool, String> {
        let o = self.finish()?;
        match o.exit {
            Exit::NotStarted => Err("the child did not start".to_string()),
            _ => Ok(o.capped),
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        self.input.take();
        match self.running.take() {
            Some(Running::Capped(h)) => {
                let _ = h.join();
            }
            Some(Running::Plain(mut p)) => {
                let _ = p.wait();
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_line_has_the_keys_of_a12_2_in_order() {
        assert_eq!(
            request_line(3, "Be brief.", "Pay \"now\".", Some("e1 button \"Pay\"\ne2 text \"x\""), "action"),
            "{\"id\":3,\"system\":\"Be brief.\",\"prompt\":\"Pay \\\"now\\\".\",\"snapshot\":\"e1 button \\\"Pay\\\"\\ne2 text \\\"x\\\"\",\"grammar\":\"action\",\"level\":\"medium\"}"
        );
        assert_eq!(
            request_line(1, "s", "p", None, "text"),
            "{\"id\":1,\"system\":\"s\",\"prompt\":\"p\",\"snapshot\":null,\"grammar\":\"text\",\"level\":\"medium\"}"
        );
    }

    #[test]
    fn a_reply_line_is_read_for_its_id() {
        let line = "{\"id\":2,\"text\":\"{\\\"action\\\":\\\"done\\\"}\",\"tokens_in\":340,\"tokens_out\":31,\"first_token_ms\":10,\"ms\":20}";
        assert_eq!(
            parse_reply(line, 2),
            Ok(Reply {
                text: "{\"action\":\"done\"}".into(),
                tokens_in: 340,
                tokens_out: 31
            })
        );
        assert!(parse_reply(line, 3).is_err());
        assert!(parse_reply("{\"id\":2,\"text\":\"x\"}", 2).is_err());
        assert!(parse_reply("x", 2).is_err());
    }
}
