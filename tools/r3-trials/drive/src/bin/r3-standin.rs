//! The stand-in child of the driver trial's control (ruling CP-173 E1): no model. It loads nothing,
//! takes no lock and opens no file but the one that `--weights` names, whose one line is its mode
//! (`right`, `effect_early`, `option_then_right`, `sign_in_click`, `option_line`,
//! `extract_then_click`, `option_twice_then_right`, `extract_to_cap` or `effect_twice`; the last walks
//! as `right` does, and on the control's slow page it clicks the effect button again while the form
//! still stands). It prints
//! `ready standin`, then answers each request line of A12.2 with one reply line whose text is one
//! action of the grammar `action` and whose two token counts are 0, and it ends at the end of its
//! input. It picks a line of the snapshot by its role and name, never by a fixed reference.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Right,
    EffectEarly,
    OptionThenRight,
    SignInClick,
    OptionLine,
    ExtractThenClick,
    OptionTwiceThenRight,
    ExtractToCap,
    EffectTwice,
}

impl Mode {
    fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "right" => Mode::Right,
            "effect_early" => Mode::EffectEarly,
            "option_then_right" => Mode::OptionThenRight,
            "sign_in_click" => Mode::SignInClick,
            "option_line" => Mode::OptionLine,
            "extract_then_click" => Mode::ExtractThenClick,
            "option_twice_then_right" => Mode::OptionTwiceThenRight,
            "extract_to_cap" => Mode::ExtractToCap,
            "effect_twice" => Mode::EffectTwice,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    reference: String,
    role: String,
    name: String,
}

fn parse_line(text: &str) -> Option<Line> {
    let (reference, rest) = text.split_once(' ')?;
    let (role, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(Line {
        reference: reference.to_string(),
        role: role.to_string(),
        name: rest[..end].replace("''", "\""),
    })
}

const EFFECT_BUTTONS: [&str; 2] = ["Pay now", "Place order"];
const TO_PAYMENT: &str = "Pay bill";
const SELECT_NAME: &str = "Payment method";
const DONE_HEADINGS: [&str; 2] = ["Payment received", "Order placed"];
const RENAMED: [(&str, &str); 3] = [
    ("Pay bill", "Make a payment"),
    ("Pay now", "Submit payment"),
    ("Place order", "Complete purchase"),
];

struct Ask<'a> {
    prompt: &'a str,
    lines: Vec<Line>,
}

impl Ask<'_> {
    fn find(&self, role: &str, name: &str) -> Option<&Line> {
        self.lines.iter().find(|l| l.role == role && l.name == name)
    }

    fn first(&self, role: &str) -> Option<&Line> {
        self.lines.iter().find(|l| l.role == role)
    }

    fn after_text(&self, label: &str) -> Option<&Line> {
        let i = self
            .lines
            .iter()
            .position(|l| l.role == "text" && l.name == label)?;
        self.lines[i + 1..].iter().find(|l| l.role == "text")
    }

    fn kept(&self) -> Vec<&str> {
        self.prompt
            .lines()
            .find_map(|l| l.strip_prefix("Kept: "))
            .map(|k| {
                if k == "none" {
                    Vec::new()
                } else {
                    k.split(", ").collect()
                }
            })
            .unwrap_or_default()
    }

    fn history(&self) -> Vec<&str> {
        self.prompt
            .lines()
            .skip_while(|l| !l.starts_with("Done so far:"))
            .skip(1)
            .filter(|l| {
                l.split_once(". ")
                    .is_some_and(|(n, _)| n.bytes().all(|b| b.is_ascii_digit()))
            })
            .collect()
    }

    fn selected(&self) -> bool {
        self.history().iter().any(|h| {
            h.split_once(". ")
                .is_some_and(|(_, a)| a.starts_with("select ") && !a.contains("(refused"))
        })
    }

    fn refused_selects(&self) -> usize {
        self.history()
            .iter()
            .filter(|h| {
                h.split_once(". ")
                    .is_some_and(|(_, a)| a.starts_with("select ") && a.contains("(refused"))
            })
            .count()
    }

    fn clicked(&self) -> bool {
        self.history().iter().any(|h| {
            h.split_once(". ")
                .is_some_and(|(_, a)| a.starts_with("click "))
        })
    }

    fn handed_back(&self) -> bool {
        self.history()
            .iter()
            .any(|h| h.split_once(". ").is_some_and(|(_, a)| a == "handback"))
    }
}

