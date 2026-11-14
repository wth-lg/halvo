use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use halvo_route_gate::derive::derive;
use halvo_route_gate::generate::generate;
use halvo_route_gate::table::HEADER;
use halvo_route_gate::Setup;

const SPECIMEN: &str = include_str!("data/specimen.tsv");

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    loop {
        let dir = base.join(format!(
            "route-gate-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
}

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_route-gate"))
        .args(args)
        .output()
        .expect("the tool runs");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).expect("utf-8"),
        stderr: String::from_utf8(output.stderr).expect("utf-8"),
    }
}

fn path(dir: &Path, name: &str) -> String {
    dir.join(name).to_str().expect("utf-8 path").to_owned()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the scratch directory")
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

fn assert_refused(args: &[&str], line: &str) {
    let run = run(args);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(2), "", format!("{line}\n").as_str()),
        "{args:?}"
    );
}

#[test]
fn refusals_of_usage() {
    assert_refused(&[], "REFUSED usage: no command");
    assert_refused(&["simulate"], "REFUSED usage: unknown command");
    assert_refused(&["spend-sim"], "REFUSED usage: --seed is required");
    assert_refused(
        &["spend-sim", "--seed", "0"],
        "REFUSED usage: --seed takes a whole number from 1",
    );
    let sim = run(&["spend-sim", "--seed", "1"]);
    assert_eq!(
        (sim.code, sim.stdout.lines().count(), sim.stderr.as_str()),
        (Some(0), 7, "")
    );
    assert!(sim
        .stdout
        .ends_with("\nspend cases=6 overruns=0 above_cap=0 admit_sum=ok result=pass\n"));
    assert_refused(&["probe"], "REFUSED usage: --rounds is required");
    assert_refused(
        &["probe", "--seed", "1"],
        "REFUSED usage: unknown argument --seed",
    );
    let probe = run(&["probe", "--rounds", "100000"]);
    assert_eq!(
        (probe.code, probe.stdout.as_str(), probe.stderr.as_str()),
        (Some(0), "probe rounds=100000 ok=yes\n", "")
    );
    assert_refused(
        &["generate", "--seed", "1"],
        "REFUSED usage: --out is required",
    );
    assert_refused(
        &["generate", "--out", "x"],
        "REFUSED usage: --seed is required",
    );
    assert_refused(
        &["generate", "--seed", "0", "--out", "x"],
        "REFUSED usage: --seed takes a whole number from 1",
    );
    assert_refused(
        &["generate", "--seed", "01", "--out", "x"],
        "REFUSED usage: --seed takes a whole number from 1",
    );
    assert_refused(
        &["generate", "--seed", "18446744073709551616", "--out", "x"],
        "REFUSED usage: --seed takes a whole number from 1",
    );
    assert_refused(
        &["generate", "--seed", "1", "--seed", "2", "--out", "x"],
        "REFUSED usage: --seed twice",
    );
    assert_refused(
        &["generate", "--seed", "1", "--out"],
        "REFUSED usage: --out has no value",
    );
    assert_refused(
        &["generate", "--seed", "1", "--out", "x", "--draws", "3"],
        "REFUSED usage: unknown argument --draws",
    );
    assert_refused(
        &["generate", "--seed=1", "--out", "x"],
        "REFUSED usage: unknown argument of another form",
    );
    assert_refused(
        &["self-test", "--tables", "0", "--seed", "1"],
        "REFUSED usage: --tables takes a whole number from 1",
    );
    assert_refused(
        &["self-test", "--tables", "4294967296", "--seed", "1"],
        "REFUSED usage: --tables takes a whole number from 1 to 4294967295",
    );
    assert_refused(
        &[
            "self-test",
            "--tables",
            "2",
            "--seed",
            "18446744073709551615",
        ],
        "REFUSED usage: the seeds from --seed on do not fit a u64",
    );
    assert_refused(
        &["self-test", "--tables", "2"],
        "REFUSED usage: --seed is required",
    );
    assert_refused(
        &[
            "score", "--table", "x", "--router", "product", "--policy", "y",
        ],
        "REFUSED file: --table cannot be read",
    );
    assert_refused(
        &["score", "--table", "x", "--router", "best"],
        "REFUSED usage: --router is one of derived, product, one_tier, random, peek",
    );
    assert_refused(
        &[
            "score", "--table", "x", "--router", "derived", "--policy", "y",
        ],
        "REFUSED usage: --policy goes with the router product alone",
    );
    assert_refused(
        &[
            "score", "--table", "x", "--router", "derived", "--draws", "0",
        ],
        "REFUSED usage: --draws takes a whole number from 1",
    );
}

