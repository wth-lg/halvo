use halvo_learn::{routine_step, Mark, RoutineState};
use serde_json::Value;

const SAMPLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/r5/trains/sample.jsonl"
);

const TRAIN_DAYS: u32 = 730;

type Event = (u32, Mark, Option<u32>);

fn day(value: &Value) -> u32 {
    let n = value.as_u64().expect("a day is a whole number");
    u32::try_from(n).expect("a day fits u32")
}

fn train_events(train: &Value) -> Vec<Event> {
    train["events"]
        .as_array()
        .expect("events is a list")
        .iter()
        .map(|event| {
            let mark = Mark::from_token(event["mark"].as_str().expect("mark is a name"))
                .expect("mark is none or stale");
            let period = match &event["period"] {
                Value::Null => None,
                period => Some(day(period)),
            };
            (day(&event["day"]), mark, period)
        })
        .collect()
}

fn window(days: &[u32], today: u32) -> Vec<u32> {
    days.iter()
        .copied()
        .filter(|&d| today.saturating_sub(92) <= d && d <= today)
        .collect()
}

fn play(days: &[u32], whole_list: bool) -> Vec<Event> {
    let mut prev: Option<RoutineState> = None;
    let mut before = (Mark::None, None);
    let mut events = Vec::new();
    for today in 1..=TRAIN_DAYS {
        let state = if whole_list {
            routine_step(prev.as_ref(), days, today)
        } else {
            routine_step(prev.as_ref(), &window(days, today), today)
        };
        if (state.mark, state.period) != before {
            events.push((today, state.mark, state.period));
        }
        before = (state.mark, state.period);
        prev = Some(state);
    }
    events
}

#[test]
fn routine_sample() {
    let text =
        std::fs::read_to_string(SAMPLE).expect("fixtures/r5/trains/sample.jsonl is readable");
    let trains: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect();
    assert_eq!(trains.len(), 135, "the sample has 135 trains");
    let mut stale = 0;
    for train in &trains {
        let name = format!("{} train {}", train["cell"], train["train"]);
        let days: Vec<u32> = train["days"]
            .as_array()
            .expect("days is a list")
            .iter()
            .map(day)
            .collect();
        let wanted = train_events(train);
        stale += wanted.iter().filter(|e| e.1 == Mark::Stale).count();
        assert_eq!(play(&days, false), wanted, "{name}, days of the window");
        assert_eq!(play(&days, true), wanted, "{name}, the whole list of days");
    }
    assert!(stale > 0, "the sample holds the mark stale");
}
