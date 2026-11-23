//! The person's key for a model's host in the app (r4 U3.18).
#![cfg(any(target_os = "macos", windows))]

use std::sync::Arc;

use halvo_agent_client::OwnerLink;
use serde::Serialize;
use serde_json::{json, Value};

use crate::product::{hex, json_str, link_over, random16, result_of, StreamCall};

/// The most bytes of a key that a save takes.
pub const KEY_MAX: usize = 1024;

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
    use halvo_agent_client::LinkError;
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
}