#[test]
fn generate_writes_a_new_file_only() {
    let dir = scratch();
    let out = path(&dir, "specimen.tsv");
    let first = run(&["generate", "--seed", "1", "--out", &out]);
    assert_eq!(
        (first.code, first.stdout.as_str(), first.stderr.as_str()),
        (Some(0), "", "")
    );
    assert_eq!(std::fs::read_to_string(&out).expect("written"), SPECIMEN);
    assert_refused(
        &["generate", "--seed", "2", "--out", &out],
        "REFUSED file: --out exists",
    );
    assert_eq!(std::fs::read_to_string(&out).expect("kept"), SPECIMEN);
    assert_refused(
        &[
            "generate",
            "--seed",
            "2",
            "--out",
            &path(&dir, "missing/specimen.tsv"),
        ],
        "REFUSED file: --out cannot be written",
    );
    let second = path(&dir, "seed-2.tsv");
    assert_eq!(
        run(&["generate", "--seed", "2", "--out", &second]).code,
        Some(0)
    );
    assert_eq!(
        std::fs::read_to_string(&second).expect("written"),
        generate(2).to_text()
    );
}

#[cfg(unix)]
fn run_under_a_size_limit(args: &[&str]) -> Run {
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg("trap '' XFSZ; ulimit -f 1; exec \"$0\" \"$@\"")
        .arg(env!("CARGO_BIN_EXE_route-gate"))
        .args(args)
        .output()
        .expect("sh runs");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).expect("utf-8"),
        stderr: String::from_utf8(output.stderr).expect("utf-8"),
    }
}

#[cfg(unix)]
#[test]
fn a_write_that_fails_leaves_no_file_at_out() {
    let dir = scratch();
    let table = path(&dir, "table.tsv");
    std::fs::write(&table, SPECIMEN).expect("fixture");
    let generated = path(&dir, "generated.tsv");
    let policy = path(&dir, "policy.tsv");
    let calls = [
        vec!["generate", "--seed", "1", "--out", &generated],
        vec!["derive", "--table", &table, "--out", &policy],
    ];
    for args in calls {
        let run = run_under_a_size_limit(&args);
        assert_eq!(
            (run.code, run.stdout.as_str(), run.stderr.as_str()),
            (Some(2), "", "REFUSED file: --out cannot be written\n"),
            "{args:?}"
        );
        assert!(!Path::new(args[args.len() - 1]).exists(), "{args:?}");
    }
    let kept = path(&dir, "kept.tsv");
    std::fs::write(&kept, "bytes of another call\n").expect("fixture");
    let run = run_under_a_size_limit(&["generate", "--seed", "1", "--out", &kept]);
    assert_eq!(
        (run.code, run.stderr.as_str()),
        (Some(2), "REFUSED file: --out exists\n")
    );
    assert_eq!(
        std::fs::read_to_string(&kept).expect("kept"),
        "bytes of another call\n"
    );
    assert_eq!(names(&dir), ["kept.tsv", "table.tsv"]);
}

#[cfg(unix)]
#[test]
fn a_call_killed_by_the_size_limit_leaves_no_table_at_out() {
    let dir = scratch();
    let table = path(&dir, "table.tsv");
    std::fs::write(&table, SPECIMEN).expect("fixture");
    let kept = path(&dir, "kept.tsv");
    std::fs::write(&kept, "bytes of another call\n").expect("fixture");
    let generated = path(&dir, "generated.tsv");
    let policy = path(&dir, "policy.tsv");
    let calls = [
        vec!["generate", "--seed", "1", "--out", &generated],
        vec!["derive", "--table", &table, "--out", &policy],
        vec!["generate", "--seed", "1", "--out", &kept],
    ];
    for args in &calls {
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg("ulimit -f 1; exec \"$0\" \"$@\"")
            .arg(env!("CARGO_BIN_EXE_route-gate"))
            .args(args)
            .output()
            .expect("sh runs");
        assert_ne!(output.status.code(), Some(0), "{args:?}");
    }
    assert!(!Path::new(&generated).exists());
    assert!(!Path::new(&policy).exists());
    assert_eq!(
        std::fs::read_to_string(&kept).expect("kept"),
        "bytes of another call\n"
    );
    for entry in std::fs::read_dir(&dir).expect("the scratch directory") {
        let name = entry
            .expect("an entry")
            .file_name()
            .into_string()
            .expect("utf-8");
        let beside = name.starts_with("route-gate-") && name.ends_with(".part");
        assert!(
            beside || name == "table.tsv" || name == "kept.tsv",
            "{name}"
        );
    }
    for args in &calls[..2] {
        let run = run(args);
        assert_eq!((run.code, run.stderr.as_str()), (Some(0), ""), "{args:?}");
    }
    assert_eq!(
        std::fs::read_to_string(&generated).expect("written"),
        SPECIMEN
    );
    assert_eq!(
        std::fs::read_to_string(&policy).expect("written"),
        derive(&generate(1), &Setup::generated()).to_text()
    );
}

