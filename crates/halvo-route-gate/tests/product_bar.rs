use std::path::Path;

use halvo_route::line::{LINE_DEV_MIN, LINE_HELD_MIN};

const TITLE: &str = "# The published bar";

const SENTENCES: [&str; 7] = [
    "Sign-in, a second factor and the confirmation of a payment go to the human at every tier, so no row measures them.",
    "Five seed categories have no runtime, and of the others only the compile and recover cells of bill and order and the write cell of reply have rows, because the other tasks call no model.",
    "The fixtures are the team's own and easier than live sites, which also put bot checks before an agent that no fixture has.",
    "A cell rests on 3 seeds and 2 machines, neither of them a 16 GB laptop.",
    "One on-device model decides every cell, the agent-setup run's, and rows of other models decide nothing.",
    "An attempt passes by the fixture's own state, while in the product a learned playbook checks itself by conditions that the model compiled, which are weaker.",
    "No hosted tier is recorded, so the owner's hypothesis about open weights against closed models is untested on every landlord task.",
];

fn bar() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/r6/bar.md");
    std::fs::read_to_string(path).expect("docs/r6/bar.md is UTF-8 text")
}

fn values<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let key = format!("{name}=");
    let mut found = Vec::new();
    for (at, _) in text.match_indices(&key) {
        let before = text[..at].bytes().next_back();
        if before.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
            continue;
        }
        let rest = &text[at + key.len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        found.push(&rest[..end]);
    }
    found
}

#[test]
fn bar_words() {
    let text = bar();
    assert!(!text.starts_with('\u{feff}'), "a byte-order mark");
    assert!(!text.contains('\r'), "a carriage return");
    assert!(
        text.ends_with('\n') && !text.ends_with("\n\n"),
        "one LF at the end"
    );
    let opening = format!("{TITLE}\n\n{}\n\n", SENTENCES.join(" "));
    assert!(
        text.starts_with(&opening),
        "docs/r6/bar.md opens with its title and the seven sentences of T4.4 as one paragraph"
    );
    for (name, value) in [
        ("LINE_DEV_MIN", LINE_DEV_MIN),
        ("LINE_HELD_MIN", LINE_HELD_MIN),
    ] {
        let want = value.to_string();
        let found = values(&text, name);
        assert!(!found.is_empty(), "the word {name}={want}");
        assert!(
            found.iter().all(|got| *got == want),
            "every {name}= reads {want}: {found:?}"
        );
    }
}
