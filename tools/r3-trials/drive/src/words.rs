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

/// The executor's role word of a select control.
pub const SELECT_ROLE: &str = "combobox";

/// The cause of a select that the page did not take: on a line that is no select control, the line's
/// reference and the executor's own role word for it and no text of the page; on a select control,
/// the page's refusal as for every other action (ruling CP-179 (2) (c)).
pub fn select_cause(reference: &str, role: &str, fault: &str) -> String {
    if role == SELECT_ROLE {
        page_refused(fault)
    } else {
        format!("{reference} is a {role} line and no select control; a select names the line of the control that holds the option")
    }
}

/// The line of an action that the harness carried out (ruling CP-179 (2) (c)): its words and
/// " (done; a request was sent)" when a state-changing request of it left, " (done; the page shows
/// no change)" when the next snapshot's text equals the one before it, and " (done)" else.
pub fn carried(a: &Action, request_sent: bool, page_unchanged: bool) -> String {
    let how = if request_sent {
        "done; a request was sent"
    } else if page_unchanged {
        "done; the page shows no change"
    } else {
        "done"
    };
    format!("{} ({how})", action_words(a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Key;

    fn click() -> Action {
        Action::Click {
            reference: "e12".into(),
        }
    }

    #[test]
    fn a_carried_out_action_says_done_and_what_it_did() {
        assert_eq!(carried(&click(), false, false), "click e12 (done)");
        assert_eq!(
            carried(&click(), false, true),
            "click e12 (done; the page shows no change)"
        );
        assert_eq!(
            carried(&click(), true, false),
            "click e12 (done; a request was sent)"
        );
        assert_eq!(
            carried(&click(), true, true),
            "click e12 (done; a request was sent)"
        );
    }

    #[test]
    fn a_select_on_a_line_that_is_no_select_control_names_its_role() {
        assert_eq!(
            select_cause("e10", "option", "select:no such option"),
            "e10 is a option line and no select control; a select names the line of the control that holds the option"
        );
        assert_eq!(
            select_cause("e3", "heading", "select:no such option"),
            "e3 is a heading line and no select control; a select names the line of the control that holds the option"
        );
        assert_eq!(
            select_cause("e9", SELECT_ROLE, "select:no such option"),
            "the page did not take it (select:no such option)"
        );
    }

    #[test]
    fn every_refusal_and_no_other_line_holds_the_mark_of_a_refusal() {
        let select = Action::Select {
            reference: "e10".into(),
            slot: "instrument".into(),
        };
        let refusals = [
            refused(&click(), &no_reference("e12")),
            refused(&click(), CREDENTIAL),
            refused(&click(), CREDENTIAL_RECOVERY),
            refused(&click(), NO_TARGET),
            refused(&click(), NOT_THAT_STEP),
            refused(&click(), &no_slot("payee")),
            refused(&click(), &page_refused("click:gone")),
            refused(&select, &select_cause("e10", "option", "x")),
            reply_refused("{x", Refused::Form),
            reply_refused("{}", Refused::Fields),
        ];
        for r in &refusals {
            assert!(r.contains("(refused"), "{r}");
        }
        let others = [
            carried(&click(), false, false),
            carried(&click(), false, true),
            carried(&click(), true, false),
            NAVIGATE_START.to_string(),
            HANDBACK.to_string(),
            frame("Goal.", &[], &[], &[], NEXT),
            repair_question(3, "click", "button \"Pay now\""),
        ];
        for o in &others {
            assert!(!o.contains("(refused"), "{o}");
        }
    }

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
