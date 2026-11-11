use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use halvo_route::policy::Policy;
use halvo_route_gate::table::HEADER;

fn case_dir(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/r3")
        .join(case)
        .join("expected")
}

fn case_file(case: &str, name: &str) -> String {
    std::fs::read_to_string(case_dir(case).join(name)).expect("a golden file")
}

fn case_path(case: &str, name: &str) -> String {
    case_dir(case)
        .join(name)
        .to_str()
        .expect("utf-8 path")
        .to_owned()
}

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    loop {
        let dir = base.join(format!(
            "route-gate-product-recorded-{}-{}",
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

fn assert_refused(args: &[&str], line: &str) {
    let run = run(args);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(2), "", format!("{line}\n").as_str()),
        "{args:?}"
    );
}

#[test]
fn the_tool_reads_a_recorded_table_with_its_freeze_file() {
    let dir = scratch();
    let header = path(&dir, "header.tsv");
    std::fs::write(&header, format!("{HEADER}\n")).expect("fixture");
    let first = path(&dir, "first.tsv");
    let derived = run(&["derive", "--table", &header, "--out", &first]);
    assert_eq!(
        (
            derived.code,
            derived.stdout.as_str(),
            derived.stderr.as_str()
        ),
        (Some(0), "", "")
    );
    assert_eq!(
        std::fs::read_to_string(&first).expect("written"),
        Policy::hand_back_all().to_text()
    );

    let table = case_path("full", "recorded.tsv");
    let freeze = case_path("full", "freeze.txt");
    let policy = path(&dir, "full.tsv");
    let derived = run(&[
        "derive", "--table", &table, "--freeze", &freeze, "--out", &policy,
    ]);
    assert_eq!(derived.code, Some(0), "{}", derived.stderr);
    let text = std::fs::read_to_string(&policy).expect("written");
    assert_eq!(text.matches("\ton_device\t").count(), 5);
    let scored = run(&[
        "score", "--table", &table, "--router", "product", "--policy", &policy, "--freeze", &freeze,
    ]);
    assert_eq!((scored.code, scored.stderr.as_str()), (Some(0), ""));
    assert_eq!(
        scored.stdout.lines().last(),
        Some("gate table=recorded router=product cells=44 routed=5 handed_back=25 derivation=same held=ok result=pass")
    );
    let blind = run(&["score", "--table", &table, "--router", "derived"]);
    assert_eq!(
        blind.stdout.lines().last(),
        Some("gate table=recorded router=derived cells=44 routed=0 handed_back=30 derivation=same held=ok result=pass")
    );
    let blind_product = run(&[
        "score", "--table", &table, "--router", "product", "--policy", &first,
    ]);
    assert_eq!(blind_product.code, Some(0));

    let hosted = path(&dir, "hosted.tsv");
    std::fs::write(
        &hosted,
        text.replacen("\ton_device\tmedium\t", "\tfrontier\tmedium\t", 1),
    )
    .expect("fixture");
    assert_refused(
        &[
            "score", "--table", &table, "--router", "product", "--policy", &hosted, "--freeze",
            &freeze,
        ],
        "REFUSED policy: ruling",
    );
}

#[test]
fn the_tool_refuses_a_freeze_file_that_does_not_fit() {
    let dir = scratch();
    let table = case_path("full", "recorded.tsv");
    let good = case_file("full", "freeze.txt");
    let cases = [
        ("missing.txt", None, "REFUSED file: --freeze cannot be read"),
        (
            "latin.txt",
            Some(b"freeze rows=132 models=6 sources=2 date=2026-11-10\non_device driver=m_\xe9 writer=none\n".to_vec()),
            "REFUSED file: --freeze is not UTF-8 text",
        ),
        (
            "two.txt",
            Some(good.replace("sources=2", "sources=3").into_bytes()),
            "REFUSED file: --freeze line 1 is not of the freeze form",
        ),
        (
            "driver.txt",
            Some(good.replace("driver=m_small", "driver=M").into_bytes()),
            "REFUSED file: --freeze line 2 is not of the freeze form",
        ),
        (
            "rows.txt",
            Some(good.replace("rows=132", "rows=131").into_bytes()),
            "REFUSED file: --freeze counts 131 rows and --table holds 132",
        ),
        (
            "more.txt",
            Some(good.replace("rows=132", "rows=133").into_bytes()),
            "REFUSED file: --freeze counts 133 rows and --table holds 132",
        ),
        (
            "other.txt",
            Some(case_file("no_driver", "freeze.txt").into_bytes()),
            "REFUSED file: --freeze counts 1 rows and --table holds 132",
        ),
    ];
    for (name, bytes, line) in cases {
        let freeze = path(&dir, name);
        if let Some(bytes) = bytes {
            std::fs::write(&freeze, bytes).expect("fixture");
        }
        assert_refused(
            &[
                "score", "--table", &table, "--router", "derived", "--freeze", &freeze,
            ],
            line,
        );
        assert_refused(
            &[
                "derive",
                "--table",
                &table,
                "--freeze",
                &freeze,
                "--out",
                &path(&dir, &format!("{name}.tsv")),
            ],
            line,
        );
    }
    assert!(!Path::new(&path(&dir, "rows.txt.tsv")).exists());
}
