use r2_probes::trial::{
    aggregate, input_verdicts, layer_verdicts, private_verdicts, render, Cell, Field, HostedRun,
    How, Record,
};
use serde_json::json;

fn record(cell: &str, run: u32, fields: &[(&str, How, serde_json::Value)]) -> Record {
    Record {
        cell: cell.to_string(),
        run,
        not_measurable: None,
        fields: fields
            .iter()
            .map(|(key, how, value)| Field {
                key: key.to_string(),
                how: *how,
                value: value.clone(),
            })
            .collect(),
        detail: serde_json::Value::Null,
    }
}

fn missing(cell: &str, reason: &str) -> Record {
    Record {
        cell: cell.to_string(),
        run: 0,
        not_measurable: Some(reason.to_string()),
        fields: Vec::new(),
        detail: serde_json::Value::Null,
    }
}

fn one(cell: &str, fields: &[(&str, How, serde_json::Value)]) -> Record {
    record(cell, 1, fields)
}

fn lines(cells: &[Cell]) -> Vec<String> {
    cells.iter().map(Cell::line).collect()
}

#[test]
fn a_cell_line_keeps_the_key_order_and_takes_the_lowest_the_largest_and_the_median() {
    let records = vec![
        record(
            "input.windows-2025.enigo",
            1,
            &[
                ("keys", How::Min, json!(20)),
                ("clicks", How::Min, json!(5)),
                ("version", How::Text, json!("0.6.1")),
            ],
        ),
        record(
            "input.windows-2025.enigo",
            2,
            &[
                ("keys", How::Min, json!(19)),
                ("clicks", How::Min, json!(5)),
                ("version", How::Text, json!("0.6.1")),
            ],
        ),
        record(
            "extract.macos-15.apple-vision-fast.text-1.x1.0",
            1,
            &[
                ("call_ms", How::Median, json!(10.04)),
                ("cer_pct", How::Median, json!(0.5)),
                ("stored_bytes", How::Max, json!(1800)),
                ("readable_runs", How::Sum, json!(1)),
            ],
        ),
        record(
            "extract.macos-15.apple-vision-fast.text-1.x1.0",
            2,
            &[
                ("call_ms", How::Median, json!(30.0)),
                ("cer_pct", How::Median, json!(1.0)),
                ("stored_bytes", How::Max, json!(1900)),
                ("readable_runs", How::Sum, json!(1)),
            ],
        ),
    ];
    assert_eq!(
        lines(&aggregate(&records)),
        vec![
            "cell input.windows-2025.enigo runs=2 keys=19 clicks=5 version=0.6.1",
            "cell extract.macos-15.apple-vision-fast.text-1.x1.0 runs=2 call_ms=20.0 cer_pct=0.75 stored_bytes=1900 readable_runs=2",
        ]
    );
}

#[test]
fn a_text_that_differs_between_runs_shows_every_value() {
    let records = vec![
        record(
            "address.macos-15.chrome",
            1,
            &[("activation", How::Text, json!("unset"))],
        ),
        record(
            "address.macos-15.chrome",
            2,
            &[("activation", How::Text, json!("set"))],
        ),
        record(
            "address.macos-15.chrome",
            3,
            &[("activation", How::Text, json!("unset"))],
        ),
    ];
    assert_eq!(
        lines(&aggregate(&records)),
        vec!["cell address.macos-15.chrome runs=3 activation=unset+set"]
    );
}

#[test]
fn a_cell_with_a_reason_is_not_measurable() {
    let records = vec![
        record(
            "address.macos-15.safari",
            1,
            &[("wall_ms", How::Median, json!(3.0))],
        ),
        missing(
            "address.macos-15.safari",
            "2 of 5 runs answered: no window in 60 s",
        ),
    ];
    assert_eq!(
        lines(&aggregate(&records)),
        vec![
            "cell address.macos-15.safari not measurable: 2 of 5 runs answered: no window in 60 s"
        ]
    );
}

