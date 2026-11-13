#![forbid(unsafe_code)]

use halvo_route::spend::{step, Account, Hold, Move, SpendCause, State};
use serde_json::{json, Map, Value};

const TRACES: &str = "../../formal/r6-spend/traces.jsonl";
const MICRO_PER_MINOR: u64 = 10_000;
const LEAST_TRACES: usize = 200;
const MOST_MOVES: usize = 12;
const DIRECTED: usize = 6;
const DIRECTED_MOVES: usize = 5;
const HEADER_KEYS: [&str; 2] = ["count", "seed"];
const TRACE_KEYS: [&str; 5] = ["active", "cap", "held", "moves", "spent"];
const MOVE_KEYS: [&str; 7] = ["answer", "arg", "cost", "held", "move", "spent", "state"];
const FORM_ORDER: [&str; 12] = [
    "seed", "count", "cap", "active", "move", "arg", "answer", "state", "spent", "held", "moves",
    "cost",
];
const STATE_WORDS: [&str; 6] = ["new", "held", "sent", "settled", "expired", "voided"];
const OUTCOMES: [&str; 5] = ["answer", "state", "spent", "held", "cost"];
const OPENINGS: [&str; 4] = ["opening spent", "opening held", "cap", "active"];

#[derive(Clone, Copy)]
struct Want {
    answer: Result<(), SpendCause>,
    state: State,
    spent: u64,
    held: u64,
    cost: u64,
}

#[derive(Clone)]
struct Trace {
    line: usize,
    account: Account,
    moves: Vec<(Move, Want)>,
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

fn form_of(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by_key(|key| {
                FORM_ORDER
                    .iter()
                    .position(|form| form == key)
                    .unwrap_or(FORM_ORDER.len())
            });
            let fields: Vec<String> = keys
                .iter()
                .map(|key| format!("{}:{}", Value::from(key.as_str()), form_of(&map[*key])))
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(form_of).collect::<Vec<_>>().join(",")
        ),
        scalar => scalar.to_string(),
    }
}

fn in_form(value: &Value, text: &str, at: &str) -> Result<(), String> {
    if form_of(value) == text {
        Ok(())
    } else {
        Err(format!(
            "{at}: the line is not written in the line form of T7.6"
        ))
    }
}

fn number(value: &Value, at: &str) -> Result<u64, String> {
    value
        .as_u64()
        .ok_or_else(|| format!("{at}: {value} is no whole number"))
}

fn micro(value: &Value, at: &str) -> Result<u64, String> {
    number(value, at)?
        .checked_mul(MICRO_PER_MINOR)
        .ok_or_else(|| format!("{at}: {value} times {MICRO_PER_MINOR} does not fit a u64"))
}

fn word<T: Copy>(
    value: &Value,
    list: &[T],
    name: fn(T) -> &'static str,
    at: &str,
) -> Result<T, String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("{at}: {value} is no word"))?;
    list.iter()
        .copied()
        .find(|item| name(*item) == text)
        .ok_or_else(|| format!("{at}: {text} is outside the list"))
}

fn read_move(value: &Value, at: &str) -> Result<(Move, Want), String> {
    let map = object(value, &MOVE_KEYS, at)?;
    let arg = &map["arg"];
    let no_arg = |mv: Move| {
        if arg.is_null() {
            Ok(mv)
        } else {
            Err(format!("{at}: {} holds the argument {arg}", mv.name()))
        }
    };
    let mv = match map["move"].as_str() {
        Some("reserve") => Move::Reserve {
            amount: micro(arg, at)?,
        },
        Some("dispatch") => no_arg(Move::Dispatch)?,
        Some("void") => no_arg(Move::Void)?,
        Some("settle") => Move::Settle {
            cost: micro(arg, at)?,
        },
        Some("expire") => no_arg(Move::Expire)?,
        _ => {
            return Err(format!(
                "{at}: the move {} is outside the list",
                map["move"]
            ))
        }
    };
    let answer = if map["answer"] == "ok" {
        Ok(())
    } else {
        Err(word(
            &map["answer"],
            &SpendCause::ALL,
            SpendCause::as_str,
            at,
        )?)
    };
    let want = Want {
        answer,
        state: word(&map["state"], &State::ALL, State::as_str, at)?,
        spent: micro(&map["spent"], at)?,
        held: micro(&map["held"], at)?,
        cost: micro(&map["cost"], at)?,
    };
    Ok((mv, want))
}

