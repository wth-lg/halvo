//! The block of a hosted round as the third source of `route-gate import`: its rows, the freeze
//! file's third line, the fill of the tier `frontier` and none of `low_cost_hosted`, and its
//! refusals. The blocks are made up; the two trial files are the golden case `no_driver`.

use std::path::Path;

use halvo_route::policy::{Basis, PickCell, Policy, Row as PolicyRow};
use halvo_route::tier::{CallKind, CategoryId, Level, Tier};
use halvo_route_gate::derive::derive;
use halvo_route_gate::import::{blob_id, import_hosted, Date, HostModels, Imported, Trial};
use halvo_route_gate::product::recorded_setup;
use halvo_route_gate::table::Table;
use halvo_route_gate::Setup;

const COMMIT: &str = "commit head=0123456789abcdef0123456789abcdef01234567 mask=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const RECORDS: &str = "records asks=216 requests_sha256=89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567 answers_sha256=fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

/// The hosted tiers' model words, written with `-` as a table of hosts writes them.
fn models() -> HostModels {
    HostModels::new(Some("m-low"), Some("m-top"))
}

/// The lines of one model's cells in both passes; `fails` names the compile attempts, as (task,
/// seed, pass), that do not pass.
fn cells(model: &str, fails: &[(&str, u32, u32)]) -> Vec<String> {
    let mut lines = Vec::new();
    for pass in 1..=2u32 {
        for task in ["bill", "order"] {
            for seed in 1..=3u32 {
                let ok = !fails.contains(&(task, seed, pass));
                let (yes, compile) = if ok { ("yes", "ok") } else { ("no", "miss") };
                lines.push(format!("attempt model={model} machine=mac task={task} seed={seed} kind=compile i={pass} pass={yes} steps=3 wall_s=10 tokens_in=5 tokens_out=6"));
                for replay in 3 * pass - 2..=3 * pass {
                    lines.push(format!("attempt model={model} machine=mac task={task} seed={seed} kind=replay i={replay} pass=yes steps=3 wall_s=1 tokens_in=0 tokens_out=0"));
                }
                lines.push(format!("drive model={model} machine=mac task={task} seed={seed} i={pass} compile={compile} replays_ok=3 of=3 recover=notread steps=3 wall_s=10 model_tokens=11 typed_credential=0 effects_without_confirm=0"));
                lines.push(format!("marks model={model} machine=mac task={task} seed={seed} i={pass} requests=5 with_mark=5 slot_text=0"));
            }
        }
        for seed in 1..=3u32 {
            lines.push(format!("attempt model={model} machine=mac task=reply seed={seed} kind=write i={pass} pass=yes steps=1 wall_s=4 tokens_in=7 tokens_out=2"));
            lines.push(format!("reply model={model} machine=mac seed={seed} i={pass} bodies=2 of=2 wall_s=4 model_tokens=9"));
            lines.push(format!("marks model={model} machine=mac task=reply seed={seed} i={pass} requests=2 with_mark=2 slot_text=0"));
        }
    }
    lines
}

fn count(tier: &str, model: &str, drive: u32) -> String {
    format!("count tier={tier} model={model} drive={drive} of=12 reply=6 of=6 typed_credential=0 effects_without_confirm=0 marked=18 of=18 slot_text=0")
}

/// A block of two passes: `m_low` with the compile of `bill` seed 1 failing in the second pass
/// unless `low_passes_all`, `m_top` passing every attempt, and the verdict line given.
fn block(low_passes_all: bool, verdict: &str) -> Vec<String> {
    let fails: &[(&str, u32, u32)] = if low_passes_all {
        &[]
    } else {
        &[("bill", 1, 2)]
    };
    let mut lines = vec![COMMIT.to_owned()];
    lines.extend(cells("m_low", fails));
    lines.extend(cells("m_top", &[]));
    lines.push(count(
        "low_cost_hosted",
        "m_low",
        if low_passes_all { 12 } else { 11 },
    ));
    lines.push(count("frontier", "m_top", 12));
    lines.push(RECORDS.to_owned());
    lines.push(verdict.to_owned());
    lines
}

