use halvo_route::spend::{step, Account, Hold, Move, SpendCause, State};
use serde_json::{Map, Value};

const TRACES: &str = "../../formal/r6-spend/traces.jsonl";
const MICRO_PER_MINOR: u64 = 10_000;
const LEAST_TRACES: usize = 200;
const HEADER_KEYS: [&str; 2] = ["count", "seed"];
const TRACE_KEYS: [&str; 5] = ["active", "cap", "held", "moves", "spent"];
const MOVE_KEYS: [&str; 7] = ["answer", "arg", "cost", "held", "move", "spent", "state"];

struct Want {
    answer: Result<(), SpendCause>,
    state: State,
    spent: u64,
    held: u64,
    cost: u64,
}

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
    let moves = map["moves"]
        .as_array()
        .ok_or_else(|| format!("{at}: moves is no list"))?
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
    let header: Value = serde_json::from_str(lines.next().ok_or("the file is empty")?)
        .map_err(|error| format!("line 1: {error}"))?;
    let header = object(&header, &HEADER_KEYS, "line 1")?;
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

#[test]
fn step_answers_every_move_of_the_committed_traces_as_the_model_does() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TRACES);
    let text = std::fs::read_to_string(&path).expect("the committed traces of formal/r6-spend");
    let (traces, moves) = check(&text).unwrap_or_else(|fault| panic!("traces.jsonl: {fault}"));
    println!("formal_traces traces={traces} moves={moves}");
}