fn read_trace(text: &str, line: usize) -> Result<Trace, String> {
    let at = format!("line {line}");
    let value: Value = serde_json::from_str(text).map_err(|error| format!("{at}: {error}"))?;
    let map = object(&value, &TRACE_KEYS, &at)?;
    in_form(&value, text, &at)?;
    let cap_minor = match &map["cap"] {
        Value::Null => None,
        cap => Some(number(cap, &at)?),
    };
    let active = match map["active"].as_str() {
        Some("yes") => true,
        Some("no") => false,
        _ => return Err(format!("{at}: active is {}", map["active"])),
    };
    let account = Account {
        cap_minor,
        active,
        spent: micro(&map["spent"], &at)?,
        held: micro(&map["held"], &at)?,
    };
    let list = map["moves"]
        .as_array()
        .ok_or_else(|| format!("{at}: moves is no list"))?;
    if list.is_empty() || list.len() > MOST_MOVES {
        return Err(format!(
            "{at}: the trace has {} moves, the form 1 to {MOST_MOVES}",
            list.len()
        ));
    }
    let moves = list
        .iter()
        .enumerate()
        .map(|(index, mv)| read_move(mv, &format!("{at} move {}", index + 1)))
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Trace {
        line,
        account,
        moves,
    })
}

fn read_file(text: &str) -> Result<Vec<Trace>, String> {
    let mut lines = text.lines();
    let first = lines.next().ok_or("the file is empty")?;
    let header: Value = serde_json::from_str(first).map_err(|error| format!("line 1: {error}"))?;
    let map = object(&header, &HEADER_KEYS, "line 1")?;
    in_form(&header, first, "line 1")?;
    number(&map["seed"], "line 1")?;
    let count = number(&map["count"], "line 1")?;
    let traces = lines
        .enumerate()
        .map(|(index, text)| read_trace(text, index + 2))
        .collect::<Result<Vec<_>, String>>()?;
    for (index, trace) in traces.iter().enumerate() {
        let (kind, want) = if index < DIRECTED {
            ("directed", DIRECTED_MOVES)
        } else {
            ("drawn", MOST_MOVES)
        };
        if trace.moves.len() != want {
            return Err(format!(
                "line {}: the trace has {} moves, and a {kind} trace {want}",
                trace.line,
                trace.moves.len()
            ));
        }
    }
    let drawn = traces.len().saturating_sub(DIRECTED);
    if drawn < LEAST_TRACES {
        return Err(format!(
            "the file holds {drawn} drawn traces, fewer than {LEAST_TRACES}"
        ));
    }
    if u64::try_from(traces.len()) != Ok(count) {
        return Err(format!(
            "the file holds {} traces and its count is {count}",
            traces.len()
        ));
    }
    Ok(traces)
}

fn replay(trace: &Trace) -> Vec<String> {
    let mut faults = Vec::new();
    let mut account = trace.account;
    let mut hold = Hold::new();
    for (index, (mv, want)) in trace.moves.iter().enumerate() {
        let at = format!("line {} move {} {mv:?}", trace.line, index + 1);
        let answer = match step(&account, &hold, *mv) {
            Ok((next_account, next_hold)) => {
                account = next_account;
                hold = next_hold;
                Ok(())
            }
            Err(cause) => Err(cause),
        };
        let want_account = Account {
            cap_minor: trace.account.cap_minor,
            active: trace.account.active,
            spent: want.spent,
            held: want.held,
        };
        if answer != want.answer {
            faults.push(format!(
                "{at}: the model answers {:?}, the code {answer:?}",
                want.answer
            ));
        }
        if account != want_account {
            faults.push(format!(
                "{at}: the model has {want_account:?}, the code {account:?}"
            ));
        }
        if hold.state != want.state || hold.cost != want.cost {
            faults.push(format!(
                "{at}: the model has the state {:?} and the cost {}, the code {:?} and {}",
                want.state, want.cost, hold.state, hold.cost
            ));
        }
    }
    faults
}

fn faults(traces: &[Trace]) -> Vec<String> {
    traces.iter().flat_map(replay).collect()
}

