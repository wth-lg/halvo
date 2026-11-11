use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

const ATTEMPT: &str = "attempt model=m_a machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6";
const VERDICT: &str = "verdict driver=none path=B writer=none";
const BOUND: &str = "bound model=m_a machine=mac bound_bytes=100 fits=yes";
const ROW: &str = "bill\tutility_bill_payment\tcompile\ton_device\tm_a\tmac\t1\tdev\t1\tyes\t3\t10\t5\t6\tmedium\ts1";
const MACHINES: [&str; 2] = ["mac", "hosted-win"];

fn driver_lines(model: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for machine in MACHINES {
        for task in ["bill", "order"] {
            for seed in 1..=3 {
                for (kind, i) in [
                    ("compile", 1),
                    ("replay", 1),
                    ("replay", 2),
                    ("replay", 3),
                    ("recover", 1),
                ] {
                    lines.push(format!("attempt model={model} machine={machine} task={task} seed={seed} kind={kind} i={i} pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6"));
                }
                lines.push(format!("drive model={model} machine={machine} task={task} seed={seed} compile=ok replays_ok=3 of=3 recover=ok steps=3 wall_s=10 model_tokens=11 typed_credential=0 effects_without_confirm=0"));
            }
        }
    }
    lines
}

fn writer_lines(model: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for machine in MACHINES {
        for seed in 1..=3 {
            lines.push(format!("attempt model={model} machine={machine} task=reply seed={seed} kind=write i=1 pass=yes steps=0 wall_s=10 tokens_in=5 tokens_out=6"));
            lines.push(format!(
                "reply model={model} machine={machine} seed={seed} bodies=2 of=2 wall_s=10 model_tokens=11"
            ));
        }
    }
    lines
}

fn with(mut lines: Vec<String>, last: &str) -> Vec<String> {
    lines.push(last.to_owned());
    lines
}

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    loop {
        let dir = base.join(format!(
            "route-gate-import-rules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("utf-8")
        })
        .collect();
    names.sort();
    names
}

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    dir: PathBuf,
    out: PathBuf,
}

impl Run {
    fn refused(&self) -> String {
        assert_eq!(self.code, Some(2), "stderr {:?}", self.stderr);
        assert_eq!(self.stdout, "");
        assert_eq!(
            names(&self.out),
            Vec::<String>::new(),
            "a refusal wrote a file"
        );
        let line = self.stderr.strip_suffix('\n').expect("one line");
        assert!(!line.contains('\n'), "one line: {line:?}");
        line.to_owned()
    }

    fn file(&self, name: &str) -> String {
        std::fs::read_to_string(self.out.join(name)).expect("written")
    }

    fn passed(&self) -> &Run {
        assert_eq!(self.code, Some(0), "stderr {:?}", self.stderr);
        assert_eq!(self.stderr, "");
        self
    }
}

fn block(lines: &[&str]) -> Vec<u8> {
    let mut text = String::from("```\n");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("```\n");
    text.into_bytes()
}

fn file(heading: &str, body: Vec<u8>) -> Vec<u8> {
    let mut text = format!("# {heading}\n").into_bytes();
    text.extend(body);
    text
}

fn run_with(driver: Vec<u8>, cells: Vec<u8>, args: &[&str]) -> Run {
    let dir = scratch();
    let out = dir.join("out");
    std::fs::create_dir(&out).expect("out");
    std::fs::write(dir.join("driver.md"), driver).expect("driver");
    std::fs::write(dir.join("model-cells.md"), cells).expect("cells");
    let output = Command::new(env!("CARGO_BIN_EXE_route-gate"))
        .current_dir(&dir)
        .args(args)
        .output()
        .expect("the tool runs");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).expect("utf-8"),
        stderr: String::from_utf8(output.stderr).expect("utf-8"),
        dir,
        out,
    }
}

const ARGS: [&str; 11] = [
    "import",
    "--driver",
    "driver.md",
    "--cells",
    "model-cells.md",
    "--date",
    "s1=2026-11-10",
    "--date",
    "s2=2026-11-10",
    "--out",
    "out",
];

fn run_files(driver: Vec<u8>, cells: Vec<u8>) -> Run {
    run_with(driver, cells, &ARGS)
}

fn run(driver: &[&str], cells: &[&str]) -> Run {
    run_files(file("driver", block(driver)), file("cells", block(cells)))
}

fn refs(lines: &[String]) -> Vec<&str> {
    lines.iter().map(String::as_str).collect()
}

