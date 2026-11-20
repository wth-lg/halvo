use halvo_learn::{edit_step, EditAnswer, EditMove, EditState, K_CLEAN};
use serde_json::{json, Map, Value};

const TRACES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../formal/r5-admit/traces.jsonl"
);
const LEAST_TRACES: usize = 200;
const MOST_MOVES: usize = 16;
const HEADER_KEYS: [&str; 2] = ["count", "seed"];
const TRACE_KEYS: [&str; 1] = ["moves"];
const MOVE_KEYS: [&str; 4] = ["answer", "clean", "guard", "move"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Guard {
    None,
    CleanBelow,
    CleanReaches,
}

impl Guard {
    const ALL: [Guard; 3] = [Guard::None, Guard::CleanBelow, Guard::CleanReaches];

    fn token(self) -> &'static str {
        match self {
            Guard::None => "-",
            Guard::CleanBelow => "clean_below",
            Guard::CleanReaches => "clean_reaches",
        }
    }

    fn from_token(token: &str) -> Option<Guard> {
        Guard::ALL.into_iter().find(|guard| guard.token() == token)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Move {
    edit_move: EditMove,
    guard: Guard,
    answer: EditAnswer,
}

#[derive(Clone, Debug)]
struct Trace {
    line: usize,
    moves: Vec<Move>,
}

fn object<'a>(value: &'a Value, keys: &[&str], at: &str) -> Result<&'a Map<String, Value>, String> {
    let map = value
        .as_object()
        .ok_or_else(|| format!("{at}: not an object"))?;
    let mut found: Vec<&str> = map.keys().map(String::as_str).collect();
    found.sort_unstable();
    if found != keys {
        return Err(format!(
            "{at}: the keys are {found:?}, the form has {keys:?}"
        ));
    }
    Ok(map)
}

fn whole(value: &Value, at: &str) -> Result<u64, String> {
    value
        .as_u64()
        .ok_or_else(|| format!("{at}: {value} is not a whole number"))
}

fn text<'a>(value: &'a Value, at: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{at}: {value} is not a text"))
}

fn answer_of(answer: &str, clean: &Value, at: &str) -> Result<EditAnswer, String> {
    match (answer, clean) {
        ("edit", Value::Null) => Ok(EditAnswer::Edit),
        ("edit", _) => Err(format!(
            "{at}: the answer edit has the clean {clean}, not null"
        )),
        (_, Value::Null) => Err(format!("{at}: the answer {answer} has the clean null")),
        (state, count) => Ok(EditAnswer::Next {
            state: EditState::from_token(state)
                .ok_or_else(|| format!("{at}: no state {state:?}"))?,
            clean: u32::try_from(whole(count, at)?)
                .map_err(|_| format!("{at}: the clean {count} is above u32"))?,
        }),
    }
}

fn move_of(value: &Value, at: &str) -> Result<Move, String> {
    let map = object(value, &MOVE_KEYS, at)?;
    let edit_move = text(&map["move"], at)?;
    let guard = text(&map["guard"], at)?;
    Ok(Move {
        edit_move: EditMove::from_token(edit_move)
            .ok_or_else(|| format!("{at}: no move {edit_move:?}"))?,
        guard: Guard::from_token(guard).ok_or_else(|| format!("{at}: no guard {guard:?}"))?,
        answer: answer_of(text(&map["answer"], at)?, &map["clean"], at)?,
    })
}

