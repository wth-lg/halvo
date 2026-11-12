use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use halvo_policy::{decide_row, Checkpoint, Decision};
use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::{ActionClass, AllowedClass, Autonomy, Caps};

const ROWS: usize = 3 * 10 * 8 * 8 * 2;

fn table() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/r3-confirm/policy-table.tsv");
    match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => panic!("cannot read {}: {e}", path.display()),
    }
}

fn yes_no(letter: char) -> YesNo {
    match letter {
        'y' => YesNo::Yes,
        'n' => YesNo::No,
        other => panic!("caps letter {other:?}"),
    }
}

struct Row {
    autonomy: Autonomy,
    class: ActionClass,
    allow: Vec<AllowedClass>,
    caps: Caps,
    checkpoint: Checkpoint,
    decision: String,
}

fn row(line: &str) -> Row {
    let cols: Vec<&str> = line.split('\t').collect();
    assert_eq!(cols.len(), 6, "a line of the table has six columns: {line}");
    let autonomy = Autonomy::parse(cols[0]).unwrap_or_else(|r| panic!("{r}: {line}"));
    let class = ActionClass::parse(cols[1]).unwrap_or_else(|r| panic!("{r}: {line}"));
    let allow = if cols[2] == "-" {
        Vec::new()
    } else {
        cols[2]
            .split(',')
            .map(|a| AllowedClass::parse(a).unwrap_or_else(|r| panic!("{r}: {line}")))
            .collect()
    };
    let letters: Vec<char> = cols[3].chars().collect();
    assert_eq!(letters.len(), 3, "caps are three letters: {line}");
    let caps = Caps {
        untrusted: yes_no(letters[0]),
        private: yes_no(letters[1]),
        external: yes_no(letters[2]),
    };
    let checkpoint = match cols[4] {
        "yes" => Checkpoint::Yes,
        "no" => Checkpoint::No,
        other => panic!("checkpoint {other:?}: {line}"),
    };
    Row {
        autonomy,
        class,
        allow,
        caps,
        checkpoint,
        decision: cols[5].to_owned(),
    }
}

#[test]
fn policy_table() {
    let text = table();
    assert!(text.ends_with('\n') && !text.contains('\r'));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), ROWS, "the table holds every row of the domain");
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, lines, "the lines are sorted in byte order");

    let mut seen = HashSet::new();
    let mut differ = Vec::new();
    for line in &lines {
        let r = row(line);
        let mut allow: Vec<&str> = r.allow.iter().map(|a| a.as_str()).collect();
        allow.sort_unstable();
        let point = (
            r.autonomy,
            r.class,
            allow.join(","),
            r.caps.untrusted,
            r.caps.private,
            r.caps.external,
            r.checkpoint,
        );
        assert!(seen.insert(point), "a row stands twice: {line}");
        let got = decide_row(r.autonomy, r.class, &r.allow, &r.caps, r.checkpoint);
        if got.as_str() != r.decision {
            differ.push(format!("{line}\tdecide_row={}", got.as_str()));
        }
    }
    assert_eq!(seen.len(), ROWS);
    assert!(
        differ.is_empty(),
        "decide_row differs from {} rows of the table, first: {:?}",
        differ.len(),
        &differ[..differ.len().min(10)]
    );
}

#[test]
fn the_table_holds_each_of_the_four_decisions() {
    let text = table();
    for decision in Decision::ALL {
        let suffix = format!("\t{}", decision.as_str());
        assert!(
            text.lines().any(|line| line.ends_with(&suffix)),
            "no row gives {}",
            decision.as_str()
        );
    }
}
