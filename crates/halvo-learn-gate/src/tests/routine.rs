use std::path::Path;

use halvo_schema::learning::names::Mark;

use crate::cli::Cause;
use crate::routine::{
    line, play, play_all, read_sample, report, run, Event, Played, Stream, Train, LAST_DAY,
    REPLAY_FROM, SAMPLE_PATH, TRAINS,
};

fn sample() -> Vec<Train> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(SAMPLE_PATH);
    read_sample(&std::fs::read(path).expect("the sample file")).expect("the sample's trains")
}

fn first_stale(trains: &[Train]) -> Train {
    trains
        .iter()
        .find(|t| t.events.iter().any(|e| e.mark == Mark::Stale))
        .cloned()
        .expect("a train with the mark stale")
}

fn differs_on(train: &Train, stream: Stream) -> Option<u32> {
    play(train, stream).expect("a play").differs_on
}

const TRAIN: &str = r#"{"cell":"7-0-0","train":1,"kind":"same","days":[1,8,15],"events":[{"day":15,"mark":"none","period":7}]}"#;

fn refused(text: &str) -> String {
    read_sample(text.as_bytes()).expect_err("a refused sample")
}

#[test]
fn routine_reads_every_train_of_the_sample() {
    let trains = sample();
    assert_eq!(trains.len(), TRAINS);
    assert!(trains.iter().all(|t| !t.days.is_empty()));
    assert!(trains.iter().any(|t| t.days.contains(&REPLAY_FROM)));
}

#[test]
fn routine_gives_the_trials_figures_on_every_day_of_a_stale_train_in_both_streams() {
    let train = first_stale(&sample());
    for stream in Stream::BOTH {
        assert_eq!(differs_on(&train, stream), None, "{stream:?}");
    }
}

#[test]
fn routine_plays_each_train_in_both_streams_in_order_with_runs_from_day_120_and_expiry() {
    let train = sample().remove(0);
    let count = |keep: &dyn Fn(u32) -> bool| train.days.iter().filter(|d| keep(**d)).count() as u32;
    let last = *train.days.last().expect("a day");
    let gone = |d: u32| d + 93 <= last;
    assert!(train.days.contains(&REPLAY_FROM));
    let mut changed = train.clone();
    changed.events[0].period = changed.events[0].period.map(|p| p + 1);
    let first = changed.events[0].day;
    let played: Vec<Played> = play_all(&[train.clone(), changed])
        .into_iter()
        .map(|r| r.expect("a play"))
        .collect();
    let [summaries, replay, changed_summaries, changed_replay] = played[..] else {
        panic!("{} plays, not 4", played.len());
    };
    assert_eq!(changed_summaries.differs_on, Some(first));
    assert_eq!(changed_replay.differs_on, Some(first));
    assert_eq!(replay.differs_on, None);
    assert_eq!(summaries.differs_on, None);
    let late = count(&|d| d >= REPLAY_FROM);
    assert_eq!(
        (replay.runs, replay.summaries),
        (late, count(&|_| true) - late)
    );
    assert_eq!((summaries.runs, summaries.summaries), (0, count(&|_| true)));
    assert_eq!(summaries.expired_summaries, count(&gone));
    assert_eq!(
        replay.expired_summaries,
        count(&|d| d < REPLAY_FROM && gone(d))
    );
    assert!(summaries.expired_runs > 0);
    assert_eq!(
        replay.expired_runs - summaries.expired_runs,
        2 * count(&|d| d >= REPLAY_FROM && gone(d))
    );
}

#[test]
fn routine_a_changed_period_of_one_event_differs_on_that_day_in_both_streams() {
    let mut train = sample().remove(0);
    let event = train
        .events
        .iter_mut()
        .find(|e| e.period.is_some())
        .expect("an event with a period");
    event.period = event.period.map(|p| p + 1);
    let day = event.day;
    for stream in Stream::BOTH {
        assert_eq!(differs_on(&train, stream), Some(day), "{stream:?}");
    }
}

#[test]
fn routine_a_changed_mark_of_one_event_differs_on_that_day_in_both_streams() {
    let mut train = first_stale(&sample());
    let event = train
        .events
        .iter_mut()
        .find(|e| e.mark == Mark::Stale)
        .expect("a stale event");
    event.mark = Mark::None;
    let day = event.day;
    for stream in Stream::BOTH {
        assert_eq!(differs_on(&train, stream), Some(day), "{stream:?}");
    }
}

