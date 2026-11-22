#![forbid(unsafe_code)]
//! The ONE door to a model's host (the agents run's unit U7.5). The program `halvo-model` reads the
//! host's key from the head of its standard input, in the form of `Key::read`, then one request a
//! line in the form of the agents run's contract A12.2, and writes one line a request. For each request it makes the closed
//! body of the routing run's contract T11.3, asks the router's `grant` with the release ruling, the
//! tier, the ceiling and the agreement of its arguments, the part `text` alone and the origin of the
//! ruling's row for the tier, and hands the grant by value to the one operation of
//! `halvo_connect::model`, which sends ONE request in the form that the row's adapter word names to
//! the origin that the grant names and to no other. With no grant it opens no connection. It holds no page, no browser, no store record
//! and no kept sign-in, reads no variable of its environment, and writes the key into no line.

use std::fmt;
use std::io::{BufRead, Write};

use halvo_connect::model::{send_under_grant, Adapter, Answered, SendError};
use halvo_route::leave::{grant, Consent, LeaveCause, PartKind};
use halvo_route::ruling::{HostRow, Ruling};
use halvo_route::tier::Tier;
use halvo_route_wire::{Body, BodyError, Effort, Message, Role, WireTier, MAX_OUTPUT_TOKENS};
use serde_json::Value;

#[cfg(feature = "loopback")]
pub mod loopback;

/// The marker lines around the snapshot in the user's text (A9.5).
pub const OPEN: &str = "<<<DATA";
pub const CLOSE: &str = "DATA>>>";

/// The adapter words of the door: a row of the table of hosts whose adapter word is not on this
/// list gets no request.
pub fn adapter_words() -> Vec<&'static str> {
    Adapter::ALL.iter().map(|adapter| adapter.word()).collect()
}

/// The most bytes of a key.
pub const KEY_MAX: usize = 4_096;

/// The host's key, read from the head of standard input. It is written into no line: its `Debug`
/// gives its number of bytes alone, and it has no `Display`.
pub struct Key(String);

impl Key {
    /// Reads the key at the head of `input`, in the form that the door's starter writes (the
    /// coordinator's log line 16979): the count of the key's bytes in decimal digits, 1 to
    /// `KEY_MAX` with no leading zero, and one LF; then exactly that many bytes; then one LF. The key
    /// is its bytes whole, each of visible ASCII (0x21 to 0x7E). Anything else gives no key: a count
    /// line that is no such count, an input that ends early, a key shorter or longer than its count
    /// (the byte after the count's bytes is then no LF, or a key byte is one), and a key that holds a
    /// line feed, a carriage return, a NUL byte, another control character, a space or a byte
    /// outside ASCII. The door then reads no request and sends nothing.
    pub fn read<I: BufRead>(input: &mut I) -> Option<Key> {
        let mut byte = [0u8; 1];
        let mut digits = String::new();
        loop {
            input.read_exact(&mut byte).ok()?;
            match byte[0] {
                b'\n' => break,
                b'0'..=b'9' if digits.len() < 4 && !(digits.is_empty() && byte[0] == b'0') => {
                    digits.push(char::from(byte[0]))
                }
                _ => return None,
            }
        }
        let count: usize = digits.parse().ok()?;
        if !(1..=KEY_MAX).contains(&count) {
            return None;
        }
        let mut key = vec![0u8; count];
        input.read_exact(&mut key).ok()?;
        input.read_exact(&mut byte).ok()?;
        let whole = byte[0] == b'\n' && key.iter().all(|b| (0x21..=0x7e).contains(b));
        whole.then(|| String::from_utf8(key).ok().map(Key))?
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key({} bytes)", self.0.len())
    }
}

/// Why a request got no reply: the word of a refusal line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The router gave no grant, for this cause of `LeaveCause`.
    Leave(LeaveCause),
    /// The line is not a request of A12.2's form.
    Request,
    /// The closed body does not fit its limits, also with an empty snapshot.
    Size,
    /// The grant's row names an adapter word that the door does not have.
    Adapter,
    /// No connection to the granted origin was made: nothing left.
    NotSent,
    /// The request left and no whole answer came within the adapter's one time limit; nothing is
    /// sent again.
    NoAnswer,
    /// The host answered a status other than 200.
    Status,
    /// The host's answer is not of the adapter's form.
    Answer,
}