#[test]
fn an_out_name_of_250_bytes_is_written() {
    let dir = scratch();
    let name = format!("{}.tsv", "g".repeat(246));
    assert_eq!(name.len(), 250);
    let long = path(&dir, &name);
    let run_long = run(&["generate", "--seed", "1", "--out", &long]);
    assert_eq!(
        (
            run_long.code,
            run_long.stdout.as_str(),
            run_long.stderr.as_str()
        ),
        (Some(0), "", "")
    );
    assert_eq!(std::fs::read_to_string(&long).expect("written"), SPECIMEN);
    let too_long = path(&dir, &format!("{}.tsv", "r".repeat(296)));
    let run_too_long = run(&["generate", "--seed", "1", "--out", &too_long]);
    assert_eq!(
        (run_too_long.code, run_too_long.stdout.as_str()),
        (Some(2), "")
    );
    assert!(
        run_too_long.stderr.starts_with("REFUSED file: "),
        "{}",
        run_too_long.stderr
    );
    assert_eq!(names(&dir), [name]);
}

#[test]
fn a_file_at_out_before_the_call_keeps_its_bytes() {
    let dir = scratch();
    let table = path(&dir, "table.tsv");
    std::fs::write(&table, SPECIMEN).expect("fixture");
    let kept = path(&dir, "kept.tsv");
    std::fs::write(&kept, "bytes of another call\n").expect("fixture");
    for args in [
        vec!["generate", "--seed", "1", "--out", &kept],
        vec!["derive", "--table", &table, "--out", &kept],
    ] {
        assert_refused(&args, "REFUSED file: --out exists");
        assert_eq!(
            std::fs::read_to_string(&kept).expect("kept"),
            "bytes of another call\n",
            "{args:?}"
        );
    }
}

#[test]
fn derive_writes_the_policy_of_a_generated_table() {
    let dir = scratch();
    let table = path(&dir, "table.tsv");
    std::fs::write(&table, SPECIMEN).expect("fixture");
    let out = path(&dir, "policy.tsv");
    let run = run(&["derive", "--table", &table, "--out", &out]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(0), "", "")
    );
    assert_eq!(
        std::fs::read_to_string(&out).expect("written"),
        derive(&generate(1), &Setup::generated()).to_text()
    );
    assert_refused(
        &["derive", "--table", &table, "--out", &out],
        "REFUSED file: --out exists",
    );
    assert_refused(
        &[
            "derive",
            "--table",
            &table,
            "--freeze",
            &table,
            "--out",
            &path(&dir, "p2.tsv"),
        ],
        "REFUSED usage: --freeze names the freeze file of a recorded table",
    );
    let header = path(&dir, "header.tsv");
    std::fs::write(&header, format!("{HEADER}\n")).expect("fixture");
    let recorded = crate::run(&[
        "derive",
        "--table",
        &header,
        "--out",
        &path(&dir, "first.tsv"),
    ]);
    assert_eq!(
        (
            recorded.code,
            recorded.stdout.as_str(),
            recorded.stderr.as_str()
        ),
        (Some(0), "", "")
    );
    assert_refused(
        &[
            "derive",
            "--table",
            &path(&dir, "none.tsv"),
            "--out",
            &path(&dir, "p4.tsv"),
        ],
        "REFUSED file: --table cannot be read",
    );
    let swapped = path(&dir, "swapped.tsv");
    let mut lines: Vec<&str> = SPECIMEN.lines().collect();
    lines.swap(1, 2);
    std::fs::write(&swapped, format!("{}\n", lines.join("\n"))).expect("fixture");
    assert_refused(
        &[
            "derive",
            "--table",
            &swapped,
            "--out",
            &path(&dir, "p5.tsv"),
        ],
        "REFUSED table: order:3",
    );
    let crlf = path(&dir, "crlf.tsv");
    std::fs::write(&crlf, SPECIMEN.replace('\n', "\r\n")).expect("fixture");
    assert_refused(
        &["derive", "--table", &crlf, "--out", &path(&dir, "p6.tsv")],
        "REFUSED table: text:1",
    );
    let binary = path(&dir, "binary.tsv");
    std::fs::write(&binary, [0xff, 0xfe, b'\n']).expect("fixture");
    assert_refused(
        &["derive", "--table", &binary, "--out", &path(&dir, "p7.tsv")],
        "REFUSED table: text:utf-8",
    );
    for name in ["p2.tsv", "p4.tsv", "p5.tsv", "p6.tsv", "p7.tsv"] {
        assert!(!dir.join(name).exists(), "{name} written by a refusal");
    }
}