#[test]
fn routine_an_occurrence_left_out_after_day_120_differs_in_both_streams() {
    let mut train = first_stale(&sample());
    let at = train
        .days
        .iter()
        .rposition(|d| *d >= REPLAY_FROM)
        .expect("an occurrence after day 120");
    let day = train.days.remove(at);
    for stream in Stream::BOTH {
        let differs = differs_on(&train, stream).expect("a play that differs");
        assert!(
            differs >= day,
            "{stream:?} differs on {differs}, before {day}"
        );
    }
}

#[test]
fn routine_an_occurrence_moved_by_one_day_before_day_120_differs_in_both_streams() {
    let mut train = sample().remove(0);
    let first = train.events[0].day;
    let at = train
        .days
        .iter()
        .position(|d| *d == first)
        .expect("the first event on an occurrence day");
    train.days[at] -= 1;
    for stream in Stream::BOTH {
        assert_eq!(differs_on(&train, stream), Some(first - 1), "{stream:?}");
    }
}

#[test]
fn routine_figures_on_a_day_are_those_of_the_last_event_at_or_before_it() {
    let train = Train {
        days: vec![1],
        events: vec![
            Event {
                day: 15,
                mark: Mark::None,
                period: Some(7),
            },
            Event {
                day: 40,
                mark: Mark::Stale,
                period: Some(7),
            },
        ],
    };
    assert_eq!(train.figures_on(14), (Mark::None, None));
    assert_eq!(train.figures_on(15), (Mark::None, Some(7)));
    assert_eq!(train.figures_on(39), (Mark::None, Some(7)));
    assert_eq!(train.figures_on(40), (Mark::Stale, Some(7)));
    assert_eq!(train.figures_on(LAST_DAY), (Mark::Stale, Some(7)));
}

#[test]
fn routine_line_is_ok_when_every_play_is_the_same_and_counts_each_that_differs() {
    let same = Played::default();
    let differs = Played {
        differs_on: Some(9),
        ..same
    };
    let all = vec![same; 2 * TRAINS];
    let report = line(TRAINS, &all);
    assert!(report.ok);
    assert_eq!(
        report.lines,
        ["routine trains=135 plays=270 same=270 result=ok"]
    );
    let mut one = all.clone();
    one[200] = differs;
    let report = line(TRAINS, &one);
    assert!(!report.ok);
    assert_eq!(
        report.lines,
        ["routine trains=135 plays=270 same=269 result=miss"]
    );
    let report = line(TRAINS, &all[1..]);
    assert!(!report.ok);
    assert_eq!(
        report.lines,
        ["routine trains=135 plays=270 same=269 result=miss"]
    );
    let mut more = all.clone();
    more.push(differs);
    let report = line(TRAINS, &more);
    assert!(!report.ok);
    assert_eq!(
        report.lines,
        ["routine trains=135 plays=270 same=270 result=miss"]
    );
}

#[test]
fn routine_report_refuses_a_sample_of_another_number_of_trains_with_case() {
    let mut trains = sample();
    trains.pop();
    let refusal = report(&trains).expect_err("a refusal");
    assert_eq!(refusal.cause, Cause::Case);
    assert_eq!(refusal.detail, "the sample holds 134 trains, not 135");
}

#[test]
fn routine_refuses_the_derived_store_with_store_and_a_wrong_call_with_usage() {
    let call = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        run(&args).expect_err("a refusal").cause
    };
    assert_eq!(call(&["--store", "derived"]), Cause::Store);
    for wrong in [
        &["--store", "disk"][..],
        &["--store"],
        &["--extra"],
        &["--store", "memory", "--store", "memory"],
        &["--store", "derived", "x"],
    ] {
        assert_eq!(call(wrong), Cause::Usage, "{wrong:?}");
    }
}