fn engine(runner: &str, name: &str, call_ms: f64, cer_pct: f64) -> Record {
    one(
        &format!("engine.{runner}.{name}.x1.0"),
        &[
            ("call_ms", How::Median, json!(call_ms)),
            ("cer_pct", How::Median, json!(cer_pct)),
        ],
    )
}

fn stored(cell: &str, bytes: u64) -> Record {
    one(cell, &[("stored_bytes", How::Max, json!(bytes))])
}

fn address(runner: &str, browser: &str, readable: u32) -> Vec<Record> {
    (1..=5)
        .map(|run| {
            record(
                &format!("address.{runner}.{browser}"),
                run,
                &[("readable_runs", How::Sum, json!(u32::from(run <= readable)))],
            )
        })
        .collect()
}

fn base() -> Vec<Record> {
    let mut records = vec![
        engine("macos-15", "apple-vision-accurate", 100.0, 0.5),
        engine("macos-15", "apple-vision-fast", 20.0, 3.0),
        engine("windows-2025", "windows-media-ocr", 150.0, 1.0),
        engine("macos-15", "pp-ocrv6-tiny", 160.0, 0.2),
        engine("windows-2025", "pp-ocrv6-tiny", 172.0, 0.2),
        stored("accessibility.macos-15.text-1.unset", 5000),
        stored("accessibility.windows-2025.calc", 78000),
        stored("extract.macos-15.apple-vision-accurate.text-1.x1.0", 3000),
        stored("extract.windows-2025.windows-media-ocr.calc.x0.5", 4000),
        stored("extract.macos-15.pp-ocrv6-tiny.writer.x1.0", 2000),
        stored("extract.windows-2025.pp-ocrv6-tiny.text-2.x1.0", 2500),
    ];
    for (runner, browser) in [
        ("macos-15", "chrome"),
        ("macos-15", "safari"),
        ("windows-2025", "edge"),
    ] {
        records.extend(address(runner, browser, 5));
    }
    records
}

fn replace(records: &mut Vec<Record>, new: Record) {
    records.retain(|record| record.cell != new.cell);
    records.push(new);
}

#[test]
fn every_line_holds() {
    assert_eq!(
        layer_verdicts(&aggregate(&base())),
        vec![
            "verdict engine apple-vision macos-15: holds",
            "verdict engine windows-media-ocr windows-2025: holds",
            "verdict engine pp-ocrv6-tiny macos-15: holds",
            "verdict engine pp-ocrv6-tiny windows-2025: holds",
            "verdict D3: os-pair",
            "verdict layer site: stays",
            "verdict layer accessibility_text: stays",
            "verdict layer extracted_text: stays",
        ]
    );
}

#[test]
fn an_engine_over_the_line_is_decided_by_its_second_timing_where_one_exists() {
    let mut records = base();
    replace(
        &mut records,
        engine("macos-15", "pp-ocrv6-tiny", 400.0, 0.2),
    );
    replace(
        &mut records,
        engine("windows-2025", "pp-ocrv6-tiny", 172.1, 0.2),
    );
    records.push(one(
        "engine.macos-15.pp-ocrv6-tiny.x1.0.this-mac",
        &[("call_ms", How::Median, json!(90.0))],
    ));
    let verdicts = layer_verdicts(&aggregate(&records));
    assert_eq!(
        verdicts[2],
        "verdict engine pp-ocrv6-tiny macos-15: holds re-timed"
    );
    assert_eq!(
        verdicts[3],
        "verdict engine pp-ocrv6-tiny windows-2025: over not re-timed"
    );
    replace(
        &mut records,
        one(
            "engine.macos-15.pp-ocrv6-tiny.x1.0.this-mac",
            &[("call_ms", How::Median, json!(200.0))],
        ),
    );
    assert_eq!(
        layer_verdicts(&aggregate(&records))[2],
        "verdict engine pp-ocrv6-tiny macos-15: over re-timed"
    );
    replace(
        &mut records,
        missing(
            "engine.macos-15.pp-ocrv6-tiny.x1.0.this-mac",
            "peak passed the bound",
        ),
    );
    assert_eq!(
        layer_verdicts(&aggregate(&records))[2],
        "verdict engine pp-ocrv6-tiny macos-15: over not re-timed"
    );
}

