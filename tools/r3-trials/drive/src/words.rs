//! The harness's own words in a request to the model, and nothing else (ruling CP-179 (2) (a), A9.5):
//! the frame around the goal text, the lines of the list "Done so far:" with their causes, and the
//! questions. The four prompt files hold the boundary text and the goal texts; the snapshot holds the
//! page's text. `run.sh --prompt-lines` prints this file's SHA-256 on a prompt line.

use crate::action::{Action, Refused};

/// The question of a compile request.
pub const NEXT: &str = "Give the next action.";

/// The first line of a compile's list: the harness opened the start page.
pub const NAVIGATE_START: &str = "navigate start";

/// The line of a handback, the model's or the harness's own (ruling CP-177 G2).
pub const HANDBACK: &str = "handback";

/// The cause of a fill, a select or a press on a line that the snapshot marks credential.
pub const CREDENTIAL: &str = "that is a credential field; the person signs in after handback";

/// The same cause in the recovery's list.
pub const CREDENTIAL_RECOVERY: &str = "that is a credential field";

/// The cause of a handback or a done where an action on a line was asked for.
pub const NO_TARGET: &str = "no target";

/// The cause of a recovery reply that is not the stored step.
pub const NOT_THAT_STEP: &str = "not that step on this page";

/// The user text of an action request: the goal, the slot names, the kept names, the list of what
/// was done so far and the question; page text goes in the snapshot alone.
pub fn frame(
    goal: &str,
    slots: &[String],
    kept: &[String],
    history: &[String],
    ask: &str,
) -> String {
    let list = |v: &[String]| {
        if v.is_empty() {
            "none".to_string()
        } else {
            v.join(", ")
        }
    };
    let mut out = format!(
        "{}\nSlots: {}\nKept: {}\nDone so far:",
        goal.trim_end(),
        list(slots),
        list(kept)
    );
    if history.is_empty() {
        out.push_str(" nothing");
    }
    for (i, h) in history.iter().enumerate() {
        out.push_str(&format!("\n{}. {h}", i + 1));
    }
    out.push('\n');
    out.push_str(ask);
    out
}

/// The question of a recovery request: the stored step that found no control, by its verb word and
/// the words of its target.
pub fn repair_question(n: usize, verb: &str, target: &str) -> String {
    format!(
        "Step {n} of the saved steps found no control on this page: {verb} {target}. Give the action that does this step on this page."
    )
}

/// The action as one short line of the list: the verb and its fields.
pub fn action_words(a: &Action) -> String {
    match a {
        Action::Click { reference } => format!("click {reference}"),
        Action::Fill { reference, slot } => format!("fill {reference} slot {slot}"),
        Action::Select { reference, slot } => format!("select {reference} slot {slot}"),
        Action::Press { reference, key } => format!("press {reference} key {}", key.as_str()),
        Action::Extract { reference, name } => format!("extract {reference} name {name}"),
        Action::Handback => HANDBACK.to_string(),
        Action::Done => "done".to_string(),
    }
}

/// The cause of a reply that is no action of the grammar.
pub fn parse_cause(r: Refused) -> &'static str {
    match r {
        Refused::Form => "action_form",
        Refused::Fields => "action_fields",
    }
}

/// The line of a reply that is no action of the grammar: its first 120 characters and the cause.
pub fn reply_refused(reply: &str, r: Refused) -> String {
    format!(
        "{} (refused: {})",
        reply.chars().take(120).collect::<String>(),
        parse_cause(r)
    )
}

/// The line of an action that the harness or the page refused, with its cause.
pub fn refused(a: &Action, cause: &str) -> String {
    format!("{} (refused: {cause})", action_words(a))
}

/// The cause of an action on a reference that the page does not hold.
pub fn no_reference(reference: &str) -> String {
    format!("no {reference} on this page")
}

/// The cause of a fill or a select that names a slot with no value.
pub fn no_slot(slot: &str) -> String {
    format!("no slot {slot}")
}

/// The cause of an action that the page did not take, with the web library's fault.
pub fn page_refused(fault: &str) -> String {
    format!("the page did not take it ({fault})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Key;

    #[test]
    fn the_prompt_holds_the_goal_the_names_and_the_steps_and_no_page_text() {
        let p = frame(
            "Task: pay.\n",
            &["payee".into(), "instrument".into()],
            &[],
            &[NAVIGATE_START.into(), HANDBACK.into()],
            NEXT,
        );
        assert_eq!(
            p,
            "Task: pay.\nSlots: payee, instrument\nKept: none\nDone so far:\n1. navigate start\n2. handback\nGive the next action."
        );
        let q = frame("Task.", &[], &["amount_due".into()], &[], "Go.");
        assert_eq!(
            q,
            "Task.\nSlots: none\nKept: amount_due\nDone so far: nothing\nGo."
        );
    }

    #[test]
    fn the_history_line_names_the_fields() {
        let a = Action::Press {
            reference: "e2".into(),
            key: Key::Tab,
        };
        assert_eq!(action_words(&a), "press e2 key Tab");
        assert_eq!(action_words(&Action::Handback), "handback");
    }

    #[test]
    fn each_refusal_line_names_its_cause_as_built() {
        let click = Action::Click {
            reference: "e4".into(),
        };
        assert_eq!(
            refused(&click, &no_reference("e4")),
            "click e4 (refused: no e4 on this page)"
        );
        assert_eq!(
            refused(&click, &page_refused("select:no such option")),
            "click e4 (refused: the page did not take it (select:no such option))"
        );
        assert_eq!(
            reply_refused("{x", Refused::Form),
            "{x (refused: action_form)"
        );
        assert_eq!(parse_cause(Refused::Fields), "action_fields");
        assert_eq!(
            repair_question(4, "click", "link \"Pay bill\""),
            "Step 4 of the saved steps found no control on this page: click link \"Pay bill\". Give the action that does this step on this page."
        );
    }
}