#[test]
fn routine_reads_a_train_and_refuses_each_line_that_is_not_one() {
    let trains = read_sample(format!("{TRAIN}\n").as_bytes()).expect("one train");
    assert_eq!(
        trains,
        [Train {
            days: vec![1, 8, 15],
            events: vec![Event {
                day: 15,
                mark: Mark::None,
                period: Some(7),
            }],
        }]
    );
    let with = |from: &str, to: &str| format!("{}\n", TRAIN.replacen(from, to, 1));
    let bad = [
        TRAIN.to_owned(),
        format!("{TRAIN}\n\n"),
        format!("{TRAIN}\nx\n"),
        with("[1,8,15]", "[0,8,15]"),
        with("[1,8,15]", "[1,8,731]"),
        with("[1,8,15]", "[8,1,15]"),
        with("[1,8,15]", "[1,8,8]"),
        with("[1,8,15]", "[1,8,-15]"),
        with("[1,8,15]", "\"1\""),
        with("\"same\"", "\"other\""),
        with("\"train\":1", "\"train\":0"),
        with("\"cell\":\"7-0-0\"", "\"cell\":7"),
        with("\"mark\":\"none\"", "\"mark\":\"Stale\""),
        with("\"period\":7", "\"period\":0"),
        with("\"period\":7", "\"period\":\"7\""),
        with("\"day\":15", "\"day\":0"),
        with("\"day\":15", "\"day\":731"),
        with(
            "{\"day\":15,\"mark\":\"none\",\"period\":7}",
            "{\"day\":15,\"mark\":\"none\",\"period\":7},{\"day\":15,\"mark\":\"stale\",\"period\":7}",
        ),
        with(
            "{\"day\":15,\"mark\":\"none\",\"period\":7}",
            "{\"day\":15,\"mark\":\"none\"}",
        ),
        with("\"period\":7}", "\"period\":7,\"x\":1}"),
        with("\"kind\":\"same\",", ""),
        with("\"events\"", "\"events\":[],\"x\""),
    ];
    for text in bad {
        let detail = refused(&text);
        assert!(
            detail.starts_with("line ") || detail.starts_with("the sample does not end"),
            "{text}: {detail}"
        );
    }
    assert_eq!(refused(TRAIN), "the sample does not end in one LF");
    assert_eq!(
        refused(&format!("{TRAIN}\nx\n")),
        "line 2 of the sample is no train"
    );
    assert_eq!(
        read_sample(b"\xff\n").expect_err("not UTF-8"),
        "the sample is not UTF-8"
    );
}

#[test]
fn routine_usage_refusals_name_routine_among_the_commands() {
    let none = crate::cli::run(&[]).expect_err("a refusal with no argument");
    assert_eq!(none.cause, Cause::Usage);
    assert_eq!(
        none.line(),
        "REFUSED usage: no command; commands: drift, delete, routine"
    );
    let other = crate::cli::run(&["nosuch".to_owned()]).expect_err("a refusal of nosuch");
    assert_eq!(other.cause, Cause::Usage);
    assert_eq!(
        other.line(),
        "REFUSED usage: an unknown command; commands: drift, delete, routine"
    );
}

fn with_event(train: &Train, event: Event) -> Train {
    let mut changed = train.clone();
    assert!(changed.events.iter().all(|e| e.day != event.day));
    changed.events.push(event);
    changed.events.sort_by_key(|e| e.day);
    changed
}

#[test]
fn routine_a_figure_changed_on_a_quiet_day_alone_differs_on_that_day_in_both_streams() {
    let train = sample().remove(0);
    let quiet = 300;
    assert!(!train.days.contains(&quiet));
    let (mark, period) = train.figures_on(quiet);
    assert_eq!(train.figures_on(quiet + 1), (mark, period));
    let changed = with_event(
        &with_event(
            &train,
            Event {
                day: quiet,
                mark,
                period: Some(period.map_or(1, |p| p + 1)),
            },
        ),
        Event {
            day: quiet + 1,
            mark,
            period,
        },
    );
    for stream in Stream::BOTH {
        assert_eq!(differs_on(&changed, stream), Some(quiet), "{stream:?}");
    }
}

#[test]
fn routine_a_figure_changed_on_day_730_alone_differs_on_day_730_in_both_streams() {
    let train = sample().remove(0);
    let (mark, period) = train.figures_on(LAST_DAY);
    let changed = with_event(
        &train,
        Event {
            day: LAST_DAY,
            mark,
            period: Some(period.map_or(1, |p| p + 1)),
        },
    );
    for stream in Stream::BOTH {
        assert_eq!(differs_on(&changed, stream), Some(LAST_DAY), "{stream:?}");
    }
}