impl Cause {
    /// The word of the refusal line.
    pub const fn as_str(self) -> &'static str {
        match self {
            Cause::Leave(cause) => cause.as_str(),
            Cause::Request => "request",
            Cause::Size => "size",
            Cause::Adapter => "adapter",
            Cause::NotSent => "not_sent",
            Cause::NoAnswer => "no_answer",
            Cause::Status => "status",
            Cause::Answer => "answer",
        }
    }
}

/// The door's arguments: the tier, the ceiling and the agreement that its starter holds.
struct Args {
    tier: Tier,
    ceiling: Tier,
    consent: Consent,
    #[cfg(feature = "loopback")]
    port: Option<u16>,
}

const USAGE: &str =
    "REFUSED usage: halvo-model --tier <tier> --ceiling <tier> --consent recorded|missing";

fn parse_args(args: &[String]) -> Option<Args> {
    let (mut tier, mut ceiling, mut consent) = (None, None, None);
    #[cfg(feature = "loopback")]
    let mut port = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next()?;
        match flag.as_str() {
            "--tier" if tier.is_none() => tier = Some(Tier::parse(value).ok()?),
            "--ceiling" if ceiling.is_none() => ceiling = Some(Tier::parse(value).ok()?),
            "--consent" if consent.is_none() => {
                consent = Some(
                    Consent::ALL
                        .into_iter()
                        .find(|consent| consent.as_str() == value)?,
                )
            }
            #[cfg(feature = "loopback")]
            flag if flag == loopback::PORT_ARGUMENT && port.is_none() => {
                port = Some(value.parse::<u16>().ok()?)
            }
            _ => return None,
        }
    }
    Some(Args {
        tier: tier?,
        ceiling: ceiling?,
        consent: consent?,
        #[cfg(feature = "loopback")]
        port,
    })
}

fn ruling_of(args: &Args) -> Ruling {
    #[cfg(feature = "loopback")]
    if let Some(port) = args.port {
        return loopback::ruling(port);
    }
    let _ = args;
    halvo_route::release::ruling()
}

/// One request line of A12.2.
struct Request {
    id: u64,
    system: String,
    prompt: String,
    snapshot: Option<String>,
    effort: Effort,
}

/// The request of `line`, or none: the door then answers the cause `request` with the id 0, and
/// writes no byte of the line.
fn parse_request(line: &str) -> Option<Request> {
    let value: Value = serde_json::from_str(line).ok()?;
    let read = || -> Option<Request> {
        let object = value.as_object()?;
        let keys = ["id", "system", "prompt", "snapshot", "grammar", "level"];
        if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
            return None;
        }
        let snapshot = match &object["snapshot"] {
            Value::Null => None,
            Value::String(text) => Some(text.clone()),
            _ => return None,
        };
        if !matches!(object["grammar"].as_str()?, "action" | "text") {
            return None;
        }
        Some(Request {
            id: value["id"].as_u64().filter(|id| *id > 0)?,
            system: object["system"].as_str()?.to_owned(),
            prompt: object["prompt"].as_str()?.to_owned(),
            snapshot,
            effort: Effort::parse(object["level"].as_str()?).ok()?,
        })
    };
    read()
}

/// The user's text: the prompt, then the snapshot between the two marker lines of A9.5.
pub fn user_text(prompt: &str, snapshot: Option<&str>) -> String {
    match snapshot {
        Some(data) => format!("{prompt}\n{OPEN}\n{data}\n{CLOSE}"),
        None => prompt.to_owned(),
    }
}

/// The closed body of T11.3 for `request`: the system text, then the user's text, with the output
/// limit `MAX_OUTPUT_TOKENS`. A snapshot that makes the body pass its size is cut at its last line
/// end that lets the body fit (the lines before that end are kept whole); when the body does not fit
/// with an empty snapshot, or with no snapshot, the cause is `size`.
fn body_of(request: &Request, tier: WireTier) -> Result<Body, Cause> {
    let make = |snapshot: Option<&str>| Body {
        tier,
        effort: request.effort,
        max_output_tokens: MAX_OUTPUT_TOKENS,
        messages: vec![
            Message {
                role: Role::System,
                content: request.system.clone(),
            },
            Message {
                role: Role::User,
                content: user_text(&request.prompt, snapshot),
            },
        ],
    };
    let fits = |body: &Body| match body.to_bytes() {
        Ok(_) => Ok(true),
        Err(BodyError::Size) => Ok(false),
        Err(_) => Err(Cause::Size),
    };
    let whole = make(request.snapshot.as_deref());
    if fits(&whole)? {
        return Ok(whole);
    }
    let Some(data) = request.snapshot.as_deref() else {
        return Err(Cause::Size);
    };
    let mut ends = vec![0];
    ends.extend(data.match_indices('\n').map(|(at, _)| at));
    if !fits(&make(Some("")))? {
        return Err(Cause::Size);
    }
    let (mut low, mut high) = (0, ends.len());
    while high - low > 1 {
        let middle = (low + high) / 2;
        if fits(&make(Some(&data[..ends[middle]])))? {
            low = middle;
        } else {
            high = middle;
        }
    }
    Ok(make(Some(&data[..ends[low]])))
}