/// The file of a block: a heading, an empty line, the opening fence (line 3), the lines, the
/// closing fence. The block's line `k`, counted from 0, is the file's line `k + 4`.
fn file(lines: &[String]) -> Vec<u8> {
    let mut text = String::from("# Hosted round (made up)\n\n```text\n");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("```\n");
    text.into_bytes()
}

fn case(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/r3/no_driver")
            .join(name),
    )
    .expect("the golden case no_driver")
}

fn run(hosted: &[u8]) -> Result<Imported, String> {
    let (driver, cells) = (case("driver.md"), case("model-cells.md"));
    let (early, late) = (
        Date::parse("2026-11-10").expect("a date"),
        Date::parse("2026-11-26").expect("a date"),
    );
    import_hosted(
        Trial {
            path: "driver.md",
            bytes: &driver,
            date: &early,
        },
        Trial {
            path: "model-cells.md",
            bytes: &cells,
            date: &early,
        },
        Trial {
            path: "hosted.md",
            bytes: hosted,
            date: &late,
        },
        &models(),
    )
    .map_err(|error| error.detail())
}

fn text<'a>(imported: &'a Imported, name: &str) -> &'a str {
    &imported
        .files
        .iter()
        .find(|(file, _)| *file == name)
        .expect("a file")
        .1
}

/// The policy that `derive` writes from the imported table and freeze file, with every tier allowed.
fn policy(imported: &Imported) -> Policy {
    let table = Table::parse(text(imported, "recorded.tsv")).expect("the recorded table");
    let setup = Setup {
        allowed: Tier::ALL.to_vec(),
        ..recorded_setup(Some(text(imported, "freeze.txt"))).expect("the freeze file")
    };
    derive(&table, &setup)
}

fn frontier(k: u32, n: u32) -> PolicyRow {
    PolicyRow {
        pick: PickCell::Tier(Tier::Frontier),
        level: Some(Level::Medium),
        basis: Basis::LineMet,
        k,
        n,
    }
}

fn untested() -> PolicyRow {
    PolicyRow {
        pick: PickCell::HandBack,
        level: None,
        basis: Basis::Untested,
        k: 0,
        n: 0,
    }
}

const DRIVEN: [CategoryId; 2] = [CategoryId::UtilityBillPayment, CategoryId::ShopOrder];

#[test]
fn a_hosted_block_of_two_passes_gives_its_rows_and_the_frontier_fill() {
    let hosted = file(&block(false, "verdict driver=m_top path=A writer=m_top"));
    let imported = run(&hosted).expect("the block imports");
    assert_eq!(imported.line(), "import rows=113 models=1 sources=3");
    assert_eq!(
        text(&imported, "freeze.txt"),
        "freeze rows=113 models=1 sources=3 date=2026-11-26\non_device driver=none writer=none\nfrontier driver=m_top\n"
    );
    assert_eq!(
        text(&imported, "sources.tsv").lines().nth(3),
        Some(format!("s3\thosted.md\t{}\t2026-11-26\thosted_driver_attempts\t3_seeds_1_machine_2_passes_team_fixtures", blob_id(&hosted)).as_str())
    );
    let recorded = text(&imported, "recorded.tsv");
    for row in [
        "bill\tutility_bill_payment\tcompile\tfrontier\tm_top\tmac\t1\tdev\t2\tyes\t3\t10\t5\t6\tmedium\ts3",
        "order\tshop_order\treplay\tfrontier\tm_top\tmac\t3\theld\t6\tyes\t3\t1\t0\t0\tmedium\ts3",
        "reply\temail\twrite\tfrontier\tm_top\tmac\t2\tdev\t2\tyes\t1\t4\t7\t2\tmedium\ts3",
        "bill\tutility_bill_payment\tcompile\tlow_cost_hosted\tm_low\tmac\t1\tdev\t2\tno\t3\t10\t5\t6\tmedium\ts3",
    ] {
        assert!(recorded.lines().any(|line| line == row), "{row}");
    }
    assert_eq!(
        recorded
            .lines()
            .filter(|line| line.ends_with("\ts3"))
            .count(),
        108
    );
    let policy = policy(&imported);
    for category in DRIVEN {
        assert_eq!(
            *policy.row(category, CallKind::Compile),
            frontier(4, 4),
            "{category:?}"
        );
        assert_eq!(
            *policy.row(category, CallKind::Recover),
            untested(),
            "{category:?}"
        );
    }
    assert_eq!(*policy.row(CategoryId::Email, CallKind::Write), untested());
}