fn attempt_with(key: &str, value: &str) -> String {
    ATTEMPT
        .split(' ')
        .map(|word| match word.split_once('=') {
            Some((k, _)) if k == key => format!("{k}={value}"),
            _ => word.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_good_pair_writes_the_four_files_and_prints_the_line() {
    let got = run(&[ATTEMPT, VERDICT], &[BOUND]);
    got.passed();
    assert_eq!(got.stdout, "import rows=1 models=1 sources=2\n");
    assert_eq!(
        names(&got.out),
        ["freeze.txt", "models.tsv", "recorded.tsv", "sources.tsv"]
    );
    assert_eq!(
        got.file("recorded.tsv"),
        format!("{}\n{ROW}\n", halvo_route_gate::table::HEADER)
    );
    assert_eq!(
        got.file("models.tsv"),
        "model\tmachine\tbound_bytes\tfits\tgroup\tsource\nm_a\tmac\t100\tyes\tagents\ts2\n"
    );
    assert_eq!(
        got.file("freeze.txt"),
        "freeze rows=1 models=1 sources=2 date=2026-11-10\non_device driver=none writer=none\n"
    );
    let driver_blob = halvo_route_gate::import::blob_id(
        &std::fs::read(got.dir.join("driver.md")).expect("driver"),
    );
    let cells_blob = halvo_route_gate::import::blob_id(
        &std::fs::read(got.dir.join("model-cells.md")).expect("cells"),
    );
    assert_eq!(
        got.file("sources.tsv"),
        format!(
            "id\tpath\tblob\tdate\tmeasures\tlimits\n\
             s1\tdriver.md\t{driver_blob}\t2026-11-10\tdriver_trial_attempts\t3_seeds_2_machines_team_fixtures\n\
             s2\tmodel-cells.md\t{cells_blob}\t2026-11-10\tfootprint_bounds\t5_runs_a_cell_no_16gb_laptop\n"
        )
    );
}

#[test]
fn the_freeze_date_is_the_later_one_and_each_source_keeps_its_own() {
    let got = run_with(
        file("driver", block(&[ATTEMPT, VERDICT])),
        file("cells", block(&[BOUND])),
        &[
            "import",
            "--date",
            "s2=2026-12-01",
            "--out",
            "out",
            "--cells",
            "./model-cells.md",
            "--date",
            "s1=2026-11-30",
            "--driver",
            "driver.md",
        ],
    );
    got.passed();
    assert!(got
        .file("freeze.txt")
        .starts_with("freeze rows=1 models=1 sources=2 date=2026-12-01\n"));
    let sources = got.file("sources.tsv");
    let rows: Vec<Vec<&str>> = sources
        .lines()
        .skip(1)
        .map(|l| l.split('\t').collect())
        .collect();
    assert_eq!(rows[0][..2], ["s1", "driver.md"]);
    assert_eq!(rows[0][3], "2026-11-30");
    assert_eq!(rows[1][..2], ["s2", "./model-cells.md"]);
    assert_eq!(rows[1][3], "2026-12-01");
}

#[test]
fn rows_and_models_stand_in_byte_order_and_routing_tokens_get_their_group() {
    let got = run(
        &[
            "attempt model=m_b machine=mac task=reply seed=3 kind=write i=1 pass=no steps=0 wall_s=7 tokens_in=1 tokens_out=2",
            ATTEMPT,
            "attempt model=m_a machine=hosted-win task=order seed=2 kind=replay i=1 pass=yes steps=4 wall_s=0 tokens_in=0 tokens_out=0",
            "attempt model=m_a machine=mac task=order seed=1 kind=recover i=1 pass=yes steps=18446744073709551615 wall_s=1 tokens_in=1 tokens_out=1",
            VERDICT,
        ],
        &[
            "bound model=ternary_bonsai_2_9b machine=mac bound_bytes=0 fits=not-measured",
            "bound model=s5_nano_4b machine=hosted-win bound_bytes=18446744073709551615 fits=no",
            BOUND,
            "bound model=gemma_4_e4b machine=mac bound_bytes=1 fits=yes",
            "bound model=gemma_4_e2b machine=mac bound_bytes=2 fits=yes",
            "bound model=gemma_4_e2 machine=mac bound_bytes=3 fits=yes",
        ],
    );
    got.passed();
    assert_eq!(got.stdout, "import rows=4 models=6 sources=2\n");
    let recorded = got.file("recorded.tsv");
    let lines: Vec<&str> = recorded.lines().skip(1).collect();
    assert_eq!(
        lines,
        [
            ROW,
            "order\tshop_order\trecover\ton_device\tm_a\tmac\t1\tdev\t1\tyes\t18446744073709551615\t1\t1\t1\tmedium\ts1",
            "order\tshop_order\treplay\ton_device\tm_a\thosted-win\t2\tdev\t1\tyes\t4\t0\t0\t0\tmedium\ts1",
            "reply\temail\twrite\ton_device\tm_b\tmac\t3\theld\t1\tno\t0\t7\t1\t2\tmedium\ts1",
        ]
    );
    assert_eq!(
        got.file("models.tsv"),
        "model\tmachine\tbound_bytes\tfits\tgroup\tsource\n\
         gemma_4_e2\tmac\t3\tyes\tagents\ts2\n\
         gemma_4_e2b\tmac\t2\tyes\trouting\ts2\n\
         gemma_4_e4b\tmac\t1\tyes\trouting\ts2\n\
         m_a\tmac\t100\tyes\tagents\ts2\n\
         s5_nano_4b\thosted-win\t18446744073709551615\tno\trouting\ts2\n\
         ternary_bonsai_2_9b\tmac\t0\tnot-measured\trouting\ts2\n"
    );
    assert!(got
        .file("freeze.txt")
        .ends_with("\non_device driver=none writer=none\n"));
}

#[test]
fn a_block_with_no_known_line_but_the_verdict_imports_empty_tables() {
    let got = run(&["verdict driver=none path=B writer=none"], &[]);
    got.passed();
    assert_eq!(got.stdout, "import rows=0 models=0 sources=2\n");
    assert_eq!(
        got.file("recorded.tsv"),
        format!("{}\n", halvo_route_gate::table::HEADER)
    );
    assert_eq!(
        got.file("models.tsv"),
        "model\tmachine\tbound_bytes\tfits\tgroup\tsource\n"
    );
    assert_eq!(
        got.file("freeze.txt"),
        "freeze rows=0 models=0 sources=2 date=2026-11-10\non_device driver=none writer=none\n"
    );
}

#[test]
fn lines_that_are_not_known_are_passed_over() {
    let passed_over = [
        "",
        "drive model=m_a machine=mac task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=1 wall_s=1 model_tokens=1 typed_credential=0 effects_without_confirm=0",
        "reply model=m_a machine=mac seed=1 bodies=2 of=2 wall_s=1 model_tokens=1",
        "limits task=bill steps=1 wall_s=1 model_tokens=1",
        "prompt file=action.txt sha256=00",
        "cell model=m_a machine=mac",
        "hosted machine=hosted-win run=1 sha=00",
        "notrun model=m_a cause=browser_start",
        " attempt model=m_a machine=mac task=bill seed=1 kind=compile i=1 pass=nonsense",
        "\tattempt model=x",
        "Attempt model=m_a",
        " verdict driver=x",
        "Verdict candidates=x",
        " bound model=m_a",
        "Bound model=m_a",
        "``",
        "\u{e9}t\u{e9} attempt model=",
    ];
    let mut driver = vec![ATTEMPT];
    driver.extend(passed_over);
    driver.push(VERDICT);
    let mut cells = passed_over.to_vec();
    cells.insert(5, BOUND);
    let got = run(&driver, &cells);
    got.passed();
    assert_eq!(got.stdout, "import rows=1 models=1 sources=2\n");
}

#[test]
fn a_known_line_of_one_file_is_refused_in_the_other() {
    let got = run(&[ATTEMPT, BOUND, VERDICT], &[BOUND]);
    assert_eq!(got.refused(), "REFUSED source: driver:4");
    let got = run(&[ATTEMPT, VERDICT], &[ATTEMPT, BOUND]);
    assert_eq!(got.refused(), "REFUSED source: cells:3");
    let got = run(&[ATTEMPT, VERDICT], &[BOUND, VERDICT]);
    assert_eq!(got.refused(), "REFUSED source: cells:4");
}

#[test]
fn a_line_that_starts_with_a_known_word_is_of_its_form_or_refused() {
    let attempt_tab = ATTEMPT.replacen("attempt ", "attempt\t", 1);
    let bound_tab = BOUND.replacen("bound ", "bound\t", 1);
    let driver_cases: Vec<(&str, &str)> = vec![
        ("attempt", "attempt"),
        ("attempt", "attempts model=m_a"),
        ("attempt", "attempt_model=m_a"),
        ("attempt", &attempt_tab),
        ("attempt", "attempted twice"),
        ("verdict", "verdict"),
        ("verdict", "verdict driver"),
        ("verdict", "verdict  driver=m_a path=A writer=m_a"),
        ("verdict", "verdict writer=m_a"),
        ("verdict", "verdict candidates=m_a"),
        ("verdict", "verdict candidates=-"),
        ("verdict", "verdicts driver=m_a path=A writer=m_a"),
        ("bound", BOUND),
        ("bound", "bound"),
        ("bound", "bounds model=m_a"),
    ];
    let cells_cases: Vec<(&str, &str)> = vec![
        ("bound", "bound"),
        ("bound", "bounds model=m_a"),
        ("bound", &bound_tab),
        ("bound", "bound_bytes=1"),
        ("verdict", VERDICT),
        ("verdict", "verdict"),
        ("verdict", "verdict candidates"),
        ("verdict", "verdict  candidates=-"),
        ("verdict", "verdict\tcandidates=-"),
        ("verdict", "verdict Candidates=-"),
        ("verdict", "verdicts candidates=-"),
        ("attempt", ATTEMPT),
        ("attempt", "attempt"),
        ("attempt", "attempts model=m_a"),
    ];
    let mut wrong = Vec::new();
    for (word, line) in &driver_cases {
        let got = run(&[ATTEMPT, line, VERDICT], &[BOUND]);
        if got.code != Some(2) || got.stderr != "REFUSED source: driver:4\n" {
            wrong.push(format!(
                "driver {word} {line:?}: {:?} {:?}",
                got.code, got.stderr
            ));
        } else {
            got.refused();
        }
    }
    for (word, line) in &cells_cases {
        let got = run(&[ATTEMPT, VERDICT], &[BOUND, line]);
        if got.code != Some(2) || got.stderr != "REFUSED source: cells:4\n" {
            wrong.push(format!(
                "cells {word} {line:?}: {:?} {:?}",
                got.code, got.stderr
            ));
        } else {
            got.refused();
        }
    }
    assert_eq!(wrong, Vec::<String>::new());
    for line in [
        "verdict candidates=-",
        "verdict candidates=m_a",
        "verdict candidates=m_a,m_b,qwen3_5_2b",
        "verdict candidates=",
        "verdict candidates=M,,x y",
    ] {
        run(&[ATTEMPT, VERDICT], &[BOUND, line]).passed();
    }
}

#[test]
fn two_swapped_trial_files_and_the_driver_file_given_twice_are_refused() {
    let full = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/r3/full");
    for (driver, cells, want) in [
        ("model-cells.md", "driver.md", "REFUSED source: driver:16\n"),
        ("driver.md", "driver.md", "REFUSED source: cells:7\n"),
    ] {
        let out = scratch();
        let output = Command::new(env!("CARGO_BIN_EXE_route-gate"))
            .current_dir(&full)
            .args([
                "import",
                "--driver",
                driver,
                "--cells",
                cells,
                "--date",
                "s1=2026-11-10",
                "--date",
                "s2=2026-11-10",
                "--out",
                out.to_str().expect("utf-8 path"),
            ])
            .output()
            .expect("the tool runs");
        assert_eq!(
            String::from_utf8(output.stderr).expect("utf-8"),
            want,
            "--driver {driver} --cells {cells}"
        );
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(output.stdout, b"");
        assert_eq!(names(&out), Vec::<String>::new());
    }
}

#[test]
fn lines_outside_the_block_are_not_read() {
    let mut driver =
        b"# driver\nattempt model=bad\nverdict driver=m_b path=A writer=m_b\n".to_vec();
    driver.extend(block(&[ATTEMPT, VERDICT]));
    driver.extend(b"attempt model=bad\nverdict driver=x\n\xff\n");
    let mut cells = b"bound model=BAD\n".to_vec();
    cells.extend(block(&[BOUND]));
    cells.extend(b"text after the block\n");
    let got = run_files(driver, cells);
    got.passed();
    assert!(got
        .file("freeze.txt")
        .ends_with("driver=none writer=none\n"));
}

#[test]
fn the_fence_opens_with_three_backticks_and_one_word_of_letters_and_closes_with_three() {
    for opener in ["```", "```text", "```sh", "```Text", "```x"] {
        let driver = format!("# d\n{opener}\n{ATTEMPT}\n{VERDICT}\n```\n").into_bytes();
        let got = run_files(driver, file("cells", block(&[BOUND])));
        got.passed();
    }
    let got = run_files(
        format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n```").into_bytes(),
        format!("```\n{BOUND}\n```").into_bytes(),
    );
    got.passed();
}

#[test]
fn a_file_with_no_block_or_two_is_refused() {
    let good_cells = || file("cells", block(&[BOUND]));
    let good_driver = || file("driver", block(&[ATTEMPT, VERDICT]));
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "no fence",
            format!("# d\n{ATTEMPT}\n{VERDICT}\n").into_bytes(),
        ),
        ("empty file", Vec::new()),
        (
            "not closed",
            format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n").into_bytes(),
        ),
        (
            "closed by a word",
            format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n```text\n").into_bytes(),
        ),
        (
            "four backticks",
            format!("# d\n````\n{ATTEMPT}\n{VERDICT}\n````\n").into_bytes(),
        ),
        (
            "two backticks",
            format!("# d\n``\n{ATTEMPT}\n{VERDICT}\n``\n").into_bytes(),
        ),
        (
            "tilde fence",
            format!("# d\n~~~\n{ATTEMPT}\n{VERDICT}\n~~~\n").into_bytes(),
        ),
        (
            "indented fence",
            format!("# d\n  ```\n{ATTEMPT}\n{VERDICT}\n  ```\n").into_bytes(),
        ),
        (
            "word with a dash",
            format!("# d\n```sh-session\n{ATTEMPT}\n{VERDICT}\n```\n").into_bytes(),
        ),
        (
            "word with a digit",
            format!("# d\n```text2\n{ATTEMPT}\n{VERDICT}\n```\n").into_bytes(),
        ),
        (
            "space before the word",
            format!("# d\n``` text\n{ATTEMPT}\n{VERDICT}\n```\n").into_bytes(),
        ),
        (
            "two words",
            format!("# d\n```text sh\n{ATTEMPT}\n{VERDICT}\n```\n").into_bytes(),
        ),
        (
            "closer with a space",
            format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n``` \n").into_bytes(),
        ),
        (
            "two blocks",
            format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n```\n```\n```\n").into_bytes(),
        ),
        (
            "two blocks, the second with a word",
            format!("# d\n```\n{ATTEMPT}\n```\n```text\n{VERDICT}\n```\n").into_bytes(),
        ),
        (
            "a second block not closed",
            format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n```\n```\n").into_bytes(),
        ),
    ];
    for (what, bytes) in cases {
        let got = run_files(bytes.clone(), good_cells());
        assert_eq!(
            got.refused(),
            "REFUSED source: driver:block",
            "driver: {what}"
        );
        let cells = String::from_utf8(bytes)
            .expect("utf-8")
            .replace(ATTEMPT, BOUND)
            .replace(VERDICT, "verdict candidates=-")
            .into_bytes();
        let got = run_files(good_driver(), cells);
        assert_eq!(
            got.refused(),
            "REFUSED source: cells:block",
            "cells: {what}"
        );
    }
}

#[test]
fn a_fence_line_of_another_form_or_a_third_fence_line_is_refused() {
    let cases: Vec<(&str, String)> = vec![
        (
            "an indented opening fence",
            format!("# d\n  ```\n{ATTEMPT}\n{VERDICT}\n```\n"),
        ),
        (
            "an indented opening fence before a fence with a word",
            format!("# d\n  ```\nnote\n```text\n{ATTEMPT}\n{VERDICT}\n```\n"),
        ),
        (
            "a space before the word",
            format!("# d\n``` text\n{ATTEMPT}\n{VERDICT}\n```\n"),
        ),
        (
            "a space before the word, before a fence with a word",
            format!("# d\n``` text\nnote\n```text\n{ATTEMPT}\n{VERDICT}\n```\n"),
        ),
        (
            "a word with a letter outside ASCII",
            format!("# d\n```\u{e9}t\u{e9}\n{ATTEMPT}\n{VERDICT}\n```\n"),
        ),
        (
            "a word with a letter outside ASCII, before a fence with a word",
            format!("# d\n```\u{e9}t\u{e9}\nnote\n```text\n{ATTEMPT}\n{VERDICT}\n```\n"),
        ),
        (
            "four backticks",
            format!("# d\n````\n{ATTEMPT}\n{VERDICT}\n```\n"),
        ),
        (
            "four backticks inside the block",
            format!("# d\n```\n{ATTEMPT}\n````\n{VERDICT}\n```\n"),
        ),
        (
            "a fence line with a word inside the block",
            format!("# d\n```\n{ATTEMPT}\n```text\n{VERDICT}\n```\n"),
        ),
        (
            "an indented fence line inside the block",
            format!("# d\n```\n{ATTEMPT}\n  ```\n{VERDICT}\n```\n"),
        ),
        (
            "a fence line with a space after it inside the block",
            format!("# d\n```\n{ATTEMPT}\n``` \n{VERDICT}\n```\n"),
        ),
        (
            "a second block that closes",
            format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n```\n```\nx\n```\n"),
        ),
        (
            "a second block that does not close",
            format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n```\n```text\nx\n"),
        ),
        (
            "text after the closing fence's backticks",
            format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n```x\n"),
        ),
        (
            "text after the backticks of a fence before the closing one",
            format!("# d\n```\n{ATTEMPT}\n{VERDICT}\n```x\nnote\n```\n"),
        ),
    ];
    let mut wrong = Vec::new();
    for (what, text) in &cases {
        let got = run_files(text.clone().into_bytes(), file("cells", block(&[BOUND])));
        if got.code != Some(2) || got.stderr != "REFUSED source: driver:block\n" {
            wrong.push(format!("driver, {what}: {:?} {:?}", got.code, got.stderr));
        } else {
            got.refused();
        }
        let cells = text
            .replace(ATTEMPT, BOUND)
            .replace(VERDICT, "verdict candidates=-");
        let got = run_files(
            file("driver", block(&[ATTEMPT, VERDICT])),
            cells.into_bytes(),
        );
        if got.code != Some(2) || got.stderr != "REFUSED source: cells:block\n" {
            wrong.push(format!("cells, {what}: {:?} {:?}", got.code, got.stderr));
        } else {
            got.refused();
        }
    }
    assert_eq!(wrong, Vec::<String>::new());
    assert_eq!(cases.len(), 15);
}

#[test]
fn a_carriage_return_is_refused_with_its_line() {
    let got = run_files(
        format!("# d\r\n```\n{ATTEMPT}\n{VERDICT}\n```\n").into_bytes(),
        file("cells", block(&[BOUND])),
    );
    assert_eq!(got.refused(), "REFUSED source: driver:1");
    let got = run_files(
        format!("# d\n```\n{ATTEMPT}\r\n{VERDICT}\n```\n").into_bytes(),
        file("cells", block(&[BOUND])),
    );
    assert_eq!(got.refused(), "REFUSED source: driver:3");
    let got = run_files(
        file("driver", block(&[ATTEMPT, VERDICT])),
        format!("# c\n```\nverdict candidates=-\n{BOUND}\n```\ntext\rmore\n").into_bytes(),
    );
    assert_eq!(got.refused(), "REFUSED source: cells:6");
    let got = run_files(
        format!("# d\n```\r\n{ATTEMPT}\n{VERDICT}\n```\r\n").into_bytes(),
        file("cells", block(&[BOUND])),
    );
    assert_eq!(got.refused(), "REFUSED source: driver:2");
}

#[test]
fn each_field_of_an_attempt_line_with_a_wrong_value_is_refused() {
    let wrong: &[(&str, &[&str])] = &[
        (
            "model",
            &[
                "",
                "M",
                "m-a",
                "m a",
                "synthetic",
                "none",
                "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
                "m\u{e9}",
            ],
        ),
        (
            "machine",
            &[
                "",
                "hosted-mac",
                "gen_a",
                "gen_b",
                "Mac",
                "hosted_win",
                "win",
            ],
        ),
        ("task", &["", "gen", "Bill", "pay", "bill,order"]),
        ("seed", &["", "0", "4", "01", "1.0", "-1", "+1"]),
        ("kind", &["", "Compile", "plan", "compile,replay"]),
        (
            "i",
            &["", "0", "01", "4294967296", "-1", "+1", "1e3", "\u{661}"],
        ),
        ("pass", &["", "Yes", "y", "true", "ok", "1"]),
        (
            "steps",
            &["", "01", "-1", "1.5", "18446744073709551616", "+1", " "],
        ),
        ("wall_s", &["", "00", "-0", "18446744073709551616", "x"]),
        ("tokens_in", &["", "007", "18446744073709551616", "1_000"]),
        ("tokens_out", &["", "01", "18446744073709551616", "-"]),
    ];
    let mut cases = 0;
    for (key, values) in wrong {
        for value in *values {
            let line = attempt_with(key, value);
            assert_ne!(line, ATTEMPT);
            let got = run(&[VERDICT, &line], &[BOUND]);
            assert_eq!(got.refused(), "REFUSED source: driver:4", "{key}={value:?}");
            cases += 1;
        }
    }
    assert_eq!(cases, 65);
}

#[test]
fn a_task_and_kind_other_than_the_seven_pairs_of_the_trial_is_refused() {
    let taken = [
        ("bill", "compile"),
        ("bill", "replay"),
        ("bill", "recover"),
        ("order", "compile"),
        ("order", "replay"),
        ("order", "recover"),
        ("reply", "write"),
    ];
    let mut wrong = Vec::new();
    let mut refused = 0;
    for task in ["bill", "order", "reply", "cleanup", "fill"] {
        for kind in ["compile", "replay", "recover", "write"] {
            let line = attempt_with("task", task);
            let line = line.replace("kind=compile", &format!("kind={kind}"));
            let got = run(&[&line, VERDICT], &[BOUND]);
            if taken.contains(&(task, kind)) {
                got.passed();
            } else if got.code != Some(2) || got.stderr != "REFUSED source: driver:3\n" {
                wrong.push(format!("{task} {kind}: {:?} {:?}", got.code, got.stderr));
            } else {
                got.refused();
                refused += 1;
            }
        }
    }
    assert_eq!(wrong, Vec::<String>::new());
    assert_eq!(refused, 13);
}

#[test]
fn an_attempt_line_of_another_shape_is_refused() {
    let shapes = [
        ATTEMPT.replace("model=", "modell="),
        ATTEMPT.replace(" tokens_out=6", ""),
        format!("{ATTEMPT} extra=1"),
        format!("{ATTEMPT} "),
        ATTEMPT.replace("seed=1 ", "seed=1  "),
        ATTEMPT.replace(" i=1", "\ti=1"),
        ATTEMPT.replace("machine=mac task=bill", "task=bill machine=mac"),
        ATTEMPT.replace("model=m_a", "model=m_a=b"),
        ATTEMPT.replace("model=m_a", "model"),
        ATTEMPT.replace("pass=yes", "pass=yes pass=yes"),
        ATTEMPT.replace("tokens_out=6", "tokens_out=6\u{a0}"),
        "attempt ".to_owned(),
        "attempt model=m_a".to_owned(),
        "attempt  model=m_a machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6".to_owned(),
    ];
    for line in &shapes {
        let got = run(&[ATTEMPT, line, VERDICT], &[BOUND]);
        assert_eq!(got.refused(), "REFUSED source: driver:4", "{line:?}");
    }
    let mut bytes = file("driver", Vec::new());
    bytes.extend(b"```\n");
    bytes.extend(b"attempt model=m_\xff machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6\n");
    bytes.extend(format!("{VERDICT}\n```\n").as_bytes());
    let got = run_files(bytes, file("cells", block(&[BOUND])));
    assert_eq!(got.refused(), "REFUSED source: driver:3");
}

#[test]
fn a_second_attempt_line_of_one_row_key_is_refused_at_its_line() {
    let same_key_other_figures = ATTEMPT.replace("pass=yes steps=3", "pass=no steps=4");
    let got = run(&[ATTEMPT, VERDICT, &same_key_other_figures], &[BOUND]);
    assert_eq!(got.refused(), "REFUSED source: driver:5");
    let got = run(&[ATTEMPT, ATTEMPT, VERDICT], &[BOUND]);
    assert_eq!(got.refused(), "REFUSED source: driver:4");
    for (key, value) in [
        ("model", "m_b"),
        ("machine", "hosted-win"),
        ("task", "order"),
        ("seed", "2"),
        ("kind", "recover"),
    ] {
        let other = attempt_with(key, value);
        run(&[ATTEMPT, &other, VERDICT], &[BOUND]).passed();
    }
    let replay = attempt_with("kind", "replay");
    let second = replay.replace(" i=1 ", " i=2 ");
    run(&[&replay, &second, VERDICT], &[BOUND]).passed();
}

fn line_of(model: &str, task: &str, seed: u32, kind: &str, i: &str) -> String {
    format!("attempt model={model} machine=mac task={task} seed={seed} kind={kind} i={i} pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6")
}

#[test]
fn an_i_outside_the_numbering_is_refused_at_its_line() {
    for (task, kind, i) in [
        ("bill", "compile", "2"),
        ("order", "recover", "2"),
        ("reply", "write", "2"),
        ("bill", "replay", "4"),
        ("order", "replay", "7"),
    ] {
        let line = line_of("m_b", task, 1, kind, i);
        let got = run(&[ATTEMPT, &line, VERDICT], &[BOUND]);
        assert_eq!(got.refused(), "REFUSED source: driver:4", "{line}");
    }
    for further in [
        line_of("m_d", "bill", 1, "replay", "4"),
        line_of("m_d", "order", 2, "compile", "2"),
    ] {
        let mut driver = vec![further.clone()];
        driver.extend(driver_lines("m_d"));
        let got = run(
            &refs(&with(driver, "verdict driver=m_d path=A writer=none")),
            &[BOUND],
        );
        assert_eq!(got.refused(), "REFUSED source: driver:3", "{further}");
    }
    let lines = driver_lines("m_d");
    let twice = with(lines.clone(), &lines[7]);
    let got = run(
        &refs(&with(twice, "verdict driver=m_d path=A writer=none")),
        &[BOUND],
    );
    assert_eq!(got.refused(), "REFUSED source: driver:75");
}

#[test]
fn a_gap_in_the_replays_is_refused_at_the_first_replay_line_whose_i_less_one_has_no_line() {
    let replay = |seed, i| line_of("m_b", "bill", seed, "replay", i);
    for (lines, want) in [
        (vec![replay(1, "1"), replay(1, "3")], "driver:4"),
        (vec![replay(1, "3"), replay(1, "2")], "driver:4"),
        (
            vec![replay(1, "2"), replay(2, "1"), replay(2, "3")],
            "driver:3",
        ),
        (
            vec![replay(2, "1"), replay(2, "3"), replay(1, "2")],
            "driver:4",
        ),
        (vec![ATTEMPT.to_owned(), replay(3, "2")], "driver:4"),
    ] {
        let got = run(&refs(&with(lines.clone(), VERDICT)), &[BOUND]);
        assert_eq!(
            got.refused(),
            format!("REFUSED source: {want}"),
            "{lines:?}"
        );
    }
    let got = run(
        &refs(&with(
            vec![replay(1, "3"), replay(1, "1"), replay(1, "2")],
            VERDICT,
        )),
        &[BOUND],
    );
    got.passed();
    assert_eq!(got.stdout, "import rows=3 models=1 sources=2\n");
}

#[test]
fn the_verdicts_driver_and_writer_have_exactly_the_attempt_lines_of_the_trial() {
    let verdict = "verdict driver=m_d path=A writer=m_d";
    let lines = [driver_lines("m_d"), writer_lines("m_d")].concat();
    let got = run(&refs(&with(lines.clone(), verdict)), &[BOUND]);
    got.passed();
    assert_eq!(got.stdout, "import rows=66 models=1 sources=2\n");
    let mut dropped = 0;
    for (n, line) in lines.iter().enumerate() {
        if !line.starts_with("attempt ") {
            continue;
        }
        let mut fewer = lines.clone();
        fewer.remove(n);
        let got = run(&refs(&with(fewer, verdict)), &[BOUND]);
        let want = if line.contains(" kind=replay i=1 ") || line.contains(" kind=replay i=2 ") {
            format!("REFUSED source: driver:{}", n + 3)
        } else {
            "REFUSED source: driver:verdict".to_owned()
        };
        assert_eq!(got.refused(), want, "{line}");
        dropped += 1;
    }
    assert_eq!(dropped, 66);
    for verdict in [
        "verdict driver=m_x path=A writer=none",
        "verdict driver=none path=B writer=m_x",
    ] {
        let got = run(&refs(&with(lines.clone(), verdict)), &[BOUND]);
        assert_eq!(got.refused(), "REFUSED source: driver:verdict", "{verdict}");
    }
    let mut writer_with_compile = writer_lines("m_w");
    writer_with_compile.push(line_of("m_w", "bill", 1, "compile", "1"));
    for (lines, verdict) in [
        (
            [driver_lines("m_d"), writer_lines("m_d")].concat(),
            "verdict driver=m_d path=A writer=none",
        ),
        (writer_with_compile, "verdict driver=none path=B writer=m_w"),
    ] {
        run(&refs(&with(lines, verdict)), &[BOUND]).passed();
    }
}

#[test]
fn a_model_that_is_neither_driver_nor_writer_may_have_compile_lines_alone() {
    let mut lines = driver_lines("m_d");
    for task in ["bill", "order"] {
        for seed in 1..=3 {
            lines.push(line_of("m_n", task, seed, "compile", "1"));
        }
    }
    let got = run(
        &refs(&with(lines, "verdict driver=m_d path=A writer=none")),
        &[BOUND],
    );
    got.passed();
    assert_eq!(got.stdout, "import rows=66 models=1 sources=2\n");
}

#[test]
fn the_verdict_line_is_required_once() {
    let got = run(&[ATTEMPT], &[BOUND]);
    assert_eq!(got.refused(), "REFUSED source: driver:verdict");
    let got = run(&[VERDICT, ATTEMPT, VERDICT], &[BOUND]);
    assert_eq!(got.refused(), "REFUSED source: driver:verdict");
    let got = run(
        &["verdict driver=m_a path=A writer=m_a", ATTEMPT, VERDICT],
        &[BOUND],
    );
    assert_eq!(got.refused(), "REFUSED source: driver:verdict");
    let got = run(&[ATTEMPT, VERDICT], &[BOUND, VERDICT]);
    assert_eq!(got.refused(), "REFUSED source: cells:4");
    let mut driver = file("driver", block(&[ATTEMPT]));
    driver.extend(format!("{VERDICT}\n").as_bytes());
    let got = run_files(driver, file("cells", block(&[BOUND])));
    assert_eq!(got.refused(), "REFUSED source: driver:verdict");
}

#[test]
fn each_field_of_the_verdict_line_with_a_wrong_value_is_refused() {
    let lines = [
        "verdict driver= path=A writer=m_a",
        "verdict driver=M path=A writer=m_a",
        "verdict driver=synthetic path=A writer=synthetic",
        "verdict driver=m_a path=C writer=m_a",
        "verdict driver=m_a path=a writer=m_a",
        "verdict driver=m_a path= writer=m_a",
        "verdict driver=m_a path=A writer=",
        "verdict driver=m_a path=A writer=M",
        "verdict driver=m_a path=A writer=-",
        "verdict driver=m_a writer=m_a path=A",
        "verdict driver=m_a path=A writer=m_a extra=1",
        "verdict driver=m_a path=A",
        "verdict driver=m_a path=A writer=m_a ",
        "verdict driver=m_a  path=A writer=m_a",
        "verdict driver=none path=A writer=none",
        "verdict driver=m_a path=B writer=m_a",
        "verdict driver=m_a path=A writer=m_b",
        "verdict driver=",
    ];
    for line in lines {
        let got = run(&[ATTEMPT, line], &[BOUND]);
        assert_eq!(got.refused(), "REFUSED source: driver:4", "{line:?}");
    }
    for (line, lines, rows, freeze) in [
        (
            "verdict driver=m_d path=A writer=none",
            driver_lines("m_d"),
            61,
            "driver=m_d writer=none",
        ),
        (
            "verdict driver=m_d path=A writer=m_d",
            [driver_lines("m_d"), writer_lines("m_d")].concat(),
            67,
            "driver=m_d writer=m_d",
        ),
        (
            "verdict driver=none path=B writer=m_w",
            writer_lines("m_w"),
            7,
            "driver=none writer=m_w",
        ),
        (
            "verdict driver=none path=B writer=none",
            Vec::new(),
            1,
            "driver=none writer=none",
        ),
    ] {
        let mut driver = vec![ATTEMPT.to_owned()];
        driver.extend(lines);
        let got = run(&refs(&with(driver, line)), &[BOUND]);
        got.passed();
        assert_eq!(
            got.file("freeze.txt"),
            format!("freeze rows={rows} models=1 sources=2 date=2026-11-10\non_device {freeze}\n")
        );
    }
}

#[test]
fn each_field_of_a_bound_line_with_a_wrong_value_is_refused() {
    let lines = [
        "bound model= machine=mac bound_bytes=100 fits=yes",
        "bound model=M machine=mac bound_bytes=100 fits=yes",
        "bound model=synthetic machine=mac bound_bytes=100 fits=yes",
        "bound model=none machine=mac bound_bytes=100 fits=yes",
        "bound model=m_a machine=hosted-mac bound_bytes=100 fits=yes",
        "bound model=m_a machine=gen_a bound_bytes=100 fits=yes",
        "bound model=m_a machine= bound_bytes=100 fits=yes",
        "bound model=m_a machine=mac bound_bytes=0100 fits=yes",
        "bound model=m_a machine=mac bound_bytes=-1 fits=yes",
        "bound model=m_a machine=mac bound_bytes=18446744073709551616 fits=yes",
        "bound model=m_a machine=mac bound_bytes= fits=yes",
        "bound model=m_a machine=mac bound_bytes=100 fits=maybe",
        "bound model=m_a machine=mac bound_bytes=100 fits=not_measured",
        "bound model=m_a machine=mac bound_bytes=100 fits=Yes",
        "bound model=m_a machine=mac bound_bytes=100 fits=",
        "bound model=m_a machine=mac fits=yes bound_bytes=100",
        "bound model=m_a machine=mac bound_bytes=100",
        "bound model=m_a machine=mac bound_bytes=100 fits=yes extra=1",
        "bound model=m_a machine=mac bound_bytes=100 fits=yes ",
        "bound model=m_a  machine=mac bound_bytes=100 fits=yes",
        "bound ",
    ];
    for line in lines {
        let got = run(&[ATTEMPT, VERDICT], &["verdict candidates=-", line]);
        assert_eq!(got.refused(), "REFUSED source: cells:4", "{line:?}");
    }
}

#[test]
fn a_second_bound_line_of_one_model_and_machine_is_refused() {
    let got = run(
        &[ATTEMPT, VERDICT],
        &[
            BOUND,
            "bound model=m_a machine=hosted-win bound_bytes=1 fits=no",
            "bound model=m_a machine=mac bound_bytes=7 fits=no",
        ],
    );
    assert_eq!(got.refused(), "REFUSED source: cells:5");
}

#[test]
fn the_driver_file_is_read_before_the_cells_file_and_each_file_in_line_order() {
    let bad_attempt = attempt_with("seed", "9");
    let got = run(&[ATTEMPT, &bad_attempt, "verdict driver=x"], &["bound x"]);
    assert_eq!(got.refused(), "REFUSED source: driver:4");
    let got = run(&[ATTEMPT], &["bound x"]);
    assert_eq!(got.refused(), "REFUSED source: driver:verdict");
    let got = run(
        &[ATTEMPT, VERDICT],
        &[
            BOUND,
            "bound x",
            "bound model=M machine=mac bound_bytes=1 fits=no",
        ],
    );
    assert_eq!(got.refused(), "REFUSED source: cells:4");
    let got = run_files(
        format!("# d\n```\n{ATTEMPT}\n").into_bytes(),
        b"no block\r\n".to_vec(),
    );
    assert_eq!(got.refused(), "REFUSED source: driver:block");
}

#[test]
fn usage_is_refused_with_its_detail() {
    let good = || {
        (
            file("driver", block(&[ATTEMPT, VERDICT])),
            file("cells", block(&[BOUND])),
        )
    };
    let (driver, cells) = good();
    run_with(driver, cells, &ARGS).passed();
    let cases: Vec<Vec<&str>> = vec![
        vec!["import"],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
        ],
        vec![
            "import",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s1=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s3=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-02-30",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2025-02-29",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-13-01",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-1-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-00",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10 ",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
            "--driver",
            "driver.md",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
            "--seed",
            "1",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
        ],
        vec![
            "import",
            "--driver",
            "",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "-",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "dri\tver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model\"cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "driver.md",
            "--cells",
            "model'cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
        vec![
            "import",
            "--driver",
            "dri\nver.md",
            "--cells",
            "model-cells.md",
            "--date",
            "s1=2026-11-10",
            "--date",
            "s2=2026-11-10",
            "--out",
            "out",
        ],
    ];
    for args in cases {
        let (driver, cells) = good();
        let got = run_with(driver, cells, &args);
        let line = got.refused();
        assert!(line.starts_with("REFUSED usage: "), "{args:?}: {line}");
        assert!(line.len() > "REFUSED usage: ".len(), "{args:?}");
    }
}

#[test]
fn a_path_that_cannot_be_read_and_an_out_that_is_no_empty_directory_are_refused_file() {
    let good = || {
        (
            file("driver", block(&[ATTEMPT, VERDICT])),
            file("cells", block(&[BOUND])),
        )
    };
    let mut args = ARGS.to_vec();
    args[2] = "missing.md";
    let (driver, cells) = good();
    let line = run_with(driver, cells, &args).refused();
    assert!(line.starts_with("REFUSED file: "), "{line}");
    assert!(line.contains("--driver"), "{line}");
    let mut args = ARGS.to_vec();
    args[4] = "missing.md";
    let (driver, cells) = good();
    let line = run_with(driver, cells, &args).refused();
    assert!(line.starts_with("REFUSED file: "), "{line}");
    assert!(line.contains("--cells"), "{line}");
    let mut args = ARGS.to_vec();
    args[2] = ".";
    let (driver, cells) = good();
    let line = run_with(driver, cells, &args).refused();
    assert!(line.starts_with("REFUSED file: "), "{line}");

    for target in ["no-such-dir", "driver.md"] {
        let mut args = ARGS.to_vec();
        args[10] = target;
        let (driver, cells) = good();
        let got = run_with(driver, cells, &args);
        let line = got.refused();
        assert!(line.starts_with("REFUSED file: "), "{target}: {line}");
        assert!(line.contains("--out"), "{line}");
    }
    for name in ["recorded.tsv", "models.tsv", "sources.tsv", "freeze.txt"] {
        let (driver, cells) = good();
        let dir = scratch();
        let out = dir.join("out");
        std::fs::create_dir(&out).expect("out");
        std::fs::write(dir.join("driver.md"), driver).expect("driver");
        std::fs::write(dir.join("model-cells.md"), cells).expect("cells");
        std::fs::write(out.join(name), "kept\n").expect("a file that stands");
        std::fs::write(out.join("other.txt"), "other\n").expect("another file");
        let output = Command::new(env!("CARGO_BIN_EXE_route-gate"))
            .current_dir(&dir)
            .args(ARGS)
            .output()
            .expect("the tool runs");
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(output.stdout, b"");
        assert_eq!(
            String::from_utf8(output.stderr).expect("utf-8"),
            format!("REFUSED file: --out holds {name}\n")
        );
        let mut want = vec![name.to_owned(), "other.txt".to_owned()];
        want.sort();
        assert_eq!(names(&out), want);
        assert_eq!(
            std::fs::read_to_string(out.join(name)).expect("kept"),
            "kept\n"
        );
    }
}

#[test]
fn a_path_with_a_backslash_is_kept_as_given() {
    let dir = scratch();
    let out = dir.join("out");
    std::fs::create_dir(&out).expect("out");
    if cfg!(windows) {
        std::fs::create_dir(dir.join("sub")).expect("sub");
    }
    std::fs::write(
        dir.join("sub\\driver.md"),
        file("driver", block(&[ATTEMPT, VERDICT])),
    )
    .expect("driver");
    std::fs::write(dir.join("model-cells.md"), file("cells", block(&[BOUND]))).expect("cells");
    let mut args = ARGS.to_vec();
    args[2] = "sub\\driver.md";
    let output = Command::new(env!("CARGO_BIN_EXE_route-gate"))
        .current_dir(&dir)
        .args(args)
        .output()
        .expect("the tool runs");
    assert_eq!(
        String::from_utf8(output.stderr).expect("utf-8"),
        "",
        "a backslash is no byte that T0.4 keeps out of a cell"
    );
    assert_eq!(output.status.code(), Some(0));
    let sources = std::fs::read_to_string(out.join("sources.tsv")).expect("written");
    assert!(sources.contains("\ns1\tsub\\driver.md\t"), "{sources}");
}

#[test]
fn a_refusal_of_the_source_writes_nothing() {
    let bad = attempt_with("pass", "maybe");
    let got = run(&[ATTEMPT, VERDICT, &bad], &[BOUND]);
    assert_eq!(got.refused(), "REFUSED source: driver:5");
    let got = run(&[ATTEMPT, VERDICT], &[BOUND, "bound model=m_a"]);
    assert_eq!(got.refused(), "REFUSED source: cells:4");
}

#[test]
fn a_row_count_of_many_attempts_is_exact() {
    let mut lines = Vec::new();
    for model in ["m_a", "m_b"] {
        for machine in ["mac", "hosted-win"] {
            for seed in 1..=3 {
                for i in 1..=3 {
                    lines.push(format!("attempt model={model} machine={machine} task=bill seed={seed} kind=replay i={i} pass=yes steps=1 wall_s=1 tokens_in=0 tokens_out=0"));
                }
            }
        }
    }
    lines.push(VERDICT.to_owned());
    let got = run(&refs(&lines), &[BOUND]);
    got.passed();
    assert_eq!(got.stdout, "import rows=36 models=1 sources=2\n");
    let recorded = got.file("recorded.tsv");
    assert_eq!(recorded.lines().count(), 37);
    let mut sorted: Vec<&str> = recorded.lines().skip(1).collect();
    sorted.sort_unstable();
    assert_eq!(sorted, recorded.lines().skip(1).collect::<Vec<_>>());
}