fn action(kind: &str, reference: Option<&str>, slot: Option<&str>, name: Option<&str>) -> String {
    let q = |v: Option<&str>| {
        v.map_or("null".to_string(), |s| {
            serde_json::to_string(s).unwrap_or_default()
        })
    };
    format!(
        "{{\"action\":\"{kind}\",\"ref\":{},\"slot\":{},\"key\":null,\"name\":{}}}",
        q(reference),
        q(slot),
        q(name)
    )
}

fn click(l: &Line) -> String {
    action("click", Some(&l.reference), None, None)
}

fn done() -> String {
    action("done", None, None, None)
}

fn repair(ask: &Ask) -> Option<String> {
    let said = ask.prompt.split("found no control on this page: ").nth(1)?;
    let said = said.split(". Give the action").next()?;
    let (verb, rest) = said.split_once(' ')?;
    let (role, name) = rest.split_once(' ')?;
    let name = name.trim_matches('"');
    if verb == "extract" {
        let (line, kept) = match role {
            "listitem" => (ask.first("listitem")?, "items"),
            _ if ask.find("text", "Total").is_some() => (ask.after_text("Total")?, "total"),
            _ => (ask.after_text("Amount due")?, "amount_due"),
        };
        return Some(action("extract", Some(&line.reference), None, Some(kept)));
    }
    let wanted = RENAMED
        .iter()
        .find(|(from, _)| *from == name)
        .map_or(name, |(_, to)| *to);
    let line = ask.find(role, wanted)?;
    Some(match verb {
        "click" => click(line),
        "select" => action("select", Some(&line.reference), Some("instrument"), None),
        _ => return None,
    })
}

fn next(mode: Mode, ask: &Ask) -> String {
    if ask.prompt.contains("found no control on this page: ") {
        return repair(ask).unwrap_or_else(done);
    }
    if DONE_HEADINGS
        .iter()
        .any(|h| ask.find("heading", h).is_some())
    {
        return done();
    }
    let sign_in =
        ask.lines.iter().any(|l| l.role == "textbox") && ask.find("button", "Sign in").is_some();
    if sign_in {
        if mode == Mode::SignInClick && !ask.handed_back() {
            if let Some(b) = ask.find("button", "Sign in") {
                return click(b);
            }
        }
        if mode == Mode::ExtractThenClick && !ask.handed_back() {
            if !ask.kept().contains(&"amount_due") {
                if let Some(t) = ask.first("text") {
                    return action("extract", Some(&t.reference), None, Some("amount_due"));
                }
            }
            if !ask.clicked() {
                if let Some(b) = ask.find("button", "Sign in") {
                    return click(b);
                }
            }
        }
        return action("handback", None, None, None);
    }
    let effect = EFFECT_BUTTONS.iter().find_map(|b| ask.find("button", b));
    if mode == Mode::EffectEarly {
        if let Some(b) = effect {
            return click(b);
        }
        if let Some(l) = ask.find("link", TO_PAYMENT) {
            return click(l);
        }
    }
    if mode == Mode::ExtractToCap {
        if let Some(l) = ask.after_text("Amount due") {
            return action("extract", Some(&l.reference), None, Some("amount_due"));
        }
    }
    let kept = ask.kept();
    let order = ask.find("text", "Total").is_some();
    let needed: &[&str] = if order {
        &["items", "total"]
    } else {
        &["amount_due"]
    };
    for name in needed {
        if kept.contains(name) {
            continue;
        }
        let line = match *name {
            "items" => ask.first("listitem"),
            "total" => ask.after_text("Total"),
            _ => ask.after_text("Amount due"),
        };
        if let Some(l) = line {
            return action("extract", Some(&l.reference), None, Some(name));
        }
    }
    if let Some(combo) = ask.find("combobox", SELECT_NAME) {
        if !ask.selected() {
            let on_option = match mode {
                Mode::OptionLine => true,
                Mode::OptionThenRight => ask.refused_selects() == 0,
                Mode::OptionTwiceThenRight => ask.refused_selects() < 2,
                _ => false,
            };
            let target = if on_option {
                ask.first("option").unwrap_or(combo)
            } else {
                combo
            };
            return action("select", Some(&target.reference), Some("instrument"), None);
        }
        if let Some(b) = effect {
            return click(b);
        }
    }
    if let Some(l) = ask.find("link", TO_PAYMENT) {
        return click(l);
    }
    done()
}

