use halvo_connect::{ReplyRule, Token};
use serde_json::Value;

use super::fixture::repo;

pub struct Seed(Value);

impl Seed {
    pub fn load(n: u8) -> Seed {
        let path = repo()
            .join("tools")
            .join("fixture-server")
            .join("variants")
            .join("data")
            .join(format!("seed-{n}.json"));
        let bytes = std::fs::read(path).expect("the seed file");
        Seed(serde_json::from_slice(&bytes).expect("the seed is JSON"))
    }

    pub fn get(&self, pointer: &str) -> &Value {
        self.0.pointer(pointer).expect("a key of the seed")
    }

    fn text(&self, pointer: &str) -> &str {
        self.get(pointer).as_str().expect("a string of the seed")
    }

    pub fn token(&self) -> Token {
        Token::new(self.token_text().to_owned()).expect("the token of the seed is a token")
    }

    pub fn token_text(&self) -> &str {
        self.text("/credentials/token")
    }

    pub fn page_marker(&self) -> &str {
        self.text("/page_marker")
    }

    pub fn rule(&self) -> ReplyRule {
        let senders = self
            .get("/bindings/reply/slots")
            .as_array()
            .expect("the slots of reply")
            .iter()
            .find(|slot| slot["name"] == "reply_senders")
            .and_then(|slot| slot["value"].as_array())
            .expect("the slot reply_senders")
            .iter()
            .map(|sender| sender.as_str().expect("an address").to_owned())
            .collect();
        ReplyRule::new(self.text("/mail/address").to_owned(), senders)
    }

    pub fn subject(&self, id: &str) -> Option<&str> {
        self.get("/mail/inbox")
            .as_array()
            .expect("the inbox")
            .iter()
            .chain(std::iter::once(self.get("/later/message")))
            .find(|message| message["id"] == id)
            .and_then(|message| message["subject"].as_str())
    }

    pub fn event_subject(&self, id: &str) -> Option<&str> {
        self.get("/calendar/events")
            .as_array()
            .expect("the events")
            .iter()
            .chain(std::iter::once(self.get("/later/event")))
            .find(|event| event["id"] == id)
            .and_then(|event| event["subject"].as_str())
    }

    pub fn end(&self, task: &str) -> &Value {
        self.get(&format!("/end/{task}"))
    }

    pub fn later(&self, pointer: &str) -> &str {
        self.text(&format!("/later/{pointer}"))
    }
}
