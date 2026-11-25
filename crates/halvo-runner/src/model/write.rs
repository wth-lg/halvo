//! The writer of a reply's body (r3 U7.4; contracts A11.4, A12.4, A12.7, A13.6). The draft step of a reply asks it
//! for the body of each message that the list kept, in the list's order and before the first draft, and the draft
//! and the send's card take that body whole with its source. The body is the writer model's text, asked through
//! `ask` (crate::model::ask) and through nothing else, and cut at `reply_body` by `cut`, where all of these hold: the
//! router's `pick` for the kind `write` names the run's own tier (crate::route); the person agreed to that tier
//! (`Runner::agreed`); no earlier body of the step ended the writer; and the model's answer is a reply whose cut
//! leaves a body of at most `BODY_JSON_MAX` bytes of JSON. In every other case the body is the const text of A11.4
//! with the source `const`: a pick that hands the call back or names another tier, and a missing agreement, ask
//! nothing (no hold, no key, no door); the person's cap (`over_cap`) and an agreement taken back at the hold
//! (`no_agreement`) end the writer for the rest of the step; a refusal of the door, a cut that leaves no body and a
//! body over the bound give the const text for that message. A message whose own newest row is `drafted` with its
//! draft, a draft of an earlier run that waits, gets no second body: no request, the const text. A missing key
//! (`no_model_key`) and every other end of `ask` end the run as `ask` says, before any draft.

use halvo_connect::Message;
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::Source;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{Cause, Outcome};

use super::ask::{self, Answer, Grammar, ModelRequest};
use super::texts::BOUNDARY;
use super::Hosted;
use crate::route::{self, Call};
use crate::run::{Runner, Stop};

#[cfg(test)]
mod tests;

/// The goal text of a reply (A9.5), the file crates/halvo-runner/prompts/goal-reply.txt, a copy byte for byte of
/// the driver trial's tools/r3-trials/drive/prompts/goal-reply.txt; a request holds it with its end trimmed, as the
/// trial's write run does.
const GOAL_REPLY: &str = include_str!("../../prompts/goal-reply.txt");

/// The opening mark of a thinking block, one of the closed list of two literal strings (A12.4).
pub(crate) const THINK_OPEN: &str = "<think>";

/// The closing mark of a thinking block, the other of the closed list (A12.4).
pub(crate) const THINK_CLOSE: &str = "</think>";

/// The most bytes of a reply's request line: the door's own bound of the body that it sends, `MAX_BODY_BYTES` of
/// halvo-route-wire, so that the hold names what the door would send.
pub(crate) const LINE_MAX: usize = 131_072;

/// The most bytes of the JSON text of a body's value, its pieces of `OwnValue` as a list: ten such bodies, each
/// beside a `to` of its most 320 bytes at six bytes of JSON a byte and a subject of at most 1,000 bytes, fit the one
/// confirmation frame of a send step (`FRAME_JSON_MAX`, 65,536 bytes).
pub(crate) const BODY_JSON_MAX: usize = 3_072;

/// The body of a reply and its source: `model` for the writer model's text, `const` for the const text of A11.4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Body {
    pub(crate) text: String,
    pub(crate) source: Source,
}

impl Body {
    /// The const text of A11.4 with the source `const`.
    pub(crate) fn constant() -> Body {
        Body {
            text: crate::connect::const_body().to_owned(),
            source: Source::Const,
        }
    }
}

/// The writer of one draft step: whether an end of `ask` ended it for the rest of the step.
#[derive(Default)]
pub(crate) struct Writer {
    ended: bool,
}

