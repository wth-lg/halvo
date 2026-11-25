use std::path::{Path, PathBuf};
use std::process::Command;

use halvo_route::prices::Prices;
use halvo_route_gate::spendsim::{simulate, simulate_with};

const SEQUENCES: &str = "../../docs/r6/gateway/sequences.jsonl";

/// The prices of the committed sequences: the billing run's fixture rates (B-3.2), as a text of this test's own, so that
/// the committed file does not follow the compiled-in price file.
const FIXTURE_PRICES: &str = "tier\tinput\toutput\tsource\tdate\n\
on_device\t0\t0\tdevice\t2026-11-10\n\
low_cost_hosted\t3100000\t15700000\tfixture:B-3.2\t2026-11-10\n\
frontier\t15500000\t78500000\tfixture:B-3.2\t2026-11-10\n";

/// The default cap of the committed sequences, in minor units.
const FIXTURE_CAP: u64 = 2_000;

fn committed() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(SEQUENCES))
        .expect("the committed sequences of docs/r6/gateway")
}

/// The committed sequences are the export of the seed 1 at the prices `FIXTURE_PRICES` and the cap `FIXTURE_CAP`.
#[test]
fn sequences_same() {
    let committed = committed();
    let prices = Prices::parse(FIXTURE_PRICES).expect("the fixture prices parse");
    let fresh = simulate_with(1, &prices, Some(FIXTURE_CAP)).export();
    let first = committed
        .split_inclusive('\n')
        .zip(fresh.split_inclusive('\n'))
        .position(|(old, new)| old != new);
    assert!(
        committed == fresh,
        "docs/r6/gateway/sequences.jsonl holds {} lines and a fresh export for the seed 1 {}; the \
         first line that differs is {:?}",
        committed.lines().count(),
        fresh.lines().count(),
        first.map(|index| index + 1)
    );
}

fn scratch() -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    (0..1_000)
        .map(|n| base.join(format!("route-gate-sequences-{}-{n}", std::process::id())))
        .find(|dir| std::fs::create_dir(dir).is_ok())
        .expect("a new scratch directory")
}

fn spend_sim(args: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_route-gate"))
        .arg("spend-sim")
        .args(args)
        .output()
        .expect("the tool runs");
    (
        output.status.code(),
        String::from_utf8(output.stdout).expect("utf-8"),
        String::from_utf8(output.stderr).expect("utf-8"),
    )
}

#[test]
fn the_export_of_each_seed_holds_the_moves_of_its_own_run() {
    let dir = scratch();
    let mut files = Vec::new();
    for seed in [2u64, 3] {
        let out = dir.join(format!("sequences-{seed}.jsonl"));
        let path = out.to_str().expect("a UTF-8 path");
        let run = spend_sim(&["--seed", &seed.to_string(), "--export", path]);
        assert_eq!(run.0, Some(0), "seed {seed}");
        let text = std::fs::read_to_string(&out).expect("the export");
        assert_eq!(text, simulate(seed).export(), "seed {seed}");
        files.push(text);
    }
    assert_ne!(files[0], files[1]);
}

#[test]
fn the_export_is_a_new_file_that_holds_the_moves_of_the_run() {
    let dir = scratch();
    let out = dir.join("sequences.jsonl");
    let path = out.to_str().expect("a UTF-8 path");
    let outcome = simulate(1);
    let lines = format!("{}\n", outcome.lines().join("\n"));
    assert_eq!(
        spend_sim(&["--seed", "1", "--export", path]),
        (Some(0), lines, String::new())
    );
    assert_eq!(
        std::fs::read_to_string(&out).expect("the export"),
        outcome.export()
    );
    std::fs::write(&out, "bytes of another call\n").expect("fixture");
    assert_eq!(
        spend_sim(&["--export", path, "--seed", "1"]),
        (
            Some(2),
            String::new(),
            "REFUSED file: --export exists\n".to_owned()
        )
    );
    assert_eq!(
        std::fs::read_to_string(&out).expect("kept"),
        "bytes of another call\n"
    );
    let missing = dir.join("no-such-directory").join("sequences.jsonl");
    assert_eq!(
        spend_sim(&[
            "--seed",
            "1",
            "--export",
            missing.to_str().expect("a UTF-8 path")
        ]),
        (
            Some(2),
            String::new(),
            "REFUSED file: --export cannot be written\n".to_owned()
        )
    );
    let mut names: Vec<String> = std::fs::read_dir(&dir)
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
    assert_eq!(names, ["sequences.jsonl"]);
}