#[test]
fn the_small_model_passing_everything_fills_nothing_for_low_cost_hosted() {
    let on_top = run(&file(&block(
        true,
        "verdict driver=m_top path=A writer=m_top",
    )))
    .expect("on m_top");
    let policy = policy(&on_top);
    for (category, kind) in halvo_route::policy::cells() {
        assert_ne!(
            policy.row(category, kind).pick,
            PickCell::Tier(Tier::LowCostHosted),
            "{category:?} {kind:?}"
        );
    }
    for category in DRIVEN {
        assert_eq!(
            *policy.row(category, CallKind::Compile),
            frontier(4, 4),
            "{category:?}"
        );
    }
    let on_low = run(&file(&block(
        true,
        "verdict driver=m_low path=A writer=m_low",
    )))
    .expect("on m_low");
    assert_eq!(
        text(&on_low, "freeze.txt").lines().nth(2),
        Some("frontier driver=none")
    );
    let policy = self::policy(&on_low);
    for category in DRIVEN {
        assert_eq!(
            *policy.row(category, CallKind::Compile),
            untested(),
            "{category:?}"
        );
    }
    let setup = recorded_setup(Some(text(&on_top, "freeze.txt"))).expect("the freeze file");
    for kind in CallKind::ALL {
        assert_eq!(setup.fill(kind, Tier::LowCostHosted), None, "{kind:?}");
    }
    assert_eq!(setup.fill(CallKind::Recover, Tier::Frontier), None);
    assert_eq!(setup.fill(CallKind::Compile, Tier::Frontier), Some("m_top"));
}

#[test]
fn each_refusal_of_a_hosted_block_names_its_line() {
    let good = block(false, "verdict driver=m_top path=A writer=m_top");
    let drive_low = good
        .iter()
        .position(|line| line.starts_with("drive model=m_low"))
        .expect("a drive line");
    let at = |index: usize, line: &str| {
        let mut lines = good.clone();
        lines.insert(index, line.to_owned());
        (lines, index)
    };
    let replaced = |index: usize, from: &str, to: &str| {
        let mut lines = good.clone();
        let changed = lines[index].replacen(from, to, 1);
        assert_ne!(changed, lines[index], "{from}");
        lines[index] = changed;
        (lines, index)
    };
    let cases = [
        ("a spend line", at(1, "spend tier=frontier requests=216 spent_micro=5 ceiling_minor=100")),
        ("an egress line", at(1, "egress list=list.txt origin=https://top.invalid starts=1 runs=1 denied=0")),
        ("a host line", at(1, "host tier=frontier origin=https://top.invalid model=m-top adapter=a")),
        ("a path", replaced(0, "mask=0123456789abcdef", "mask=crates/halvo-runner/src/model/snapshot.rs")),
        ("a drive line without recover=notread", replaced(drive_low, "recover=notread", "recover=ok")),
        ("a third attempt", at(1, "attempt model=m_low machine=mac task=bill seed=1 kind=compile i=3 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6")),
        ("a seed beyond 3", at(1, "attempt model=m_low machine=mac task=bill seed=4 kind=compile i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6")),
        ("a model that no row names", at(1, "attempt model=m_mid machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6")),
        ("a recovery", at(1, "attempt model=m_top machine=mac task=bill seed=1 kind=recover i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6")),
    ];
    for (name, (lines, index)) in cases {
        assert_eq!(
            run(&file(&lines)).map(|_| ()),
            Err(format!("hosted:{}", index + 4)),
            "{name}"
        );
    }
}
