//! The wall that keeps the browser's request maker out of a build that a person uses (r3 U5.10 cut A): a runner
//! built with the feature `loopback` holds the maker's own marker text, and check 62-build refuses a shipped file
//! that holds it.

use std::path::Path;
use std::process::Command;

/// The marker text of the browser's request maker (crate::web::maker), one line of the list `markers` of
/// scripts/r1/ci.d/62-build.sh.
const MARKER: &str = "halvo-runner:web-maker:this-build-has-it!";

/// Builds the runner's program as check 62-build builds a shipped one (`cargo build --release --locked -p
/// halvo-runner --bin halvo-runner`) plus the feature `loopback`, which no shipped build names, into a target
/// directory of its own under this target's temporary directory, so that the binary which the runner's other test
/// targets start stays as cargo built it for them (the coordinator's rule on a test that builds another program
/// inside itself). The built file holds the maker's marker, and the list `markers` of check 62-build holds the text.
#[test]
fn a_runner_built_with_the_feature_holds_the_maker_s_marker() {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("maker-marker");
    let built = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "halvo-runner",
            "--bin",
            "halvo-runner",
            "--features",
            "loopback",
        ])
        .arg("--target-dir")
        .arg(&target)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo runs");
    assert!(
        built.status.success(),
        "cargo did not build the runner with the feature loopback: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let path = target
        .join("release")
        .join(format!("halvo-runner{}", std::env::consts::EXE_SUFFIX));
    let binary = std::fs::read(&path).expect("the built runner reads");
    let marker = MARKER.as_bytes();
    assert!(
        binary.windows(marker.len()).any(|bytes| bytes == marker),
        "the runner built with the feature loopback at {} holds no marker {MARKER} of the browser's request maker",
        path.display()
    );
    let check = include_str!("../../../scripts/r1/ci.d/62-build.sh");
    let markers = check
        .split_once("\nmarkers='")
        .and_then(|(_, rest)| rest.split_once('\''))
        .map(|(list, _)| list)
        .expect("check 62-build holds its list markers");
    assert!(
        markers.lines().any(|line| line == MARKER),
        "the list markers of check 62-build holds no line {MARKER}: {markers}"
    );
}