fn read(file: &str) -> Result<Vec<Trace>, String> {
    let mut lines = file.lines();
    let first = lines.next().ok_or("the file is empty")?;
    let header: Value = serde_json::from_str(first).map_err(|fault| format!("line 1: {fault}"))?;
    let header = object(&header, &HEADER_KEYS, "line 1")?;
    whole(&header["seed"], "line 1")?;
    let count = whole(&header["count"], "line 1")?;
    let mut traces = Vec::new();
    for (index, line_text) in lines.enumerate() {
        let line = index + 2;
        let at = format!("line {line}");
        let value: Value =
            serde_json::from_str(line_text).map_err(|fault| format!("{at}: {fault}"))?;
        let map = object(&value, &TRACE_KEYS, &at)?;
        let moves = map["moves"]
            .as_array()
            .ok_or_else(|| format!("{at}: moves is not a list"))?;
        if moves.is_empty() || moves.len() > MOST_MOVES {
            return Err(format!(
                "{at}: {} moves, not 1 to {MOST_MOVES}",
                moves.len()
            ));
        }
        let moves = moves
            .iter()
            .enumerate()
            .map(|(index, value)| move_of(value, &format!("{at}, move {}", index + 1)))
            .collect::<Result<Vec<Move>, String>>()?;
        traces.push(Trace { line, moves });
    }
    if u64::try_from(traces.len()) != Ok(count) {
        return Err(format!(
            "line 1: the count is {count}, the file holds {} traces",
            traces.len()
        ));
    }
    if traces.len() < LEAST_TRACES {
        return Err(format!(
            "the file holds {} traces, fewer than {LEAST_TRACES}",
            traces.len()
        ));
    }
    let moves: usize = traces.iter().map(|trace| trace.moves.len()).sum();
    let written = [
        ("seed", 1),
        ("count", 1),
        ("moves", traces.len()),
        ("move", moves),
        ("guard", moves),
        ("answer", moves),
        ("clean", moves),
    ];
    for (key, read) in written {
        let found = file.matches(&format!("\"{key}\":")).count();
        if found != read {
            return Err(format!(
                "the key {key} is written {found} times and read {read} times"
            ));
        }
    }
    Ok(traces)
}

fn guard_of(state: EditState, edit_move: EditMove, clean: u32) -> Guard {
    match (state, edit_move) {
        (EditState::Provisional, EditMove::UseOk) if clean < K_CLEAN - 1 => Guard::CleanBelow,
        (EditState::Provisional, EditMove::UseOk) if clean == K_CLEAN - 1 => Guard::CleanReaches,
        _ => Guard::None,
    }
}

fn shown(answer: EditAnswer) -> String {
    match answer {
        EditAnswer::Edit => "edit".to_string(),
        EditAnswer::Next { state, clean } => format!("{} with clean {clean}", state.token()),
    }
}

fn replay(
    trace: &Trace,
    step: impl Fn(EditState, EditMove, u32) -> EditAnswer,
) -> Result<usize, String> {
    let mut state = EditState::None;
    let mut clean = 0;
    let mut replayed = 0;
    for (index, done) in trace.moves.iter().enumerate() {
        let at = format!("line {}, move {}", trace.line, index + 1);
        let guard = guard_of(state, done.edit_move, clean);
        if guard != done.guard {
            return Err(format!(
                "{at}: {} from {} at clean {clean} takes the guard {}, the trace has {}",
                done.edit_move.token(),
                state.token(),
                guard.token(),
                done.guard.token()
            ));
        }
        let answer = step(state, done.edit_move, clean);
        if answer != done.answer {
            return Err(format!(
                "{at}: {} from {} at clean {clean} answers {}, the model {}",
                done.edit_move.token(),
                state.token(),
                shown(answer),
                shown(done.answer)
            ));
        }
        if let EditAnswer::Next {
            state: next,
            clean: next_clean,
        } = answer
        {
            state = next;
            clean = next_clean;
        }
        replayed += 1;
    }
    Ok(replayed)
}

fn committed() -> (String, Vec<Trace>) {
    let file = std::fs::read_to_string(TRACES).expect("formal/r5-admit/traces.jsonl is readable");
    let traces = read(&file).unwrap_or_else(|fault| panic!("traces.jsonl: {fault}"));
    (file, traces)
}

fn replay_all(
    traces: &[Trace],
    step: impl Fn(EditState, EditMove, u32) -> EditAnswer + Copy,
) -> Result<usize, String> {
    traces.iter().map(|trace| replay(trace, step)).sum()
}