#[test]
fn score_prints_its_lines_and_exits_by_the_verdict() {
    let dir = scratch();
    let table = path(&dir, "table.tsv");
    std::fs::write(&table, SPECIMEN).expect("fixture");
    let derived = run(&["score", "--table", &table, "--router", "derived"]);
    assert_eq!((derived.code, derived.stderr.as_str()), (Some(0), ""));
    let lines: Vec<&str> = derived.stdout.lines().collect();
    assert_eq!(lines.len(), 44 + 11 + 6 + 1);
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("cell "))
            .count(),
        44
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("category "))
            .count(),
        11
    );
    assert_eq!(lines[61], "gate table=generated router=derived cells=44 routed=21 handed_back=9 derivation=same held=ok result=pass");
    for (router, verdict) in [
        (
            "one_tier",
            "routed=30 handed_back=0 derivation=differs held=miss result=fail",
        ),
        (
            "peek",
            "routed=28 handed_back=2 derivation=differs held=ok result=fail",
        ),
        (
            "random",
            "routed=23 handed_back=7 derivation=differs held=miss result=fail",
        ),
    ] {
        let run = run(&[
            "score", "--table", &table, "--router", router, "--draws", "5",
        ]);
        assert_eq!((run.code, run.stderr.as_str()), (Some(1), ""), "{router}");
        let last = run.stdout.lines().last().expect("a verdict");
        assert_eq!(
            last,
            format!("gate table=generated router={router} cells=44 {verdict}")
        );
    }
    let header = path(&dir, "header.tsv");
    std::fs::write(&header, format!("{HEADER}\n")).expect("fixture");
    let recorded = run(&["score", "--table", &header, "--router", "derived"]);
    assert_eq!((recorded.code, recorded.stderr.as_str()), (Some(0), ""));
    let categories = [
        ("utility_bill_payment", "untested"),
        ("tenant_rent", "no_runtime"),
        ("lease_renewal_or_change", "no_runtime"),
        ("tenant_application_review", "no_runtime"),
        ("email", "untested"),
        ("spreadsheet_fill", "untested"),
        ("shop_order", "untested"),
        ("calendar_cleanup", "untested"),
        ("ride_order", "no_runtime"),
        ("daily_chart_check", "no_runtime"),
        ("other", "not_seed"),
    ];
    let zero = "dev_k=0 dev_n=0 held_k=0 held_n=0 lower_pct=0 cost_micro=0 wall_s=0";
    let mut want = String::new();
    for (id, word) in categories {
        for kind in ["compile", "recover", "write"] {
            want += &format!("cell category={id} kind={kind} pick=hand_back basis={word} {zero}\n");
        }
        want += &format!("cell category={id} kind=replay pick=none basis=no_model {zero}\n");
    }
    for (id, word) in categories {
        want += &format!("category id={id} compile=hand_back recover=hand_back write=hand_back line={word} cost_micro=0\n");
    }
    for name in ["router", "random", "hindsight"] {
        want += &format!(
            "versus name={name} attempts=0 passed=0 wasted=0 handed_back=0 cost_micro=0 wall_s=0\n"
        );
    }
    want += "gate table=recorded router=derived cells=44 routed=0 handed_back=30 derivation=same held=ok result=pass\n";
    assert_eq!(recorded.stdout, want);
    assert_refused(
        &[
            "score", "--table", &table, "--router", "derived", "--freeze", &table,
        ],
        "REFUSED usage: --freeze names the freeze file of a recorded table",
    );
}

#[test]
fn self_test_prints_its_line() {
    let run = run(&["self-test", "--tables", "3", "--seed", "7"]);
    assert_eq!((run.code, run.stdout.as_str(), run.stderr.as_str()), (Some(0), "selftest tables=3 derived_pass=3 product_pass=3 one_tier_fail=3 random_fail=3 peek_fail=3\n", ""));
}