fn check(text: &str) -> Result<(usize, usize), String> {
    let traces = read_file(text)?;
    let faults = faults(&traces);
    let moves: usize = traces.iter().map(|trace| trace.moves.len()).sum();
    if !faults.is_empty() {
        return Err(format!(
            "the code differs from the model in {} places of the {moves} moves of its traces:\n{}",
            faults.len(),
            faults[..faults.len().min(20)].join("\n")
        ));
    }
    directed(&traces[..DIRECTED])?;
    Ok((traces.len(), moves))
}

fn directed(traces: &[Trace]) -> Result<(), String> {
    let pattern = [
        ("reserve", Ok(())),
        ("dispatch", Ok(())),
        ("settle", Ok(())),
        ("reserve", Err(SpendCause::Repeat)),
        ("settle", Err(SpendCause::State)),
    ];
    let mut classes = [0; 3];
    for trace in traces {
        let shape: Vec<(&str, Result<(), SpendCause>)> = trace
            .moves
            .iter()
            .map(|(mv, want)| (mv.name(), want.answer))
            .collect();
        let costs = match (trace.moves[0].0, trace.moves[2].0, trace.moves[4].0) {
            (Move::Reserve { amount }, Move::Settle { cost }, Move::Settle { cost: again })
                if shape == pattern && again == cost + MICRO_PER_MINOR =>
            {
                Some((amount, cost))
            }
            _ => None,
        };
        let class = match costs {
            Some((_, 0)) => 0,
            Some((amount, cost)) if cost < amount => 1,
            Some((amount, cost)) if cost == amount => 2,
            _ => {
                return Err(format!(
                    "line {}: a directed trace is a reserve, a dispatch and a settle at or under \
                     the amount that go through, then the reserve again, answered repeat, and a \
                     settle at one more cost, answered state",
                    trace.line
                ))
            }
        };
        classes[class] += 1;
    }
    if classes != [2, 2, 2] {
        return Err(format!(
            "the costs of the directed settles are 0 in {}, below the amount in {} and at it in \
             {}, and the six directed traces have two of each",
            classes[0], classes[1], classes[2]
        ));
    }
    Ok(())
}

fn committed() -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TRACES))
        .expect("the committed traces of formal/r6-spend")
}

fn with_line(text: &str, line: usize, edit: impl Fn(&mut Value)) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut value: Value = serde_json::from_str(&lines[line - 1]).expect("a line of JSON");
    edit(&mut value);
    lines[line - 1] = form_of(&value);
    lines.join("\n")
}

#[test]
fn formal_traces() {
    let text = committed();
    let (traces, moves) = check(&text).unwrap_or_else(|fault| panic!("traces.jsonl: {fault}"));
    let written = text.matches("\"move\":").count();
    assert_eq!(
        moves, written,
        "the reader read {moves} moves of the {written} that the file writes"
    );
    println!("formal_traces traces={traces} moves={moves}");
}

fn changes(want: &Want) -> Vec<(&'static str, Want)> {
    let mut out = Vec::new();
    let answers = std::iter::once(Ok(())).chain(SpendCause::ALL.into_iter().map(Err));
    for answer in answers.filter(|answer| *answer != want.answer) {
        out.push(("answer", Want { answer, ..*want }));
    }
    for state in State::ALL.into_iter().filter(|state| *state != want.state) {
        out.push(("state", Want { state, ..*want }));
    }
    let others = |value: u64| {
        [value + 1, value + MICRO_PER_MINOR, 0]
            .into_iter()
            .filter(move |other| *other != value)
    };
    for spent in others(want.spent) {
        out.push(("spent", Want { spent, ..*want }));
    }
    for held in others(want.held) {
        out.push(("held", Want { held, ..*want }));
    }
    for cost in others(want.cost) {
        out.push(("cost", Want { cost, ..*want }));
    }
    out
}

#[test]
fn a_changed_value_of_any_move_of_any_trace_is_a_fault() {
    let traces = read_file(&committed()).expect("the committed traces");
    let mut unseen = Vec::new();
    for trace in &traces {
        for index in 0..trace.moves.len() {
            for (key, want) in changes(&trace.moves[index].1) {
                let mut changed = trace.clone();
                changed.moves[index].1 = want;
                if replay(&changed).is_empty() {
                    unseen.push(format!("line {} move {} {key}", trace.line, index + 1));
                }
            }
        }
    }
    assert!(
        unseen.is_empty(),
        "{} changed values of the traces gave no fault, among them {:?}",
        unseen.len(),
        &unseen[..unseen.len().min(5)]
    );
}