#[test]
fn formal_traces() {
    let (file, traces) = committed();
    let mut replayed = 0;
    for trace in &traces {
        replayed +=
            replay(trace, edit_step).unwrap_or_else(|fault| panic!("traces.jsonl: {fault}"));
    }
    let written = file.matches("\"move\":").count();
    assert_eq!(
        replayed, written,
        "the replay made {replayed} moves of the {written} that the file writes"
    );
    println!("formal_traces traces={} moves={replayed}", traces.len());
}

fn other_answers(answer: EditAnswer) -> Vec<EditAnswer> {
    let mut all = vec![EditAnswer::Edit];
    for &state in EditState::ALL {
        for clean in 0..=K_CLEAN + 1 {
            all.push(EditAnswer::Next { state, clean });
        }
    }
    all.retain(|other| *other != answer);
    all
}

#[test]
fn formal_traces_refuses_every_changed_answer_and_guard_at_its_own_move() {
    let (_, traces) = committed();
    let mut changed = 0;
    for trace in &traces {
        for index in 0..trace.moves.len() {
            let done = trace.moves[index];
            let others = other_answers(done.answer)
                .into_iter()
                .map(|answer| Move { answer, ..done })
                .chain(
                    Guard::ALL
                        .into_iter()
                        .filter(|guard| *guard != done.guard)
                        .map(|guard| Move { guard, ..done }),
                );
            for other in others {
                let mut copy = trace.clone();
                copy.moves[index] = other;
                let at = format!("line {}, move {}: ", trace.line, index + 1);
                match replay(&copy, edit_step) {
                    Err(fault) if fault.starts_with(&at) => changed += 1,
                    Err(fault) => panic!("{other:?} at {at}is refused elsewhere: {fault}"),
                    Ok(_) => panic!("{other:?} at {at}is not refused"),
                }
            }
        }
    }
    let moves: usize = traces.iter().map(|trace| trace.moves.len()).sum();
    assert_eq!(
        changed,
        moves * 30,
        "each move is changed to the 28 other answers of 7 states at a clean of 0 to 3 or edit, and to the 2 other guards"
    );

    let mut answer = traces[0].clone();
    answer.moves[0].answer = EditAnswer::Edit;
    assert_eq!(
        replay(&answer, edit_step),
        Err("line 2, move 1: propose_ok from none at clean 0 answers candidate with clean 0, the model edit".to_string())
    );
    let mut guard = traces[0].clone();
    guard.moves[0].guard = Guard::CleanBelow;
    assert_eq!(
        replay(&guard, edit_step),
        Err("line 2, move 1: propose_ok from none at clean 0 takes the guard -, the trace has clean_below".to_string())
    );
}

#[test]
fn formal_traces_fails_each_wrong_form_of_edit_step_that_the_readme_names() {
    use EditMove as M;
    use EditState as S;
    let (_, traces) = committed();
    let wrong_clean = |from: S, moves: &'static [M], clean_of: fn(u32) -> u32| {
        move |state: S, edit_move: M, clean: u32| match edit_step(state, edit_move, clean) {
            EditAnswer::Next { state: next, clean }
                if state == from && moves.contains(&edit_move) =>
            {
                EditAnswer::Next {
                    state: next,
                    clean: clean_of(clean),
                }
            }
            answer => answer,
        }
    };
    let forms: [(&str, &str, Result<usize, String>); 6] = [
        (
            "a clean that rises on a clean use of an active edit",
            "use_ok from active",
            replay_all(
                &traces,
                wrong_clean(S::Active, &[M::UseOk], |clean| clean + 1),
            ),
        ),
        (
            "a clean that falls to 0 on a counted failure of an active edit",
            "use_fail_counted from active",
            replay_all(&traces, wrong_clean(S::Active, &[M::UseFailCounted], |_| 0)),
        ),
        (
            "a clean that falls to 0 on a failure of an active edit that does not count",
            "use_fail_other from active",
            replay_all(&traces, wrong_clean(S::Active, &[M::UseFailOther], |_| 0)),
        ),
        (
            "a clean that rises on a revert",
            "use_fail_counted from provisional",
            replay_all(
                &traces,
                wrong_clean(S::Provisional, &[M::UseFailCounted], |clean| clean + 1),
            ),
        ),
        (
            "a clean that falls on a withdrawal of a provisional edit",
            "recompile from provisional",
            replay_all(
                &traces,
                wrong_clean(S::Provisional, &[M::Recompile], |clean| {
                    clean.saturating_sub(1)
                }),
            ),
        ),
        (
            "a clean that falls on a withdrawal of an active edit",
            "recompile from active",
            replay_all(
                &traces,
                wrong_clean(S::Active, &[M::Recompile], |clean| clean.saturating_sub(1)),
            ),
        ),
    ];
    for (form, place, result) in forms {
        match result {
            Err(fault) => assert!(
                fault.contains(&format!(": {place} at clean ")),
                "{form} fails at another move: {fault}"
            ),
            Ok(_) => panic!("the traces do not fail {form}"),
        }
    }
}

