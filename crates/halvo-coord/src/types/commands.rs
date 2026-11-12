//! The page's commands (I8.1), from which the app registers them and `commands.json` is written
//! (I8.3).

use super::{ArgType, Cause, Choice};
use serde::Serialize;

/// An argument of a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Arg {
    /// `name`.
    pub name: &'static str,
    /// `type`: `count`, `id`, `name` or `text`.
    #[serde(rename = "type")]
    pub kind: ArgType,
    /// `when`: the choice for which the argument is given, or none when it always is.
    pub when: Option<Choice>,
    /// `values`: the closed list of a name, or none.
    pub values: Option<&'static [&'static str]>,
}

/// A command of I8.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Command {
    /// `command`: its name.
    #[serde(rename = "command")]
    pub name: &'static str,
    /// `args`.
    pub args: &'static [Arg],
    /// `replies`: the stems of the fixture files that hold one of its replies.
    pub replies: &'static [&'static str],
    /// `errors`: the causes with which it can be refused.
    pub errors: &'static [Cause],
}

const STATES: [&str; 21] = [
    "state-empty",
    "state-ten",
    "state-needs_confirm_page",
    "state-needs_confirm_native",
    "state-needs_sign_in",
    "state-needs_link_account",
    "state-needs_stuck",
    "state-needs_check",
    "state-needs_setup",
    "state-needs_ready",
    "state-needs_show",
    "state-setup_three",
    "state-card",
    "state-problem",
    "state-off",
    "state-working",
    "state-queue",
    "state-no_service",
    "state-needs_sign_in_bind",
    "state-needs_confirm_mixed",
    "state-needs_confirm_changed",
];

const NEXT: [&str; 23] = [
    "state-empty",
    "state-ten",
    "state-needs_confirm_page",
    "state-needs_confirm_native",
    "state-needs_sign_in",
    "state-needs_link_account",
    "state-needs_stuck",
    "state-needs_check",
    "state-needs_setup",
    "state-needs_ready",
    "state-needs_show",
    "state-setup_three",
    "state-card",
    "state-problem",
    "state-off",
    "state-working",
    "state-queue",
    "state-no_service",
    "state-needs_sign_in_bind",
    "state-needs_confirm_mixed",
    "state-needs_confirm_changed",
    "reply-next-same",
    "reply-closed",
];

const CHOICES: [&str; 10] = [
    "reject",
    "allow",
    "decline",
    "done",
    "try_again",
    "went_through",
    "none",
    "choose",
    "skip",
    "start",
];

const PARAMS: [&str; 9] = [
    "start",
    "payee",
    "instrument",
    "reply_senders",
    "workbook",
    "sheet",
    "range",
    "source",
    "seller",
];

const ACTIONS: [&str; 4] = ["run_now", "stop", "switch_off", "switch_on"];

const fn arg(name: &'static str, kind: ArgType) -> Arg {
    Arg {
        name,
        kind,
        when: None,
        values: None,
    }
}

const ITEM: Arg = arg("item", ArgType::Id);
const AGENT: Arg = arg("agent", ArgType::Id);
const OK: &[&str] = &["reply-ok"];

/// The commands of I8.1 in their order, each with its arguments, the stems of its reply files
/// and its error causes. The `choice` of `agents_answer` takes the ten choices of I3.1 and not
/// `present`, which the page sends as `agents_present`.
pub const LIST: [Command; 7] = [
    Command {
        name: "agents_snapshot",
        args: &[],
        replies: &STATES,
        errors: &[],
    },
    Command {
        name: "agents_next",
        args: &[arg("after", ArgType::Count)],
        replies: &NEXT,
        errors: &[],
    },
    Command {
        name: "agents_card",
        args: &[AGENT],
        replies: &["state-card"],
        errors: &[
            Cause::UnknownAgent,
            Cause::NoService,
            Cause::Busy,
            Cause::Malformed,
        ],
    },
    Command {
        name: "agents_answer",
        args: &[
            ITEM,
            Arg {
                values: Some(&CHOICES),
                ..arg("choice", ArgType::Name)
            },
            Arg {
                when: Some(Choice::Choose),
                values: Some(&PARAMS),
                ..arg("param", ArgType::Name)
            },
            Arg {
                when: Some(Choice::Choose),
                ..arg("n", ArgType::Count)
            },
        ],
        replies: OK,
        errors: &[
            Cause::Stale,
            Cause::Presenter,
            Cause::Early,
            Cause::State,
            Cause::UnknownItem,
            Cause::NoService,
            Cause::Busy,
            Cause::Malformed,
        ],
    },
    Command {
        name: "agents_setup_typed",
        args: &[
            ITEM,
            Arg {
                values: Some(&PARAMS),
                ..arg("param", ArgType::Name)
            },
            arg("text", ArgType::Text),
        ],
        replies: OK,
        errors: &[
            Cause::Stale,
            Cause::State,
            Cause::UnknownItem,
            Cause::Malformed,
        ],
    },
    Command {
        name: "agents_present",
        args: &[ITEM],
        replies: OK,
        errors: &[
            Cause::Stale,
            Cause::Presenter,
            Cause::UnknownItem,
            Cause::NoService,
            Cause::Malformed,
        ],
    },
    Command {
        name: "agents_control",
        args: &[
            AGENT,
            Arg {
                values: Some(&ACTIONS),
                ..arg("action", ArgType::Name)
            },
        ],
        replies: OK,
        errors: &[
            Cause::Hold,
            Cause::State,
            Cause::Budget,
            Cause::QueueFull,
            Cause::UnknownAgent,
            Cause::NoService,
            Cause::Busy,
            Cause::Malformed,
        ],
    },
];

/// The text of `commands.json`: `LIST` as compact JSON on one line with one LF at its end.
pub fn json() -> String {
    let mut text = serde_json::to_string(&LIST).expect("the list serializes");
    text.push('\n');
    text
}