struct TextEdit {
    line: usize,
    index: usize,
    kind: String,
    value: &'static str,
}

fn certain_values(trace: &Value, index: usize) -> Vec<&'static str> {
    let moves = trace["moves"].as_array().expect("a list of moves");
    let mv = &moves[index];
    let mut values = Vec::new();
    if mv["answer"] == "ok" {
        values.push("move");
        if mv["arg"].is_u64() {
            values.push("arg");
        }
    }
    if index == 0 {
        values.extend(["opening spent", "opening held"]);
        let reserve_on_new = moves.iter().enumerate().any(|(at, other)| {
            let before = if at == 0 {
                "new"
            } else {
                moves[at - 1]["state"].as_str().unwrap_or("")
            };
            other["move"] == "reserve" && other["arg"].as_u64().unwrap_or(0) > 0 && before == "new"
        });
        if reserve_on_new && trace["active"] == "yes" {
            values.push("cap");
        }
        if reserve_on_new {
            values.push("active");
        }
    }
    values.extend(OUTCOMES);
    values
}

fn apply(trace: &mut Value, index: usize, value: &str) {
    let plus_one = |field: &mut Value| *field = json!(field.as_u64().expect("a number") + 1);
    match value {
        "opening spent" => plus_one(&mut trace["spent"]),
        "opening held" => plus_one(&mut trace["held"]),
        "cap" => {
            trace["cap"] = if trace["cap"].is_null() {
                json!(3)
            } else {
                Value::Null
            }
        }
        "active" => {
            trace["active"] = json!(if trace["active"] == "yes" {
                "no"
            } else {
                "yes"
            })
        }
        _ => {
            let mv = &mut trace["moves"][index];
            match value {
                "move" => {
                    let other = match mv["move"].as_str().expect("a word") {
                        "reserve" => "settle",
                        "settle" => "reserve",
                        "dispatch" => "void",
                        "void" => "expire",
                        _ => "dispatch",
                    };
                    mv["move"] = json!(other);
                }
                "arg" => plus_one(&mut mv["arg"]),
                "answer" => mv["answer"] = json!(if mv["answer"] == "ok" { "state" } else { "ok" }),
                "state" => {
                    let now = STATE_WORDS
                        .iter()
                        .position(|state| mv["state"] == *state)
                        .expect("a state");
                    mv["state"] = json!(STATE_WORDS[(now + 1) % STATE_WORDS.len()]);
                }
                field => plus_one(&mut mv[field]),
            }
        }
    }
}

fn text_edits(text: &str) -> (Vec<TextEdit>, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut edits = Vec::new();
    let mut wanted: Vec<(String, &'static str)> = Vec::new();
    for (offset, line) in lines[1..].iter().enumerate() {
        let trace: Value = serde_json::from_str(line).expect("a trace line");
        let moves = trace["moves"].as_array().expect("a list of moves");
        for mv in moves {
            let kind = mv["move"].as_str().expect("a word").to_string();
            for value in ["move", "answer", "state", "spent", "held", "cost"] {
                wanted.push((kind.clone(), value));
            }
            if mv["arg"].is_u64() {
                wanted.push((kind.clone(), "arg"));
            }
        }
        let places = [0, moves.len() / 2, moves.len() - 1];
        for (place, index) in places.into_iter().enumerate() {
            let values = certain_values(&trace, index);
            let value = values[(offset * places.len() + place) % values.len()];
            edits.push(TextEdit {
                line: offset + 2,
                index,
                kind: moves[index]["move"].as_str().expect("a word").to_string(),
                value,
            });
        }
    }
    for opening in OPENINGS {
        wanted.push(("trace".to_string(), opening));
    }
    wanted.sort();
    wanted.dedup();
    let hit = |edits: &[TextEdit], kind: &str, value: &str| {
        edits.iter().any(|edit| {
            edit.value == value
                && (edit.kind == kind || (kind == "trace" && OPENINGS.contains(&value)))
        })
    };
    for (kind, value) in &wanted {
        if hit(&edits, kind, value) {
            continue;
        }
        'search: for (offset, line) in lines[1..].iter().enumerate() {
            let trace: Value = serde_json::from_str(line).expect("a trace line");
            let moves = trace["moves"].as_array().expect("a list of moves");
            for (index, mv) in moves.iter().enumerate() {
                let fits = kind == "trace" || mv["move"] == kind.as_str();
                if fits && certain_values(&trace, index).contains(value) {
                    edits.push(TextEdit {
                        line: offset + 2,
                        index,
                        kind: mv["move"].as_str().expect("a word").to_string(),
                        value,
                    });
                    break 'search;
                }
            }
        }
    }
    let missed = wanted
        .iter()
        .filter(|(kind, value)| !hit(&edits, kind, value))
        .map(|(kind, value)| format!("{kind} {value}"))
        .collect();
    (edits, missed)
}

