use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

const FILES: [&str; 4] = ["freeze.txt", "models.tsv", "recorded.tsv", "sources.tsv"];

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    loop {
        let dir = base.join(format!(
            "route-gate-import-golden-{}-{}",
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

fn case_dir(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/r3")
        .join(case)
}

fn freeze_count(freeze: &str, key: &str) -> String {
    freeze
        .split([' ', '\n'])
        .find_map(|word| word.strip_prefix(key))
        .expect("the key")
        .to_owned()
}

#[test]
fn import_golden() {
    let cases = ["full", "no_writer", "no_driver"];
    assert_eq!(
        names(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/r3")),
        {
            let mut sorted = cases.map(str::to_owned).to_vec();
            sorted.sort();
            sorted
        }
    );
    for case in cases {
        let dir = case_dir(case);
        assert_eq!(
            names(&dir),
            ["driver.md", "expected", "model-cells.md"],
            "{case}"
        );
        assert_eq!(names(&dir.join("expected")), FILES, "{case}");
        let out = scratch();
        let output = Command::new(env!("CARGO_BIN_EXE_route-gate"))
            .current_dir(&dir)
            .args([
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
                out.to_str().expect("utf-8 path"),
            ])
            .output()
            .expect("the tool runs");
        assert_eq!(
            String::from_utf8(output.stderr).expect("utf-8"),
            "",
            "{case}"
        );
        assert_eq!(output.status.code(), Some(0), "{case}");
        let freeze =
            std::fs::read_to_string(dir.join("expected/freeze.txt")).expect("the freeze file");
        assert_eq!(
            String::from_utf8(output.stdout).expect("utf-8"),
            format!(
                "import rows={} models={} sources=2\n",
                freeze_count(&freeze, "rows="),
                freeze_count(&freeze, "models=")
            ),
            "{case}"
        );
        assert_eq!(names(&out), FILES, "{case}");
        for file in FILES {
            let got = std::fs::read(out.join(file)).expect("written");
            let want = std::fs::read(dir.join("expected").join(file)).expect("expected");
            assert!(got == want, "{case}/{file} differs from the expected file");
        }
    }
}

#[test]
fn golden_cases_hold_what_the_contract_names() {
    let full = std::fs::read_to_string(case_dir("full").join("expected/freeze.txt")).expect("full");
    assert_eq!(
        full,
        "freeze rows=132 models=6 sources=2 date=2026-11-10\non_device driver=m_small writer=m_small\n"
    );
    let no_writer = std::fs::read_to_string(case_dir("no_writer").join("expected/freeze.txt"))
        .expect("no_writer");
    assert_eq!(
        no_writer,
        "freeze rows=132 models=6 sources=2 date=2026-11-10\non_device driver=m_small writer=none\n"
    );
    let no_driver = std::fs::read_to_string(case_dir("no_driver").join("expected/freeze.txt"))
        .expect("no_driver");
    assert_eq!(
        no_driver,
        "freeze rows=1 models=1 sources=2 date=2026-11-10\non_device driver=none writer=none\n"
    );
    let failed = |case: &str| -> Vec<String> {
        std::fs::read_to_string(case_dir(case).join("expected/recorded.tsv"))
            .expect("recorded")
            .lines()
            .filter(|line| line.split('\t').nth(9) == Some("no"))
            .map(|line| line.split('\t').take(9).collect::<Vec<_>>().join(" "))
            .collect()
    };
    assert_eq!(
        failed("full"),
        ["order shop_order compile on_device m_tiny hosted-win 2 dev 1"]
    );
    assert_eq!(
        failed("no_writer"),
        [
            "order shop_order compile on_device m_tiny hosted-win 2 dev 1",
            "reply email write on_device m_small hosted-win 3 held 1"
        ]
    );
    assert_eq!(
        failed("no_driver"),
        ["bill utility_bill_payment recover on_device m_tiny hosted-win 3 held 1"]
    );
    let block = |case: &str, name: &str| -> Vec<String> {
        std::fs::read_to_string(case_dir(case).join(name))
            .expect("a golden input")
            .lines()
            .skip_while(|line| !line.starts_with("```"))
            .skip(1)
            .take_while(|line| *line != "```")
            .map(str::to_owned)
            .collect()
    };
    for case in ["full", "no_writer", "no_driver"] {
        for name in ["driver.md", "model-cells.md"] {
            let lines = block(case, name);
            assert!(
                lines.last().is_some_and(|line| line.starts_with("verdict")),
                "{case}/{name}: the block ends with its verdict line"
            );
        }
    }
    let count = |case: &str, head: &str, rule: &str| {
        block(case, "driver.md")
            .iter()
            .filter(|line| line.starts_with(head) && line.contains(rule))
            .count()
    };
    let passed = "compile=ok replays_ok=3 of=3 recover=ok";
    assert_eq!(count("full", "drive model=m_small ", passed), 12);
    assert_eq!(count("full", "reply model=m_small ", " bodies=2 "), 6);
    assert_eq!(count("no_writer", "drive model=m_small ", passed), 12);
}