#[test]
fn apple_vision_falls_back_to_the_fast_level_when_the_accurate_level_is_over() {
    let mut records = base();
    replace(
        &mut records,
        engine("macos-15", "apple-vision-accurate", 500.0, 0.5),
    );
    let verdicts = layer_verdicts(&aggregate(&records));
    assert_eq!(verdicts[0], "verdict engine apple-vision macos-15: holds");
    assert_eq!(verdicts[4], "verdict D3: os-pair");
    replace(
        &mut records,
        engine("macos-15", "apple-vision-fast", 20.0, 3.1),
    );
    let verdicts = layer_verdicts(&aggregate(&records));
    assert_eq!(verdicts[4], "verdict D3: pp-ocrv6-tiny");
    replace(
        &mut records,
        engine("macos-15", "apple-vision-fast", 173.0, 1.0),
    );
    let verdicts = layer_verdicts(&aggregate(&records));
    assert_eq!(
        verdicts[0],
        "verdict engine apple-vision macos-15: over not re-timed"
    );
    assert_eq!(verdicts[4], "verdict D3: pp-ocrv6-tiny");
}

#[test]
fn d3_falls_to_the_one_engine_and_the_layer_follows_that_engine() {
    let mut records = base();
    replace(
        &mut records,
        engine("windows-2025", "windows-media-ocr", 150.0, 2.6),
    );
    let verdicts = layer_verdicts(&aggregate(&records));
    assert_eq!(verdicts[4], "verdict D3: pp-ocrv6-tiny");
    assert_eq!(verdicts[7], "verdict layer extracted_text: stays");
    replace(
        &mut records,
        engine("windows-2025", "pp-ocrv6-tiny", 300.0, 0.2),
    );
    let verdicts = layer_verdicts(&aggregate(&records));
    assert_eq!(verdicts[7], "verdict layer extracted_text: leaves");
}

#[test]
fn an_engine_with_no_figure_has_no_line_and_is_not_present_for_d3() {
    let mut records = base();
    replace(
        &mut records,
        missing(
            "engine.windows-2025.windows-media-ocr.x1.0",
            "no recognizer language",
        ),
    );
    replace(
        &mut records,
        missing(
            "engine.windows-2025.pp-ocrv6-tiny.x1.0",
            "the runtime did not load",
        ),
    );
    assert_eq!(
        layer_verdicts(&aggregate(&records)),
        vec![
            "verdict engine apple-vision macos-15: holds",
            "verdict engine pp-ocrv6-tiny macos-15: holds",
            "verdict D3: pp-ocrv6-tiny",
            "verdict layer site: stays",
            "verdict layer accessibility_text: stays",
            "verdict layer extracted_text: not measurable",
        ]
    );
}

#[test]
fn the_site_layer_needs_every_run_of_every_browser() {
    let mut records = base();
    records.retain(|record| record.cell != "address.windows-2025.edge");
    records.extend(address("windows-2025", "edge", 4));
    assert_eq!(
        layer_verdicts(&aggregate(&records))[5],
        "verdict layer site: leaves"
    );
    records.retain(|record| record.cell != "address.windows-2025.edge");
    records.push(missing("address.windows-2025.edge", "no window in 60 s"));
    assert_eq!(
        layer_verdicts(&aggregate(&records))[5],
        "verdict layer site: not measurable"
    );
    records.retain(|record| record.cell != "address.macos-15.safari");
    records.extend(address("macos-15", "safari", 0));
    assert_eq!(
        layer_verdicts(&aggregate(&records))[5],
        "verdict layer site: leaves"
    );
}

