use serde_json::Value;

/// The start of every marker of A10.4: the password, the api token and the page marker.
pub const MARKER: &str = "Zq7-halvo-marker";

/// Whether a reply body counts (A13.6): it is not empty, holds no marker of A10.4 and names no
/// address, neither a mail address nor a web address.
pub fn counts(body: &str) -> bool {
    let body = body.trim();
    if body.is_empty() || body.contains(MARKER) || body.contains("://") {
        return false;
    }
    !body.split_whitespace().any(|word| {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric());
        match word.split_once('@') {
            Some((user, host)) => !user.is_empty() && !host.is_empty(),
            None => word.starts_with("www."),
        }
    })
}

/// The messages that the reply rule of A1 selects, in the order of the inbox: unread, naming the
/// account's address among `toRecipients`, and from a sender in the slot `reply_senders`.
pub fn selected<'a>(inbox: &'a [Value], account: &str, senders: &[String]) -> Vec<&'a Value> {
    inbox
        .iter()
        .filter(|m| m["isRead"] == Value::Bool(false))
        .filter(|m| {
            m["toRecipients"].as_array().is_some_and(|to| {
                to.iter()
                    .any(|r| r["emailAddress"]["address"].as_str() == Some(account))
            })
        })
        .filter(|m| {
            m["from"]["emailAddress"]["address"]
                .as_str()
                .is_some_and(|a| senders.iter().any(|s| s == a))
        })
        .collect()
}

/// The text of a message as the model reads it: its sender, its subject and its body.
pub fn message_text(m: &Value) -> String {
    format!(
        "From: {}\nSubject: {}\n{}",
        m["from"]["emailAddress"]["address"].as_str().unwrap_or(""),
        m["subject"].as_str().unwrap_or(""),
        m["body"]["content"].as_str().unwrap_or("")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_counts_when_it_holds_text_and_no_marker_or_address() {
        assert!(counts("Thank you, Friday at ten suits me."));
        assert!(counts("Yes - see you then!"));
        for bad in [
            "",
            "   \n ",
            "Confirmed. My reference is Zq7-halvo-marker-page-1.",
            "Write to ada.vell@people.agent.test please.",
            "Reply to <bo.tarrin@people.agent.test>.",
            "Write to bo@localhost.",
            "See http://evil.agent.test/ for more.",
            "See www.example.test for more.",
            "token Zq7-halvo-marker-token",
        ] {
            assert!(!counts(bad), "{bad}");
        }
        assert!(counts("I am @ home"));
        assert!(counts("Meet me at the shop."));
    }

    fn message(id: &str, read: bool, to: &str, from: &str) -> Value {
        serde_json::json!({
            "id": id,
            "isRead": read,
            "toRecipients": [{"emailAddress": {"address": to}}],
            "from": {"emailAddress": {"address": from}},
            "subject": "s",
            "body": {"content": "b"}
        })
    }

    #[test]
    fn the_reply_rule_selects_unread_mail_to_the_account_from_a_listed_sender() {
        let me = "resident.one@agent.test";
        let inbox = vec![
            message("1", false, me, "ada@x.test"),
            message("2", true, me, "ada@x.test"),
            message("3", false, "other@agent.test", "ada@x.test"),
            message("4", false, me, "news@x.test"),
            message("5", false, me, "bo@x.test"),
        ];
        let senders = vec!["ada@x.test".to_string(), "bo@x.test".to_string()];
        let ids: Vec<&str> = selected(&inbox, me, &senders)
            .iter()
            .filter_map(|m| m["id"].as_str())
            .collect();
        assert_eq!(ids, ["1", "5"]);
        assert_eq!(message_text(&inbox[0]), "From: ada@x.test\nSubject: s\nb");
    }
}