/// What the door answers for one request: a reply line or a refusal line, each one line of JSON.
fn line_for(answer: Result<(u64, Answered), (u64, Cause)>) -> String {
    match answer {
        Ok((id, answered)) => format!(
            "{{\"id\":{id},\"text\":{},\"tokens_in\":{},\"tokens_out\":{},\"first_token_ms\":{},\"ms\":{}}}",
            serde_json::to_string(&answered.text).unwrap_or_else(|_| "\"\"".to_owned()),
            answered.tokens_in,
            answered.tokens_out,
            answered.head_ms,
            answered.ms
        ),
        Err((id, cause)) => format!("{{\"id\":{id},\"refused\":\"{}\"}}", cause.as_str()),
    }
}

/// The cause of a refusal line for an error of the adapter's one operation.
fn cause_of(error: SendError) -> Cause {
    match error {
        SendError::Adapter => Cause::Adapter,
        SendError::Origin | SendError::Client | SendError::NotSent => Cause::NotSent,
        SendError::Form => Cause::Size,
        SendError::NoAnswer => Cause::NoAnswer,
        SendError::Status(_) => Cause::Status,
        SendError::Answer => Cause::Answer,
    }
}

fn answer(
    line: &str,
    ruling: &Ruling,
    args: &Args,
    key: &Key,
) -> Result<(u64, Answered), (u64, Cause)> {
    let request = parse_request(line).ok_or((0, Cause::Request))?;
    let id = request.id;
    let origin = ruling.row(args.tier).map_or("", HostRow::origin);
    let made = grant(
        ruling,
        args.tier,
        args.ceiling,
        &[PartKind::Text],
        origin,
        args.consent,
    )
    .map_err(|cause| (id, Cause::Leave(cause)))?;
    let tier = match args.tier {
        Tier::LowCostHosted => WireTier::LowCostHosted,
        Tier::Frontier => WireTier::Frontier,
        Tier::OnDevice => return Err((id, Cause::Leave(LeaveCause::Local))),
    };
    let body = body_of(&request, tier).map_err(|cause| (id, cause))?;
    send_under_grant(made, &key.0, &body)
        .map(|answered| (id, answered))
        .map_err(|error| (id, cause_of(error)))
}

/// The door: `args` are its arguments, `input` its standard input, `out` its standard output and
/// `err` its standard error. Returns its exit status: 0 at the end of the input, 2 when the
/// arguments or the key are refused (with one line `REFUSED usage...` or `REFUSED key` on
/// `err`, which names no key), 1 when a line cannot be written.
pub fn run<I: BufRead, O: Write, E: Write>(
    args: &[String],
    mut input: I,
    mut out: O,
    mut err: E,
) -> i32 {
    let Some(args) = parse_args(args) else {
        let _ = writeln!(err, "{USAGE}");
        return 2;
    };
    let Some(key) = Key::read(&mut input) else {
        let _ = writeln!(err, "REFUSED key");
        return 2;
    };
    let ruling = ruling_of(&args);
    let mut line = String::new();
    loop {
        line.clear();
        match input.read_line(&mut line) {
            Ok(0) => return 0,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => line.clear(),
            Err(_) => return 1,
        }
        let text = line.strip_suffix('\n').unwrap_or(&line);
        let text = text.strip_suffix('\r').unwrap_or(text);
        let written = line_for(answer(text, &ruling, &args, &key));
        if writeln!(out, "{written}")
            .and_then(|()| out.flush())
            .is_err()
        {
            return 1;
        }
    }
}
