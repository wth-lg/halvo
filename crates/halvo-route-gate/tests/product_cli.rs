use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

const SPECIMEN: &str = include_str!("data/specimen.tsv");

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    loop {
        let dir = base.join(format!(
            "route-gate-product-cli-{}-{}",
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

fn specimen_policy(dir: &Path) -> (String, String) {
    let table = path(dir, "specimen.tsv");
    std::fs::write(&table, SPECIMEN).expect("fixture");
    let policy = path(dir, "policy.tsv");
    let derived = run(&["derive", "--table", &table, "--out", &policy]);
    assert_eq!(derived.code, Some(0), "{}", derived.stderr);
    (table, policy)
}

#[test]
fn product_on_the_specimen_prints_the_derived_lines() {
    let dir = scratch();
    let (table, policy) = specimen_policy(&dir);
    let product = run(&[
        "score", "--table", &table, "--router", "product", "--policy", &policy,
    ]);
    assert_eq!((product.code, product.stderr.as_str()), (Some(0), ""));
    let derived = run(&["score", "--table", &table, "--router", "derived"]);
    assert_eq!(derived.code, Some(0));
    assert_eq!(
        product.stdout,
        derived.stdout.replace(
            "gate table=generated router=derived ",
            "gate table=generated router=product "
        )
    );
    let last = product.stdout.lines().last().expect("a verdict");
    assert!(last.starts_with("gate table=generated router=product cells=44 "));
    assert!(last.ends_with(" derivation=same held=ok result=pass"));
}

#[test]
fn product_needs_its_policy_before_anything_is_read() {
    let dir = scratch();
    let missing = path(&dir, "none.tsv");
    assert_refused(
        &["score", "--table", &missing, "--router", "product"],
        "REFUSED usage: --policy is required with the router product",
    );
    assert_refused(
        &[
            "score", "--table", &missing, "--router", "product", "--draws", "0",
        ],
        "REFUSED usage: --policy is required with the router product",
    );
    assert_refused(
        &[
            "score", "--table", &missing, "--router", "derived", "--policy", &missing,
        ],
        "REFUSED usage: --policy goes with the router product alone",
    );
    assert_refused(
        &[
            "score", "--table", &missing, "--router", "product", "--policy", &missing,
        ],
        "REFUSED file: --table cannot be read",
    );
}

#[test]
fn product_refuses_a_policy_that_does_not_parse() {
    let dir = scratch();
    let (table, policy) = specimen_policy(&dir);
    let text = std::fs::read_to_string(&policy).expect("written");
    let missing = path(&dir, "none.tsv");
    assert_refused(
        &[
            "score", "--table", &table, "--router", "product", "--policy", &missing,
        ],
        "REFUSED file: --policy cannot be read",
    );
    let cases = [
        (
            "header.tsv",
            text.replacen("category", "categories", 1).into_bytes(),
            "header",
        ),
        (
            "crlf.tsv",
            text.replace('\n', "\r\n").into_bytes(),
            "header",
        ),
        (
            "cr-last.tsv",
            [text.strip_suffix('\n').expect("LF").as_bytes(), b"\r\n"].concat(),
            "rows",
        ),
        (
            "short.tsv",
            text.replacen("other\treplay\tnone\t-\tno_model\t0\t0\n", "", 1)
                .into_bytes(),
            "rows",
        ),
        ("utf8.tsv", [text.as_bytes(), b"\xff\n"].concat(), "rows"),
        (
            "shape.tsv",
            text.replacen(
                "other\treplay\tnone\t-\tno_model\t0\t0",
                "other\treplay\tnone\t-\tno_model\t0\t1",
                1,
            )
            .into_bytes(),
            "shape",
        ),
    ];
    for (name, bytes, error) in cases {
        let file = path(&dir, name);
        std::fs::write(&file, bytes).expect("fixture");
        assert_refused(
            &[
                "score", "--table", &table, "--router", "product", "--policy", &file,
            ],
            &format!("REFUSED policy: {error}"),
        );
    }
}

#[test]
fn score_takes_a_thousand_draws_when_draws_is_absent() {
    let dir = scratch();
    let (table, policy) = specimen_policy(&dir);
    let random = |extra: &[&str]| {
        let mut args = vec![
            "score", "--table", &table, "--router", "product", "--policy", &policy,
        ];
        args.extend_from_slice(extra);
        let scored = run(&args);
        assert_eq!(scored.code, Some(0), "{extra:?}");
        scored
            .stdout
            .lines()
            .find(|line| line.starts_with("versus name=random "))
            .expect("a random line")
            .to_owned()
    };
    let absent = random(&[]);
    assert_eq!(absent, random(&["--draws", "1000"]));
    assert_ne!(absent, random(&["--draws", "1"]));
}

#[test]
fn self_test_counts_the_router_product() {
    let run = run(&["self-test", "--tables", "3", "--seed", "7"]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (
            Some(0),
            "selftest tables=3 derived_pass=3 product_pass=3 one_tier_fail=3 random_fail=3 peek_fail=3\n",
            ""
        )
    );
}
