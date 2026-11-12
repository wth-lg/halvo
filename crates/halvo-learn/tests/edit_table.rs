use halvo_learn::{edit_step, EditAnswer, EditMove, EditState, K_CLEAN};

const TABLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../formal/r5-admit/edit-table.tsv"
);

fn state(token: &str, line: &str) -> EditState {
    EditState::from_token(token).unwrap_or_else(|| panic!("row {line:?}: no state {token:?}"))
}

fn cleans_of(guard: &str, line: &str) -> Vec<u32> {
    match guard {
        "-" => (0..=K_CLEAN + 1).collect(),
        "clean_below" => (0..K_CLEAN - 1).collect(),
        "clean_reaches" => vec![K_CLEAN - 1],
        other => panic!("row {line:?}: no guard {other:?}"),
    }
}

fn wanted(answer: &str, clean: &str, passed: u32, line: &str) -> EditAnswer {
    match (answer, clean) {
        ("edit", "-") => EditAnswer::Edit,
        ("edit", _) | (_, "-") => {
            panic!("row {line:?}: an answer `edit` goes with the clean `-` alone")
        }
        (next, "same") => EditAnswer::Next {
            state: state(next, line),
            clean: passed,
        },
        (next, count) => EditAnswer::Next {
            state: state(next, line),
            clean: count
                .parse()
                .unwrap_or_else(|_| panic!("row {line:?}: no count {count:?}")),
        },
    }
}

#[test]
fn edit_table() {
    let text = std::fs::read_to_string(TABLE).expect("formal/r5-admit/edit-table.tsv is readable");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 57, "the table has 57 rows");
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted, "the rows stand in byte order");

    let mut keys = Vec::new();
    for line in &lines {
        let fields: Vec<&str> = line.split('\t').collect();
        let [from, edit_move, guard, answer, clean] = fields[..] else {
            panic!("row {line:?} does not have five fields");
        };
        let from = state(from, line);
        let edit_move = EditMove::from_token(edit_move)
            .unwrap_or_else(|| panic!("row {line:?}: no move {edit_move:?}"));
        let key = (from, edit_move, guard);
        assert!(!keys.contains(&key), "row {line:?} stands twice");
        keys.push(key);
        for passed in cleans_of(guard, line) {
            assert_eq!(
                edit_step(from, edit_move, passed),
                wanted(answer, clean, passed, line),
                "row {line:?} at clean {passed}"
            );
        }
    }

    for &from in EditState::ALL {
        for &edit_move in EditMove::ALL {
            assert!(
                keys.iter().any(|&(s, m, _)| s == from && m == edit_move),
                "no row for {} and {}",
                from.token(),
                edit_move.token()
            );
        }
    }

    assert_eq!(
        edit_step(EditState::Provisional, EditMove::UseOk, K_CLEAN),
        EditAnswer::Edit,
        "provisional with use_ok at clean K answers edit"
    );
}