#[test]
fn a_changed_value_in_the_text_of_every_trace_fails_the_check() {
    let text = committed();
    let (edits, missed) = text_edits(&text);
    assert!(
        missed.is_empty(),
        "no move of the file takes these changes for certain: {missed:?}"
    );
    let traces = text.lines().count() - 1;
    let short: Vec<usize> = (2..traces + 2)
        .filter(|line| edits.iter().filter(|edit| edit.line == *line).count() < 3)
        .collect();
    assert!(
        short.is_empty(),
        "lines with fewer than three edits: {short:?}"
    );
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let size = edits.len().div_ceil(threads);
    let passed: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = edits
            .chunks(size)
            .map(|chunk| {
                let text = &text;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|edit| {
                            let changed = with_line(text, edit.line, |trace| {
                                apply(trace, edit.index, edit.value)
                            });
                            let name = format!(
                                "line {} move {} {} {}",
                                edit.line,
                                edit.index + 1,
                                edit.kind,
                                edit.value
                            );
                            match check(&changed) {
                                Err(fault) if fault.contains("differs from the model") => None,
                                Err(fault) => Some(format!("{name}: refused as {fault}")),
                                Ok(_) => Some(format!("{name}: passed")),
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("a worker thread"))
            .collect()
    });
    assert!(
        passed.is_empty(),
        "{} of {} changed texts did not fail the check as a fault, among them {:?}",
        passed.len(),
        edits.len(),
        &passed[..passed.len().min(5)]
    );
}

#[test]
fn a_changed_spent_at_every_move_of_every_trace_is_a_fault_of_that_move() {
    let text = committed();
    let mut changed = 0;
    let mut unnamed = Vec::new();
    for index in 0..MOST_MOVES {
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        let mut places = Vec::new();
        for (offset, line) in lines.iter_mut().enumerate().skip(1) {
            let mut trace: Value = serde_json::from_str(line).expect("a trace line");
            if trace["moves"].as_array().expect("a list of moves").len() > index {
                apply(&mut trace, index, "spent");
                *line = form_of(&trace);
                places.push(format!("line {} move {} ", offset + 1, index + 1));
            }
        }
        let traces = read_file(&lines.join("\n")).expect("the changed traces");
        let found = faults(&traces);
        changed += places.len();
        unnamed.extend(
            places
                .into_iter()
                .filter(|place| !found.iter().any(|fault| fault.starts_with(place))),
        );
    }
    assert_eq!(
        changed,
        text.matches("\"move\":").count(),
        "every move of the file is changed once"
    );
    assert!(
        unnamed.is_empty(),
        "{} changed moves gave no fault of their own, among them {:?}",
        unnamed.len(),
        &unnamed[..unnamed.len().min(5)]
    );
}

#[test]
fn a_fault_names_its_file_line_and_its_move() {
    let text = committed();
    let middle = text.lines().count() / 2;
    for (line, index) in [(2, 0), (middle, 2)] {
        let changed = with_line(&text, line, |trace| apply(trace, index, "spent"));
        let fault = check(&changed).expect_err("a changed spent");
        let name = format!("line {line} move {} ", index + 1);
        assert!(fault.contains(&name), "{name}: {fault}");
    }
}

fn with_header(count: usize, lines: &[&str]) -> String {
    let mut text = format!("{{\"seed\":3,\"count\":{count}}}");
    for line in lines {
        text.push('\n');
        text.push_str(line);
    }
    text
}

#[test]
fn the_floor_refuses_a_file_without_the_directed_traces() {
    let text = committed();
    let drawn: Vec<&str> = text.lines().skip(7).collect();
    let fault = check(&with_header(drawn.len(), &drawn)).expect_err("no directed traces");
    assert!(fault.contains("line 2: the trace has 12 moves"), "{fault}");
}

#[test]
fn the_floor_refuses_199_drawn_traces() {
    let text = committed();
    let lines: Vec<&str> = text.lines().skip(1).collect();
    let fewer = &lines[..lines.len() - 1];
    let fault = check(&with_header(fewer.len(), fewer)).expect_err("199 drawn traces");
    assert!(
        fault.contains("the file holds 199 drawn traces, fewer than 200"),
        "{fault}"
    );
}

#[test]
fn the_floor_refuses_one_trace_written_206_times() {
    let text = committed();
    let lines: Vec<&str> = text.lines().collect();
    for (line, fragment) in [
        (2, "line 8: the trace has 5 moves"),
        (8, "line 2: the trace has 12 moves"),
    ] {
        let copies = vec![lines[line - 1]; 206];
        let fault = check(&with_header(206, &copies)).expect_err("one trace 206 times");
        assert!(fault.contains(fragment), "line {line}: {fault}");
    }
}

#[test]
fn the_floor_refuses_directed_traces_that_are_not_the_six() {
    let text = committed();
    let lines: Vec<&str> = text.lines().collect();
    let mut first_six_times = vec![lines[1]; 6];
    first_six_times.extend(&lines[7..]);
    let mut no_below = lines[1..].to_vec();
    no_below[0] = lines[3];
    no_below[1] = lines[4];
    let mut swapped = lines[1..].to_vec();
    swapped.swap(5, 6);
    let cases = [
        (
            "the first directed trace six times",
            with_header(206, &first_six_times),
            "the costs of the directed settles",
        ),
        (
            "two settles at the amount in place of the two below it",
            with_header(206, &no_below),
            "the costs of the directed settles are 0 in 2, below the amount in 0 and at it in 4",
        ),
        (
            "a drawn trace among the directed ones",
            with_header(206, &swapped),
            "the trace has",
        ),
        (
            "a last settle with the same cost",
            with_line(&text, 3, |trace| {
                trace["moves"][4]["arg"] = trace["moves"][2]["arg"].clone()
            }),
            "line 3: a directed trace",
        ),
        (
            "no reserve answered repeat",
            with_line(&text, 4, |trace| {
                let moves = trace["moves"].as_array_mut().expect("a list of moves");
                moves[3] = moves[2].clone();
                moves[3]["move"] = json!("dispatch");
                moves[3]["arg"] = Value::Null;
                moves[3]["answer"] = json!("used");
            }),
            "line 4: a directed trace",
        ),
    ];
    let taken: Vec<String> = cases
        .iter()
        .filter_map(|(name, text, fragment)| match check(text) {
            Err(fault) if fault.contains(fragment) => None,
            Err(fault) => Some(format!("{name}: refused as {fault}")),
            Ok(_) => Some(format!("{name}: accepted")),
        })
        .collect();
    assert!(
        taken.is_empty(),
        "the floor took or refused otherwise: {taken:?}"
    );
}

#[test]
fn the_reader_refuses_a_file_outside_the_form() {
    let text = committed();
    let fewer = text
        .lines()
        .take(200)
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                "{\"seed\":3,\"count\":199}"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let written = |old: &str, new: &str| {
        assert!(text.contains(old), "the committed traces hold {old}");
        text.replacen(old, new, 1)
    };
    let first = "\"move\":\"reserve\",\"arg\":2,";
    let form = "is not written in the line form";
    let cases = [
        (
            "count",
            with_line(&text, 1, |head| head["count"] = json!(205)),
            "its count is 205",
        ),
        (
            "count above",
            with_line(&text, 1, |head| head["count"] = json!(207)),
            "its count is 207",
        ),
        (
            "key twice in the header",
            written("\"count\":206}", "\"count\":5,\"count\":206}"),
            form,
        ),
        (
            "key twice in a move",
            written(first, "\"move\":\"reserve\",\"arg\":5,\"arg\":2,"),
            form,
        ),
        (
            "key twice, the second with a space before its colon",
            written(first, "\"move\":\"reserve\",\"arg\":2,\"arg\" :3,"),
            form,
        ),
        (
            "key twice, the second with an escaped first letter",
            written(first, "\"move\":\"reserve\",\"arg\":2,\"\\u0061rg\":3,"),
            form,
        ),
        (
            "move twice, the second with an escaped first letter",
            written(
                first,
                "\"move\":\"reserve\",\"\\u006dove\":\"settle\",\"arg\":2,",
            ),
            form,
        ),
        (
            "moves twice",
            written("\"moves\":[", "\"moves\":[],\"moves\":["),
            form,
        ),
        (
            "keys in another order",
            written("{\"seed\":3,\"count\":206}", "{\"count\":206,\"seed\":3}"),
            form,
        ),
        (
            "a space after a colon",
            written(first, "\"move\":\"reserve\",\"arg\": 2,"),
            form,
        ),
        (
            "no move",
            with_line(&text, 2, |trace| trace["moves"] = json!([])),
            "the trace has 0 moves",
        ),
        (
            "thirteen moves",
            with_line(&text, 2, |trace| {
                let moves = trace["moves"].as_array_mut().expect("a list of moves");
                let last = moves[moves.len() - 1].clone();
                moves.resize(13, last);
            }),
            "the trace has 13 moves",
        ),
        ("fewer", fewer, "fewer than 200"),
        (
            "seed",
            with_line(&text, 1, |head| head["seed"] = json!("x")),
            "is no whole number",
        ),
        (
            "renamed key",
            with_line(&text, 2, |trace| {
                let mv = trace["moves"][0].as_object_mut().expect("a move");
                let cost = mv.remove("cost").expect("a cost");
                mv.insert("costs".to_string(), cost);
            }),
            "the keys are",
        ),
        (
            "missing key",
            with_line(&text, 2, |trace| {
                trace["moves"][0]
                    .as_object_mut()
                    .expect("a move")
                    .remove("held");
            }),
            "the keys are",
        ),
        (
            "extra key",
            with_line(&text, 2, |trace| {
                trace
                    .as_object_mut()
                    .expect("a trace")
                    .insert("note".to_string(), json!(1));
            }),
            "the keys are",
        ),
        (
            "move",
            with_line(&text, 2, |trace| trace["moves"][1]["move"] = json!("send")),
            "the move \"send\" is outside the list",
        ),
        (
            "answer",
            with_line(&text, 2, |trace| {
                trace["moves"][3]["answer"] = json!("again")
            }),
            "again is outside the list",
        ),
        (
            "state",
            with_line(&text, 2, |trace| trace["moves"][0]["state"] = json!("done")),
            "done is outside the list",
        ),
        (
            "a state that begins a word of the list",
            with_line(&text, 2, |trace| trace["moves"][1]["state"] = json!("se")),
            "se is outside the list",
        ),
        (
            "active",
            with_line(&text, 2, |trace| trace["active"] = json!("maybe")),
            "active is \"maybe\"",
        ),
        (
            "argument of dispatch",
            with_line(&text, 2, |trace| trace["moves"][1]["arg"] = json!(1)),
            "dispatch holds the argument 1",
        ),
        (
            "no argument of reserve",
            with_line(&text, 2, |trace| trace["moves"][0]["arg"] = json!(null)),
            "null is no whole number",
        ),
        (
            "fraction",
            with_line(&text, 2, |trace| trace["spent"] = json!(0.5)),
            "0.5 is no whole number",
        ),
        (
            "negative",
            with_line(&text, 2, |trace| trace["held"] = json!(-1)),
            "-1 is no whole number",
        ),
        (
            "too large",
            with_line(&text, 2, |trace| {
                trace["moves"][0]["arg"] = json!(u64::MAX / 2)
            }),
            "does not fit a u64",
        ),
    ];
    let taken: Vec<String> = cases
        .iter()
        .filter_map(|(name, text, fragment)| match read_file(text) {
            Err(fault) if fault.contains(fragment) => None,
            Err(fault) => Some(format!("{name}: refused as {fault}")),
            Ok(_) => Some(format!("{name}: accepted")),
        })
        .collect();
    assert!(
        taken.is_empty(),
        "the reader took or refused otherwise: {taken:?}"
    );
}
