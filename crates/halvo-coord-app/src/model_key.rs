//! The person's key for a model's host in the app (r4 U3.18).
#![cfg(any(target_os = "macos", windows))]

use std::sync::Arc;

use halvo_agent_client::{LinkError, OwnerLink};
use serde::Serialize;
use serde_json::{json, Value};

use crate::product::{hex, json_str, link_over, random16, result_of, StreamCall};

/// The most bytes of a key that a save takes.
pub const KEY_MAX: usize = 1024;

/// The page's three commands of the key, which `ModelKey::call` answers.
pub const COMMANDS: [&str; 3] = ["model_key_state", "save_model_key", "remove_model_key"];

/// The host of this build: the origin of the rows of `halvo_route::release::ruling()` without its `https://`, where
/// every row names the same one origin, and none where the ruling holds no row or its rows name two origins. With
/// main's data it is none.
pub fn host() -> Option<String> {
    host_of(
        halvo_route::release::ruling()
            .rows()
            .iter()
            .map(|row| row.origin()),
    )
}

/// The host of `origins`, none where there is none or two of them differ.
fn host_of<'a>(origins: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut origins = origins.into_iter();
    let first = origins.next()?;
    if origins.any(|origin| origin != first) {
        return None;
    }
    first.strip_prefix("https://").map(str::to_owned)
}

/// What the page is told of the person's key: `saved`, `none`, or `not_known`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    Saved,
    None,
    NotKnown,
}

/// Why a save or a removal did not finish: `no_key`, a text that is no key and went nowhere, or `not_finished`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCause {
    NoKey,
    NotFinished,
}

/// A reply of a command of the key to the window. None holds the key, a part of it, its length or a sign made from
/// it: for any two keys the replies are the same bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum KeyReply {
    /// `model_key_state` in a build with a host: `{"host":<host>,"state":<state>}`.
    Host { host: String, state: KeyState },
    /// `model_key_state` in a build with no host: `{"host":null}`, with nothing sent.
    NoHost { host: () },
    /// A save or a removal that the store took: `{"state":<the state that it leaves>}`.
    Done { state: KeyState },
    /// A save or a removal that did not finish: `{"error":<cause>}`.
    Error { error: KeyCause },
}

impl From<Result<KeyState, KeyCause>> for KeyReply {
    fn from(outcome: Result<KeyState, KeyCause>) -> KeyReply {
        match outcome {
            Ok(state) => KeyReply::Done { state },
            Err(error) => KeyReply::Error { error },
        }
    }
}

/// The person's key in the agent store, through ONE coordinator body on an owner stream for each call. It keeps no
/// key: a key that it is handed or reads goes into one request or one answer and nowhere else.
#[derive(Clone)]
pub struct ModelKey {
    link: Arc<dyn OwnerLink>,
}

impl ModelKey {
    /// The key's calls over `link`.
    pub fn new(link: Arc<dyn OwnerLink>) -> ModelKey {
        ModelKey { link }
    }

    /// The key's calls over the owner streams that `owner_stream` gives.
    pub fn over(owner_stream: StreamCall) -> ModelKey {
        ModelKey::new(link_over(owner_stream))
    }

    /// The key's calls where no service answers: each request fails as one that meets no service.
    pub fn no_service() -> ModelKey {
        ModelKey::new(Arc::new(NoService))
    }

    /// The reply of the page's command `command` with `args` in a build whose host is `host`, none for a name that
    /// is not one of `COMMANDS`. `model_key_state` sends nothing where there is no host; `save_model_key` saves the
    /// text of the key `key` of `args`, the empty text where there is none.
    pub fn call(&self, host: Option<&str>, command: &str, args: Option<&Value>) -> Option<KeyReply> {
        Some(match command {
            "model_key_state" => match host {
                Some(host) => KeyReply::Host {
                    host: host.to_owned(),
                    state: self.state(),
                },
                None => KeyReply::NoHost { host: () },
            },
            "save_model_key" => {
                let text = args.and_then(|a| a.get("key")).and_then(Value::as_str);
                self.save(text.unwrap_or_default()).into()
            }
            "remove_model_key" => self.remove().into(),
            _ => return None,
        })
    }

    /// The state of the key: ONE body `model_key_state`; `saved` where the store answers `{"stands":"yes"}`, `none`
    /// where it answers `{"stands":"no"}`, and `not_known` at every other outcome.
    pub fn state(&self) -> KeyState {
        match self.ask("model_key_state", "") {
            Some(result) if result == json!({"stands": "yes"}) => KeyState::Saved,
            Some(result) if result == json!({"stands": "no"}) => KeyState::None,
            _ => KeyState::NotKnown,
        }
    }