fn answer(mode: Mode, request: &Value) -> String {
    let prompt = request["prompt"].as_str().unwrap_or("");
    let snapshot = request["snapshot"].as_str().unwrap_or("");
    let ask = Ask {
        prompt,
        lines: snapshot.lines().filter_map(parse_line).collect(),
    };
    let text = if request["grammar"] == "action" {
        next(mode, &ask)
    } else {
        String::new()
    };
    format!(
        "{{\"id\":{},\"text\":{},\"tokens_in\":0,\"tokens_out\":0,\"first_token_ms\":0,\"ms\":0}}",
        request["id"].as_u64().unwrap_or(0),
        serde_json::to_string(&text).unwrap_or_default()
    )
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let weights = argv
        .windows(2)
        .find(|w| w[0] == "--weights")
        .map(|w| w[1].clone());
    let mode = weights
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| t.lines().next().and_then(Mode::parse));
    let Some(mode) = mode else {
        eprintln!(
            "REFUSED usage: --weights must name a file whose one line is a mode of the stand-in"
        );
        return ExitCode::from(2);
    };
    let mut out = std::io::stdout().lock();
    if writeln!(out, "ready standin")
        .and_then(|()| out.flush())
        .is_err()
    {
        return ExitCode::from(1);
    }
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            return ExitCode::from(1);
        };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            return ExitCode::from(1);
        };
        if writeln!(out, "{}", answer(mode, &request))
            .and_then(|()| out.flush())
            .is_err()
        {
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(prompt: &str, snapshot: &str) -> String {
        let a = Ask {
            prompt,
            lines: snapshot.lines().filter_map(parse_line).collect(),
        };
        next(Mode::Right, &a)
    }

    #[test]
    fn a_snapshot_line_gives_its_reference_role_and_name() {
        assert_eq!(
            parse_line("e7 combobox \"Payment method\" value=\"x\" frame=http://pay.agent.test:1"),
            Some(Line {
                reference: "e7".into(),
                role: "combobox".into(),
                name: "Payment method".into()
            })
        );
        assert_eq!(
            parse_line("e4 listitem \"\"").map(|l| l.name),
            Some(String::new())
        );
    }

    #[test]
    fn the_right_walk_hands_back_extracts_selects_and_pays() {
        let signin = "e1 heading \"Sign in\"\ne3 textbox \"Email\"\ne5 textbox \"Password\" credential\ne6 button \"Sign in\"";
        assert!(
            ask("Kept: none\nDone so far:\n1. navigate start\n", signin).contains("\"handback\"")
        );
        let bill = "e1 heading \"Your bill\"\ne4 text \"Amount due\"\ne5 text \"1.00\"\ne6 link \"Pay bill\"";
        assert_eq!(
            ask("Kept: none\nDone so far:\n1. navigate start\n2. handback\n", bill),
            "{\"action\":\"extract\",\"ref\":\"e5\",\"slot\":null,\"key\":null,\"name\":\"amount_due\"}"
        );
        assert!(ask("Kept: amount_due\nDone so far:\n", bill).contains("\"ref\":\"e6\""));
        let pay = "e9 combobox \"Payment method\"\ne10 option \"a\"\ne12 button \"Pay now\"";
        assert!(
            ask("Kept: amount_due\nDone so far:\n1. navigate start\n", pay)
                .contains("\"select\",\"ref\":\"e9\"")
        );
        assert!(ask(
            "Kept: amount_due\nDone so far:\n4. select e9 slot instrument\n",
            pay
        )
        .contains("\"click\",\"ref\":\"e12\""));
        assert!(ask(
            "Kept: amount_due\nDone so far:\n",
            "e3 heading \"Payment received\""
        )
        .contains("\"done\""));
    }

    #[test]
    fn the_repair_of_an_extract_names_the_line_under_the_step_name() {
        let order = "e4 listitem \"\"\ne5 listitem \"\"\ne7 text \"Total\"\ne8 text \"2.00\"";
        assert_eq!(
            ask("Step 3 of the saved steps found no control on this page: extract listitem \"\". Give the action that does this step on this page.", order),
            "{\"action\":\"extract\",\"ref\":\"e4\",\"slot\":null,\"key\":null,\"name\":\"items\"}"
        );
        assert!(ask("Step 4 of the saved steps found no control on this page: extract text \"2.00\". Give the action that does this step on this page.", order)
            .contains("\"ref\":\"e8\",\"slot\":null,\"key\":null,\"name\":\"total\""));
        let bill = "e4 text \"Amount due\"\ne5 text \"1.00\"";
        assert!(ask("Step 3 of the saved steps found no control on this page: extract text \"1.00\". Give the action that does this step on this page.", bill)
            .contains("\"name\":\"amount_due\""));
    }

    #[test]
    fn the_modes_of_ruling_cp_179_give_their_replies() {
        let of = |mode: Mode, prompt: &str, snapshot: &str| {
            next(
                mode,
                &Ask {
                    prompt,
                    lines: snapshot.lines().filter_map(parse_line).collect(),
                },
            )
        };
        let pay = "e9 combobox \"Payment method\"\ne10 option \"a\"\ne12 button \"Pay now\"";
        let once = "Kept: amount_due\nDone so far:\n1. select e10 slot instrument (refused: x)\n";
        let twice = "Kept: amount_due\nDone so far:\n1. select e10 slot instrument (refused: x)\n2. select e10 slot instrument (refused: x)\n";
        assert!(of(Mode::OptionTwiceThenRight, once, pay).contains("\"select\",\"ref\":\"e10\""));
        assert!(of(Mode::OptionTwiceThenRight, twice, pay).contains("\"select\",\"ref\":\"e9\""));
        assert!(of(Mode::OptionThenRight, once, pay).contains("\"select\",\"ref\":\"e9\""));
        let bill = "e4 text \"Amount due\"\ne5 text \"1.00\"\ne6 link \"Pay bill\"";
        assert!(
            of(Mode::ExtractToCap, "Kept: amount_due\nDone so far:\n", bill)
                .contains("\"extract\",\"ref\":\"e5\"")
        );
        let chose = "Kept: amount_due\nDone so far:\n4. select e9 slot instrument (done)\n5. click e12 (done; a request was sent)\n";
        assert!(of(Mode::EffectTwice, chose, pay).contains("\"click\",\"ref\":\"e12\""));
    }

    #[test]
    fn the_repair_names_the_changed_control() {
        let page = "e2 link \"Make a payment\"\ne3 button \"Submit payment\"";
        assert!(ask("Step 4 of the saved steps found no control on this page: click link \"Pay bill\". Give the action that does this step on this page.", page)
            .contains("\"ref\":\"e2\""));
        assert!(ask("Step 6 of the saved steps found no control on this page: click button \"Pay now\". Give the action that does this step on this page.", page)
            .contains("\"ref\":\"e3\""));
    }
}