#[test]
fn the_byte_line_is_78000_and_reads_the_largest_cell() {
    let mut records = base();
    replace(
        &mut records,
        stored("accessibility.windows-2025.calc", 78001),
    );
    assert_eq!(
        layer_verdicts(&aggregate(&records))[6],
        "verdict layer accessibility_text: leaves"
    );
    replace(
        &mut records,
        missing("accessibility.windows-2025.calc", "no tree"),
    );
    assert_eq!(
        layer_verdicts(&aggregate(&records))[6],
        "verdict layer accessibility_text: not measurable"
    );
    let mut records = base();
    replace(
        &mut records,
        stored("extract.macos-15.apple-vision-accurate.text-1.x1.0", 90000),
    );
    assert_eq!(
        layer_verdicts(&aggregate(&records))[7],
        "verdict layer extracted_text: leaves"
    );
    let mut records = base();
    replace(
        &mut records,
        stored("extract.macos-15.pp-ocrv6-tiny.writer.x1.0", 90000),
    );
    assert_eq!(
        layer_verdicts(&aggregate(&records))[7],
        "verdict layer extracted_text: stays"
    );
}

fn driver(name: &str, keys: u32, clicks: u32) -> Record {
    one(
        &format!("input.windows-2025.{name}"),
        &[
            ("keys", How::Min, json!(keys)),
            ("clicks", How::Min, json!(clicks)),
        ],
    )
}

#[test]
fn the_driver_is_the_first_whose_25_inputs_were_all_counted() {
    let verdict = |records: Vec<Record>| input_verdicts(&aggregate(&records));
    assert_eq!(
        verdict(vec![driver("playwright", 20, 5), driver("enigo", 20, 5)]),
        vec!["verdict driver: playwright"]
    );
    assert_eq!(
        verdict(vec![driver("playwright", 0, 0), driver("enigo", 20, 5)]),
        vec!["verdict driver: enigo"]
    );
    assert_eq!(
        verdict(vec![driver("playwright", 20, 4), driver("enigo", 19, 5)]),
        vec!["verdict driver: none"]
    );
    assert_eq!(
        verdict(vec![
            missing("input.windows-2025.playwright", "no window"),
            driver("enigo", 21, 5)
        ]),
        vec!["verdict driver: none"]
    );
}

#[test]
fn private_window_lines_are_ordered_by_browser_and_then_by_runner() {
    let signal = |cell: &str, value: &str| one(cell, &[("signal", How::Text, json!(value))]);
    let records = vec![
        signal("private.windows-2025.chrome", "title"),
        signal("private.windows-2025.firefox", "class"),
        signal("private.macos-15.safari", "none"),
        signal("private.macos-15.chrome", "attribute"),
        missing("private.macos-15.edge", "a prompt ended the cell"),
        signal("private.windows-2025.edge", "none"),
        signal("private.macos-15.firefox", "title+none"),
    ];
    assert_eq!(
        private_verdicts(&aggregate(&records)),
        vec![
            "verdict private-window chrome macos-15: readable",
            "verdict private-window chrome windows-2025: readable",
            "verdict private-window edge macos-15: not measurable",
            "verdict private-window edge windows-2025: not-readable",
            "verdict private-window firefox macos-15: not-readable",
            "verdict private-window firefox windows-2025: readable",
            "verdict private-window safari macos-15: not-readable",
        ]
    );
}

#[test]
fn a_trial_file_starts_with_its_cells_and_ends_with_its_verdicts() {
    let cells = aggregate(&[driver("playwright", 0, 0), driver("enigo", 20, 5)]);
    let hosted = vec![HostedRun {
        branch: "scratch/r2-probes-1".to_string(),
        commit: "0123456789abcdef0123456789abcdef01234567".to_string(),
        run: "42".to_string(),
        attempt: "1".to_string(),
    }];
    let text = render(
        &cells,
        &hosted,
        &["crate enigo 0.6.1 MIT".to_string()],
        &input_verdicts(&cells),
    );
    assert_eq!(
        text,
        "cells: input.windows-2025.playwright input.windows-2025.enigo\n\
         hosted-run branch=scratch/r2-probes-1 commit=0123456789abcdef0123456789abcdef01234567 run=42 attempt=1\n\
         cell input.windows-2025.playwright runs=1 keys=0 clicks=0\n\
         cell input.windows-2025.enigo runs=1 keys=20 clicks=5\n\
         crate enigo 0.6.1 MIT\n\
         verdict driver: enigo\n"
    );
}