    /// Removes the key: ONE body `drop_model_key`; `none` where the store answers `{"stored":0}`, and
    /// `not_finished` at every other outcome.
    pub fn remove(&self) -> Result<KeyState, KeyCause> {
        match self.ask("drop_model_key", "") {
            Some(result) if result == json!({"stored": 0}) => Ok(KeyState::None),
            _ => Err(KeyCause::NotFinished),
        }
    }

    /// Saves the key of `text` in ONE body `put_model_key`, whole and byte for byte, in the place of the key that
    /// stands: `saved` where the store answers `{"stored":1}`. A text that is no key (`key_of`) is `no_key` and is
    /// sent nowhere; every other outcome is `not_finished`.
    pub fn save(&self, text: &str) -> Result<KeyState, KeyCause> {
        let key = key_of(text).ok_or(KeyCause::NoKey)?;
        match self.ask("put_model_key", &format!(r#","key":{}"#, json_str(key))) {
            Some(result) if result == json!({"stored": 1}) => Ok(KeyState::Saved),
            _ => Err(KeyCause::NotFinished),
        }
    }

    /// The result of ONE coordinator body `op` with `workflow` and `agent` null and the keys `more`, none where the
    /// request fails, the owner refuses it or its reply does not read.
    fn ask(&self, op: &str, more: &str) -> Option<Value> {
        let id = hex(&random16().ok()?);
        let request = format!(
            r#"{{"v":1,"id":"{id}","request":"coordinator_write","body":{{"op":"{op}","workflow":null,"agent":null{more}}}}}"#
        );
        let reply = self.link.request(request.as_bytes()).ok()?;
        result_of(&reply, Some(&id)).ok()
    }
}

/// The link where no service answers.
struct NoService;

impl OwnerLink for NoService {
    fn request(&self, _request: &[u8]) -> Result<Vec<u8>, LinkError> {
        Err(LinkError)
    }
}

/// The key of `text`: the text without blanks, tabs and line breaks at its two ends, where it is at least one byte,
/// at most `KEY_MAX` bytes and every byte is a visible sign of ASCII (0x21 to 0x7E); else none.
fn key_of(text: &str) -> Option<&str> {
    let key = text.trim_matches(&[' ', '\t', '\n', '\r'][..]);
    let fits = !key.is_empty()
        && key.len() <= KEY_MAX
        && key.bytes().all(|b| (0x21..=0x7e).contains(&b));
    fits.then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// U3.18 sentence 1: the host is the one origin of the rows without its `https://`: none for no row, the host for
    /// one row and for two rows of the same origin, none for two origins; main's ruling gives none.
    #[test]
    fn the_host_is_the_one_origin_of_the_rows_without_https() {
        assert_eq!(host_of([]), None);
        assert_eq!(
            host_of(["https://api.host.invalid"]),
            Some("api.host.invalid".to_owned())
        );
        assert_eq!(
            host_of(["https://api.host.invalid", "https://api.host.invalid"]),
            Some("api.host.invalid".to_owned())
        );
        assert_eq!(
            host_of(["https://api.host.invalid", "https://other.host.invalid"]),
            None
        );
        assert_eq!(host(), None);
    }

    /// What the stand-in owner answers each request: a result, a refusal by its error name, or no reply.
    #[derive(Clone, Copy)]
    enum Answer {
        Result(&'static str),
        Refused(&'static str),
        NoReply,
    }

    /// A stand-in owner link that keeps each request as JSON and answers it with `answer`.
    struct Stand {
        answer: Answer,
        kept: Mutex<Vec<Value>>,
    }

    impl Stand {
        fn kept(&self) -> Vec<Value> {
            self.kept.lock().unwrap().clone()
        }
    }

    impl OwnerLink for Stand {
        fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
            let request: Value = serde_json::from_slice(request).expect("a request reads as JSON");
            let id = request["id"].to_string();
            self.kept.lock().unwrap().push(request);
            match self.answer {
                Answer::Result(result) => {
                    Ok(format!(r#"{{"v":1,"id":{id},"ok":{result}}}"#).into_bytes())
                }
                Answer::Refused(name) => {
                    Ok(format!(r#"{{"v":1,"id":{id},"error":"{name}"}}"#).into_bytes())
                }
                Answer::NoReply => Err(LinkError),
            }
        }
    }

    /// The stand-in link that answers `answer`, and the key's calls over it.
    fn stand(answer: Answer) -> (Arc<Stand>, ModelKey) {
        let link = Arc::new(Stand {
            answer,
            kept: Mutex::new(Vec::new()),
        });
        (Arc::clone(&link), ModelKey::new(link))
    }

    /// U3.18 sentence 5: a save strips blanks, tabs and line breaks at the two ends and sends the key that it takes
    /// whole in ONE body `put_model_key`; it refuses as no key the empty text, blanks alone, 1,025 bytes, a blank
    /// inside, 0x7F and a sign outside ASCII, each with no request sent.
    #[test]
    fn a_save_strips_the_ends_refuses_what_is_no_key_and_sends_a_taken_key_whole() {
        let most = "k".repeat(KEY_MAX);
        let taken = [
            ("test-value-not-a-model-key\n", "test-value-not-a-model-key"),
            (r#"test-"value"-not\a-key"#, r#"test-"value"-not\a-key"#),
            (most.as_str(), most.as_str()),
        ];
        for (text, key) in taken {
            let (link, model) = stand(Answer::Result(r#"{"stored":1}"#));
            assert_eq!(model.save(text), Ok(KeyState::Saved), "{text:?}");
            let kept = link.kept();
            assert_eq!(kept.len(), 1, "{text:?}");
            assert_eq!(kept[0]["request"], "coordinator_write");
            assert_eq!(
                kept[0]["body"],
                json!({"op": "put_model_key", "workflow": null, "agent": null, "key": key}),
                "{text:?}"
            );
        }
        let over = "k".repeat(KEY_MAX + 1);
        for text in [
            "",
            " \t\r\n",
            over.as_str(),
            "test value",
            "test\u{7f}value",
            "test-v\u{e4}lue",
        ] {
            let (link, model) = stand(Answer::Result(r#"{"stored":1}"#));
            assert_eq!(model.save(text), Err(KeyCause::NoKey), "{text:?}");
            assert!(link.kept().is_empty(), "{text:?}");
        }
    }

    /// U3.18 sentence 6: each answer of the store gives the state or the cause of the sentence, with ONE body a
    /// command: `model_key_state` is saved at stands yes, none at stands no and not known at every other answer; a
    /// save is saved at stored 1 and a removal none at stored 0, each not finished at every other answer, the
    /// refusal `store`, no reply and a reply of another shape among them. Run with two keys of different texts and
    /// lengths, the serialised replies are the same bytes.
    #[test]
    fn each_answer_of_the_store_gives_one_state_or_cause_and_no_reply_tells_one_key_from_another() {
        const HOST: &str = "api.host.invalid";
        const NOT: &str = r#"{"error":"not_finished"}"#;
        const KNOWN: &str = r#"{"host":"api.host.invalid","state":"not_known"}"#;
        let rows = [
            (
                Answer::Result(r#"{"stands":"yes"}"#),
                [r#"{"host":"api.host.invalid","state":"saved"}"#, NOT, NOT],
            ),
            (
                Answer::Result(r#"{"stands":"no"}"#),
                [r#"{"host":"api.host.invalid","state":"none"}"#, NOT, NOT],
            ),
            (
                Answer::Result(r#"{"stored":1}"#),
                [KNOWN, r#"{"state":"saved"}"#, NOT],
            ),
            (
                Answer::Result(r#"{"stored":0}"#),
                [KNOWN, NOT, r#"{"state":"none"}"#],
            ),
            (Answer::Refused("store"), [KNOWN, NOT, NOT]),
            (Answer::NoReply, [KNOWN, NOT, NOT]),
            (Answer::Result(r#"{"key":null}"#), [KNOWN, NOT, NOT]),
        ];
        let replies = |key: &str| -> Vec<String> {
            let mut all = Vec::new();
            for (answer, expected) in &rows {
                let (link, model) = stand(*answer);
                let args = json!({ "key": key });
                let got = [
                    model.call(Some(HOST), "model_key_state", None),
                    model.call(Some(HOST), "save_model_key", Some(&args)),
                    model.call(Some(HOST), "remove_model_key", None),
                ]
                .map(|reply| serde_json::to_string(&reply.expect("a command of the key")).unwrap());
                assert_eq!(got, expected.map(str::to_owned), "{expected:?}");
                let ops: Vec<Value> = link.kept().iter().map(|r| r["body"]["op"].clone()).collect();
                assert_eq!(ops, ["model_key_state", "put_model_key", "drop_model_key"]);
                all.extend(got);
            }
            all
        };
        assert_eq!(
            replies("test-value-not-a-model-key"),
            replies("test-value-not-a-model-key-of-another-length-0123456789")
        );
    }
}
