//! verifier-46: the forced and not-forced ends of a runner that exits by itself, looped, counts alone.
//! A runner that exits with 1 and leaves nothing must give `forced` none; one that leaves a child in its job
//! (Windows) or group (macOS) must give `link_end` without the browser; one that leaves its browser, `link_end`
//! with the browser. The lines go to the standard error handle directly, past libtest's capture.
//! V46_SETTLE_RUNS sets the runs of each case (default 250).
mod common;

use std::io::Write;
use std::time::Duration;

use common::Rig;
use halvo_agent_client::{Forced, ForcedPath};

fn case(rig: &Rig, lines: &[&str]) -> (Option<Forced>, bool) {
    let go = rig.dir.join(format!("go-{}", common::run_id()));
    let wait = format!("wait-for {}", go.display());
    let lines: Vec<&str> = lines
        .iter()
        .map(|l| if *l == "GO" { wait.as_str() } else { l })
        .collect();
    let script = rig.script(&lines);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    if rig.sup.start(spec).is_err() {
        return (None, false);
    }
    std::fs::write(&go, b"").unwrap();
    match rig.rec.wait_end(&run, Duration::from_secs(30)) {
        Some(e) => (e.forced, true),
        None => {
            let _ = rig.sup.kill(&run);
            (None, false)
        }
    }
}

#[test]
fn v46_settle_counts() {
    let n: usize = std::env::var("V46_SETTLE_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(250);
    let rig = Rig::new("v46-settle");
    let mut ok = true;
    for (label, lines, want) in [
        ("nothing-left", vec!["hello", "GO", "exit 1"], None),
        (
            "child-left",
            vec!["hello", "child", "GO", "exit 1"],
            Some(Forced {
                path: ForcedPath::LinkEnd,
                browser: false,
            }),
        ),
        (
            "browser-left",
            vec!["hello", "on-eof hang", "browser", "GO", "exit 0"],
            Some(Forced {
                path: ForcedPath::LinkEnd,
                browser: true,
            }),
        ),
    ] {
        let runs = if label == "browser-left" { n / 5 } else { n };
        let (mut same, mut other, mut unended) = (0, 0, 0);
        let mut seen = std::collections::BTreeMap::new();
        for _ in 0..runs {
            let (f, ended) = case(&rig, &lines);
            if !ended {
                unended += 1;
            } else if f == want {
                same += 1;
            } else {
                other += 1;
                *seen.entry(format!("{f:?}")).or_insert(0) += 1;
            }
        }
        ok &= other == 0 && unended == 0;
        let _ = writeln!(
            std::io::stderr().lock(),
            "v46 settle {label} runs={runs} as_expected={same} other={other} unended={unended} others={seen:?}"
        );
    }
    let _ = writeln!(
        std::io::stderr().lock(),
        "v46 settle all_as_expected={}",
        if ok { "yes" } else { "no" }
    );
}