/// The body of a model output `output` at `reply_body` (A12.4, by the rule of A13.6): none where an opening mark has no
/// closing mark after it; else the text from the end of the last closing mark on, or the whole output where it holds
/// no closing mark, with white space trimmed at both ends; none where that text is empty or still holds a mark.
pub(crate) fn cut(output: &str) -> Option<&str> {
    let close = output.rfind(THINK_CLOSE);
    if let Some(open) = output.rfind(THINK_OPEN) {
        if close.is_none_or(|close| close < open) {
            return None;
        }
    }
    let body = close
        .map_or(output, |close| &output[close + THINK_CLOSE.len()..])
        .trim();
    if body.is_empty() || body.contains(THINK_OPEN) || body.contains(THINK_CLOSE) {
        return None;
    }
    Some(body)
}

/// The bytes of the JSON text of `body` as a bound field's value: its pieces of `OwnValue` as a list of strings.
pub(crate) fn json_bytes(body: &str) -> usize {
    let value = OwnValue::new(body);
    let pieces = value.pieces();
    let strings: usize = pieces
        .iter()
        .map(|piece| serde_json::Value::String(piece.clone()).to_string().len())
        .sum();
    2 + strings + pieces.len().saturating_sub(1)
}

/// The text of `message` that a request holds (A12.7): its sender, its subject and its body, in the form of the trial's
/// `message_text`, and nothing else of the mail.
fn message_text(message: &Message) -> String {
    format!(
        "From: {}\nSubject: {}\n{}",
        message.sender(),
        message.subject(),
        message.body()
    )
}

/// `request` with its snapshot cut so that its line fits `LINE_MAX` bytes at the longest id: the snapshot up to its
/// last line end at which the line fits, the lines before it kept whole, as the door cuts its own body.
fn fitted(mut request: ModelRequest) -> ModelRequest {
    let fits = |request: &ModelRequest| ask::request_line(u64::MAX, request).len() <= LINE_MAX;
    if fits(&request) {
        return request;
    }
    let data = request.snapshot.take().unwrap_or_default();
    let mut ends = vec![0];
    ends.extend(data.match_indices('\n').map(|(at, _)| at));
    let with = |end: usize| ModelRequest {
        snapshot: Some(data[..end].to_owned()),
        ..request.clone()
    };
    let (mut low, mut high) = (0, ends.len());
    while high - low > 1 {
        let middle = (low + high) / 2;
        if fits(&with(ends[middle])) {
            low = middle;
        } else {
            high = middle;
        }
    }
    with(ends[low])
}

impl Runner {
    /// The body of the reply to `message` of a run whose load is `loaded`, by the rules of this module; `waits` where
    /// the message's own newest row is `drafted` with its draft.
    pub(crate) fn reply_body(
        &mut self,
        writer: &mut Writer,
        loaded: &LoadReply,
        message: &Message,
        waits: bool,
    ) -> Result<Body, Stop> {
        if waits || writer.ended {
            return Ok(Body::constant());
        }
        let Ok(level) = route::level(loaded, Call::Write) else {
            return Ok(Body::constant());
        };
        let tier = loaded.config.tier;
        if !self.agreed(tier) {
            return Ok(Body::constant());
        }
        if self.hosted.is_none() {
            self.hosted = Some(Hosted {
                tier,
                door: None,
                next_line: 1,
            });
        }
        let request = fitted(ModelRequest {
            system: BOUNDARY.to_owned(),
            prompt: GOAL_REPLY.trim_end().to_owned(),
            snapshot: Some(message_text(message)),
            grammar: Grammar::Text,
            level,
        });
        match self.ask(&request) {
            Ok(Answer::Reply { text, .. }) => Ok(cut(&text)
                .filter(|body| json_bytes(body) <= BODY_JSON_MAX)
                .map_or_else(Body::constant, |body| Body {
                    text: body.to_owned(),
                    source: Source::Model,
                })),
            Ok(Answer::Refused(_)) => Ok(Body::constant()),
            Err(Stop::End(Outcome::HandedBack, Some(Cause::OverCap | Cause::NoAgreement))) => {
                writer.ended = true;
                Ok(Body::constant())
            }
            Err(stop) => Err(stop),
        }
    }
}
