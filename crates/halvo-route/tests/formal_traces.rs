use halvo_route::spend::{step, Account, Hold, Move, SpendCause, State};
use serde_json::{Map, Value};

const TRACES: &str = "../../formal/r6-spend/traces.jsonl";
const MICRO_PER_MINOR: u64 = 10_000;
const LEAST_TRACES: usize = 200;
const MOST_MOVES: usize = 12;
const HEADER_KEYS: [&str; 2] = ["count", "seed"];
const TRACE_KEYS: [&str; 5] = ["active", "cap", "held", "moves", "spent"];
const MOVE_KEYS: [&str; 7] = ["answer", "arg", "cost", "held", "move", "spent", "state"];

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

fn once(text: &str, keys: &[(&str, usize)], at: &str) -> Result<(), String> {
    for (key, want) in keys {
        let found = text.matches(&format!("\"{key}\":")).count();
        if found != *want {
            return Err(format!(
                "{at}: the key {key} is written {found} times, the form has {want}"
            ));
        }
    }
    Ok(())
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
    let n = list.len();
    once(
        text,
        &[
            ("cap", 1),
            ("active", 1),
            ("moves", 1),
            ("spent", n + 1),
            ("held", n + 1),
            ("move", n),
            ("arg", n),
            ("answer", n),
            ("state", n),
            ("cost", n),
        ],
        &at,
    )?;
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
    let header = object(&header, &HEADER_KEYS, "line 1")?;
    once(first, &[("count", 1), ("seed", 1)], "line 1")?;
    number(&header["seed"], "line 1")?;
    let count = number(&header["count"], "line 1")?;
    let traces = lines
        .enumerate()
        .map(|(index, text)| read_trace(text, index + 2))
        .collect::<Result<Vec<_>, String>>()?;
    if traces.len() < LEAST_TRACES {
        return Err(format!(
            "the file holds {} traces, fewer than {LEAST_TRACES}",
            traces.len()
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
    Ok((traces.len(), moves))
}

fn committed() -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TRACES))
        .expect("the committed traces of formal/r6-spend")
}

fn edited(line: usize, edit: impl Fn(&mut Value)) -> String {
    let mut lines: Vec<String> = committed().lines().map(str::to_string).collect();
    let mut value: Value = serde_json::from_str(&lines[line - 1]).expect("a line of JSON");
    edit(&mut value);
    lines[line - 1] = value.to_string();
    lines.join("\n")
}

#[test]
fn step_answers_every_move_of_the_committed_traces_as_the_model_does() {
    let (traces, moves) =
        check(&committed()).unwrap_or_else(|fault| panic!("traces.jsonl: {fault}"));
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
        [value + 1, 0]
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

#[test]
fn a_changed_first_or_last_trace_fails_the_check() {
    let last = committed().lines().count();
    for (line, key) in [(2, "spent"), (last, "cost")] {
        let text = edited(line, |trace| {
            let moves = trace["moves"].as_array_mut().expect("a list of moves");
            let mv = if line == 2 {
                &mut moves[0]
            } else {
                moves.last_mut().expect("a move")
            };
            mv[key] = serde_json::json!(mv[key].as_u64().expect("a number") + 1);
        });
        match check(&text) {
            Err(fault) => assert!(
                fault.contains("differs from the model"),
                "line {line}: {fault}"
            ),
            Ok(_) => panic!("line {line} with a changed {key} passed the check"),
        }
    }
}

#[test]
fn the_reader_refuses_a_file_outside_the_form() {
    use serde_json::json;
    let fewer = committed()
        .lines()
        .take(200)
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                "{\"count\":199,\"seed\":3}"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let written = |old: &str, new: &str| {
        let text = committed();
        assert!(text.contains(old), "the committed traces hold {old}");
        text.replacen(old, new, 1)
    };
    let cases = [
        (
            "count",
            edited(1, |head| head["count"] = json!(205)),
            "its count is 205",
        ),
        (
            "count above",
            edited(1, |head| head["count"] = json!(207)),
            "its count is 207",
        ),
        (
            "key twice in the header",
            written("\"count\":206}", "\"count\":5,\"count\":206}"),
            "the key count is written 2 times",
        ),
        (
            "key twice in a move",
            written(
                "\"move\":\"reserve\",\"arg\":2,",
                "\"move\":\"reserve\",\"arg\":5,\"arg\":2,",
            ),
            "the key arg is written 6 times, the form has 5",
        ),
        (
            "moves twice",
            written("\"moves\":[", "\"moves\":[],\"moves\":["),
            "the key moves is written 2 times",
        ),
        (
            "no move",
            edited(2, |trace| trace["moves"] = json!([])),
            "the trace has 0 moves",
        ),
        (
            "thirteen moves",
            edited(2, |trace| {
                let moves = trace["moves"].as_array_mut().expect("a list of moves");
                let last = moves[moves.len() - 1].clone();
                moves.resize(13, last);
            }),
            "the trace has 13 moves",
        ),
        ("fewer", fewer, "fewer than 200"),
        (
            "seed",
            edited(1, |head| head["seed"] = json!("x")),
            "is no whole number",
        ),
        (
            "renamed key",
            edited(2, |trace| {
                let mv = trace["moves"][0].as_object_mut().expect("a move");
                let cost = mv.remove("cost").expect("a cost");
                mv.insert("costs".to_string(), cost);
            }),
            "the keys are",
        ),
        (
            "missing key",
            edited(2, |trace| {
                trace["moves"][0]
                    .as_object_mut()
                    .expect("a move")
                    .remove("held");
            }),
            "the keys are",
        ),
        (
            "extra key",
            edited(2, |trace| {
                trace
                    .as_object_mut()
                    .expect("a trace")
                    .insert("note".to_string(), json!(1));
            }),
            "the keys are",
        ),
        (
            "move",
            edited(2, |trace| trace["moves"][1]["move"] = json!("send")),
            "the move \"send\" is outside the list",
        ),
        (
            "answer",
            edited(2, |trace| trace["moves"][3]["answer"] = json!("again")),
            "again is outside the list",
        ),
        (
            "state",
            edited(2, |trace| trace["moves"][0]["state"] = json!("done")),
            "done is outside the list",
        ),
        (
            "active",
            edited(2, |trace| trace["active"] = json!("maybe")),
            "active is \"maybe\"",
        ),
        (
            "argument of dispatch",
            edited(2, |trace| trace["moves"][1]["arg"] = json!(1)),
            "dispatch holds the argument 1",
        ),
        (
            "no argument of reserve",
            edited(2, |trace| trace["moves"][0]["arg"] = json!(null)),
            "null is no whole number",
        ),
        (
            "fraction",
            edited(2, |trace| trace["spent"] = json!(0.5)),
            "0.5 is no whole number",
        ),
        (
            "negative",
            edited(2, |trace| trace["held"] = json!(-1)),
            "-1 is no whole number",
        ),
        (
            "too large",
            edited(2, |trace| trace["moves"][0]["arg"] = json!(u64::MAX / 2)),
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
