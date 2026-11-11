use r5_routine::files::parse_targets;
use r5_routine::rule::walk;
use r5_routine::sample::{events, line, SAMPLE_TRAINS};
use r5_routine::train::{grid, train, Kind, Train};

const SAMPLE: &str = include_str!("../../../../fixtures/r5/trains/sample.jsonl");
const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

fn between<'a>(text: &'a str, open: &str, close: &str) -> &'a str {
    let start = text.find(open).expect("the opening key") + open.len();
    let len = text[start..].find(close).expect("the closing key");
    &text[start..start + len]
}

fn parsed(text: &str) -> (String, Train) {
    let cell = between(text, "{\"cell\":\"", "\",\"train\":").to_owned();
    let number: u32 = between(text, "\"train\":", ",\"kind\":\"")
        .parse()
        .expect("a train number");
    let kind = match between(text, "\"kind\":\"", "\",\"days\":[") {
        "same" => Kind::Same,
        "stop" => Kind::Stop,
        "shift" => Kind::Shift,
        other => panic!("kind {other}"),
    };
    let days = between(text, "\"days\":[", "],\"events\":[")
        .split(',')
        .map(|d| d.parse().expect("a day"))
        .collect();
    (cell, Train { number, kind, days })
}

#[test]
fn routine_sample_replays_every_train_through_the_rule_and_requires_its_events() {
    let setting = parse_targets(Some(TARGETS))
        .and_then(|t| t.setting())
        .expect("the three keys of [routine]");
    let lines: Vec<&str> = SAMPLE.lines().collect();
    assert_eq!(lines.len(), grid().len() * SAMPLE_TRAINS.len());
    for text in &lines {
        let (cell_name, sampled) = parsed(text);
        let cell = grid()
            .into_iter()
            .find(|c| c.name() == cell_name)
            .expect("a cell of the grid");
        let replayed = events(&walk(setting, &sampled.days));
        assert_eq!(line(&cell, &sampled, &replayed), *text);
    }
}

#[test]
fn routine_sample_holds_the_trains_that_the_generator_makes() {
    let mut order = Vec::new();
    for text in SAMPLE.lines() {
        let (cell_name, sampled) = parsed(text);
        let cell = grid()
            .into_iter()
            .find(|c| c.name() == cell_name)
            .expect("a cell of the grid");
        assert_eq!(train(&cell, sampled.number), sampled, "{cell_name}");
        order.push((cell.seed, sampled.number));
    }
    let want: Vec<(u64, u32)> = grid()
        .iter()
        .flat_map(|c| SAMPLE_TRAINS.map(|n| (c.seed, n)))
        .collect();
    assert_eq!(order, want);
}