fn with_line(file: &str, line: usize, edit: impl Fn(&mut Value)) -> String {
    let mut lines: Vec<String> = file.lines().map(str::to_string).collect();
    let mut value: Value = serde_json::from_str(&lines[line - 1]).expect("a line of JSON");
    edit(&mut value);
    lines[line - 1] = value.to_string();
    lines.join("\n") + "\n"
}

fn with_move(file: &str, line: usize, index: usize, key: &str, value: Value) -> String {
    with_line(file, line, |trace| {
        trace["moves"][index][key] = value.clone()
    })
}

fn without_last_trace(file: &str, count: u64) -> String {
    let lines: Vec<&str> = file.lines().collect();
    let kept = lines[1..lines.len() - 1].join("\n");
    format!("{}\n{kept}\n", json!({"seed": 1, "count": count}))
}

#[test]
fn formal_traces_reader_refuses_each_broken_form_and_reads_its_bounds() {
    let (file, traces) = committed();
    let first = &traces[0];
    assert_eq!(first.line, 2);
    assert_eq!(
        first.moves[0].answer,
        EditAnswer::Next {
            state: EditState::Candidate,
            clean: 0
        }
    );
    assert_eq!(first.moves[2].answer, EditAnswer::Edit);
    let header = |value: Value| with_line(&file, 1, move |line| *line = value.clone());
    let longer = |moves: usize| {
        with_line(&file, 2, move |trace| {
            let one = trace["moves"][0].clone();
            trace["moves"] = Value::Array(vec![one; moves]);
        })
    };
    let broken: Vec<(&str, String)> = vec![
        ("an empty file", String::new()),
        ("a header that is no JSON", file.replacen('{', "", 1)),
        ("a header that is a text", header(json!("seed 1 count 200"))),
        (
            "a header with a third key",
            header(json!({"seed": 1, "count": 200, "moves": []})),
        ),
        ("a header without its seed", header(json!({"count": 200}))),
        (
            "a seed that is a text",
            header(json!({"seed": "1", "count": 200})),
        ),
        ("a seed below 0", header(json!({"seed": -1, "count": 200}))),
        (
            "a count above the traces",
            header(json!({"seed": 1, "count": 201})),
        ),
        (
            "a count below the traces",
            header(json!({"seed": 1, "count": 199})),
        ),
        (
            "a count that is a text",
            header(json!({"seed": 1, "count": "200"})),
        ),
        (
            "199 traces with the count 199",
            without_last_trace(&file, 199),
        ),
        (
            "a blank line among the traces",
            file.replacen('\n', "\n\n", 1),
        ),
        (
            "a trace with a second key",
            with_line(&file, 2, |trace| trace["x"] = json!(1)),
        ),
        (
            "a trace whose moves are no list",
            with_line(&file, 2, |trace| trace["moves"] = json!({})),
        ),
        (
            "a trace that is a list",
            with_line(&file, 2, |trace| *trace = json!([])),
        ),
        ("a trace of 17 moves", longer(17)),
        ("a trace of no moves", longer(0)),
        (
            "a seed that is not whole",
            header(json!({"seed": 1.5, "count": 200})),
        ),
        (
            "a count with a point",
            header(json!({"seed": 1, "count": 200.0})),
        ),
        (
            "a trace whose moves stand twice",
            file.replacen("{\"moves\":[", "{\"moves\":[],\"moves\":[", 1),
        ),
        (
            "a move whose move stands twice",
            file.replacen(
                "{\"move\":\"propose_ok\"",
                "{\"move\":\"drop\",\"move\":\"propose_ok\"",
                1,
            ),
        ),
        (
            "a move whose guard stands twice",
            file.replacen(
                "\"guard\":\"-\"",
                "\"guard\":\"clean_below\",\"guard\":\"-\"",
                1,
            ),
        ),
        (
            "an edit answer whose clean stands twice, a count first",
            file.replacen(
                "\"answer\":\"edit\",\"clean\":null",
                "\"answer\":\"edit\",\"clean\":0,\"clean\":null",
                1,
            ),
        ),
        (
            "a move with a fifth key",
            with_move(&file, 2, 0, "state", json!("none")),
        ),
        (
            "a move without its guard",
            with_line(&file, 2, |trace| {
                trace["moves"][0]
                    .as_object_mut()
                    .expect("a move")
                    .remove("guard");
            }),
        ),
        (
            "a move whose guard stands under another key",
            with_line(&file, 2, |trace| {
                let one = trace["moves"][0].as_object_mut().expect("a move");
                let guard = one.remove("guard").expect("a guard");
                one.insert("guards".to_string(), guard);
            }),
        ),
        (
            "a move that is a text",
            with_line(&file, 2, |trace| trace["moves"][0] = json!("propose_ok")),
        ),
        (
            "a move token that no move has",
            with_move(&file, 2, 0, "move", json!("propose")),
        ),
        (
            "a move token that is no text",
            with_move(&file, 2, 0, "move", json!(1)),
        ),
        (
            "a guard token that no guard has",
            with_move(&file, 2, 0, "guard", json!("clean")),
        ),
        ("an empty guard", with_move(&file, 2, 0, "guard", json!(""))),
        (
            "an answer that no state has",
            with_move(&file, 2, 0, "answer", json!("kept")),
        ),
        (
            "an answer that is no text",
            with_move(&file, 2, 0, "answer", Value::Null),
        ),
        (
            "a state answer with the clean null",
            with_move(&file, 2, 0, "clean", Value::Null),
        ),
        (
            "an edit answer with a count",
            with_move(&file, 2, 2, "clean", json!(0)),
        ),
        (
            "a clean below 0",
            with_move(&file, 2, 0, "clean", json!(-1)),
        ),
        (
            "a clean that is not whole",
            with_move(&file, 2, 0, "clean", json!(0.5)),
        ),
        (
            "a clean that is a text",
            with_move(&file, 2, 0, "clean", json!("0")),
        ),
        (
            "a clean above u32",
            with_move(&file, 2, 0, "clean", json!(u64::from(u32::MAX) + 1)),
        ),
    ];
    for (case, broken_file) in &broken {
        assert!(read(broken_file).is_err(), "the reader reads {case}");
    }

    let sixteen = read(&longer(16)).expect("a trace of 16 moves is read");
    assert_eq!(sixteen[0].moves.len(), 16);
    let one_more = format!("{file}{}\n", file.lines().nth(1).expect("a trace"));
    let one_more = with_line(&one_more, 1, |line| line["count"] = json!(201));
    assert_eq!(read(&one_more).expect("201 traces are read").len(), 201);
    let at_the_top = with_move(&file, 2, 0, "clean", json!(u32::MAX));
    assert_eq!(
        read(&at_the_top).expect("a clean of u32::MAX is read")[0].moves[0].answer,
        EditAnswer::Next {
            state: EditState::Candidate,
            clean: u32::MAX
        }
    );
    assert_eq!(
        read(&file.replace('\n', "\r\n"))
            .expect("CRLF line ends are read")
            .len(),
        traces.len()
    );
}
