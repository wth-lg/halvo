use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use halvo_cap::{check_supported, run, ChildSpec, Exit, Limit, Outcome};

const ROLE: &str = "HALVO_CAP_CHILD";
const MIB: u64 = 1_048_576;
const REFUSED: i32 = 7;

static FILL: [u8; 65_536] = [0xa5; 65_536];

fn allocate(blocks: u64, block_mib: u64) -> Vec<Vec<u8>> {
    let size = (block_mib * MIB) as usize;
    let mut held: Vec<Vec<u8>> = Vec::new();
    if held.try_reserve_exact(blocks as usize).is_err() {
        std::process::exit(REFUSED);
    }
    for _ in 0..blocks {
        let mut block: Vec<u8> = Vec::new();
        if block.try_reserve_exact(size).is_err() {
            std::process::exit(REFUSED);
        }
        while block.len() < size {
            let n = (size - block.len()).min(FILL.len());
            block.extend_from_slice(&FILL[..n]);
        }
        held.push(block);
    }
    std::hint::black_box(held)
}

#[test]
#[ignore = "a child process that the other tests start"]
fn child() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let words: Vec<&str> = role.split(' ').collect();
    let number = |i: usize| -> u64 { words[i].parse().unwrap() };
    match words[0] {
        "hold" => {
            let held = allocate(number(1), 1);
            std::thread::sleep(Duration::from_millis(number(2)));
            drop(held);
        }
        "spike" => {
            drop(allocate(1, number(1)));
            std::thread::sleep(Duration::from_millis(number(2)));
        }
        "exit" => std::process::exit(number(1) as i32),
        "echo" => println!("hello from the child"),
        "grandchild" => {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(child_args())
                .env(ROLE, format!("hold {} {}", words[1], words[2]))
                .status()
                .unwrap();
            std::process::exit(status.code().unwrap_or(8));
        }
        _ => std::process::exit(6),
    }
    std::process::exit(0);
}

fn child_args() -> [&'static str; 5] {
    ["--ignored", "--exact", "child", "--nocapture", "--quiet"]
}

fn spec(role: &str) -> ChildSpec {
    let mut spec = ChildSpec::new(std::env::current_exe().unwrap(), child_args());
    spec.env.push((OsString::from(ROLE), OsString::from(role)));
    spec.stdout = Stdio::null();
    spec
}

fn limit(mib: u64) -> Limit {
    Limit::new(mib * MIB).unwrap()
}

fn elapsed_ms(outcome: &Outcome) -> i64 {
    (outcome.end.0 - outcome.start.0) / 1_000_000
}

#[test]
fn the_cap_is_supported_here() {
    check_supported(limit(256)).unwrap();
}

#[test]
fn a_limit_is_above_zero() {
    assert_eq!(Limit::new(0), None);
    assert_eq!(Limit::new(1).map(Limit::bytes), Some(1));
}

#[test]
fn a_child_under_the_limit_runs_to_its_end() {
    let outcome = run(spec("hold 32 600"), limit(256)).unwrap();
    assert_eq!(outcome.exit, Exit::Code(0), "{outcome:?}");
    assert!(!outcome.capped, "{outcome:?}");
    assert!(outcome.peak_bytes >= 32 * MIB, "{outcome:?}");
    assert!(outcome.peak_bytes <= 256 * MIB, "{outcome:?}");
    assert!(elapsed_ms(&outcome) >= 600, "{outcome:?}");
}

#[test]
fn a_child_that_ends_before_the_first_poll_has_its_peak_read() {
    let outcome = run(spec("hold 64 0"), limit(256)).unwrap();
    assert_eq!(outcome.exit, Exit::Code(0), "{outcome:?}");
    assert!(outcome.peak_bytes >= 64 * MIB, "{outcome:?}");
    assert!(!outcome.capped, "{outcome:?}");
}

#[test]
fn a_child_that_ends_above_the_limit_before_the_first_poll_is_capped() {
    let outcome = run(spec("hold 96 0"), limit(64)).unwrap();
    assert!(outcome.capped, "{outcome:?}");
    if cfg!(windows) {
        assert_eq!(outcome.exit, Exit::Code(REFUSED), "{outcome:?}");
    } else {
        assert!(outcome.peak_bytes > 64 * MIB, "{outcome:?}");
    }
}

#[test]
fn a_child_above_the_limit_is_stopped() {
    let outcome = run(spec("hold 128 20000"), limit(64)).unwrap();
    assert!(outcome.capped, "{outcome:?}");
    assert!(elapsed_ms(&outcome) < 15_000, "{outcome:?}");
    if cfg!(windows) {
        assert_eq!(outcome.exit, Exit::Code(REFUSED), "{outcome:?}");
        assert!(outcome.peak_bytes <= 64 * MIB, "{outcome:?}");
    } else {
        assert_eq!(outcome.exit, Exit::Signal(9), "{outcome:?}");
        assert!(outcome.peak_bytes > 64 * MIB, "{outcome:?}");
    }
}

#[test]
fn a_spike_between_two_reads_counts_in_the_peak() {
    let outcome = run(spec("spike 96 1500"), limit(256)).unwrap();
    assert_eq!(outcome.exit, Exit::Code(0), "{outcome:?}");
    assert!(!outcome.capped, "{outcome:?}");
    assert!(outcome.peak_bytes >= 96 * MIB, "{outcome:?}");
}

#[test]
fn a_spike_above_the_limit_is_stopped() {
    let outcome = run(spec("spike 96 20000"), limit(64)).unwrap();
    assert!(outcome.capped, "{outcome:?}");
    assert!(elapsed_ms(&outcome) < 15_000, "{outcome:?}");
    if cfg!(windows) {
        assert_eq!(outcome.exit, Exit::Code(REFUSED), "{outcome:?}");
    } else {
        assert_eq!(outcome.exit, Exit::Signal(9), "{outcome:?}");
    }
}

#[test]
fn the_exit_code_passes_through() {
    let outcome = run(spec("exit 3"), limit(256)).unwrap();
    assert_eq!(outcome.exit, Exit::Code(3), "{outcome:?}");
    assert!(!outcome.capped, "{outcome:?}");
    assert!(outcome.start <= outcome.end, "{outcome:?}");
}

#[test]
fn a_program_that_cannot_start_is_not_started() {
    let missing = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-program");
    let outcome = run(ChildSpec::new(missing, ["x"]), limit(256)).unwrap();
    assert_eq!(outcome.exit, Exit::NotStarted);
    assert_eq!(outcome.peak_bytes, 0);
    assert!(!outcome.capped);
    assert_eq!(outcome.start, outcome.end);
}

#[test]
fn the_standard_output_is_the_one_the_caller_gives() {
    let (mut reader, writer) = std::io::pipe().unwrap();
    let mut spec = spec("echo");
    spec.stdout = Stdio::from(writer);
    let read = std::thread::spawn(move || {
        let mut text = String::new();
        reader.read_to_string(&mut text).unwrap();
        text
    });
    let outcome = run(spec, limit(256)).unwrap();
    assert_eq!(outcome.exit, Exit::Code(0), "{outcome:?}");
    let text = read.join().unwrap();
    assert!(text.lines().any(|l| l == "hello from the child"), "{text}");
}

#[test]
fn the_environment_is_the_one_the_caller_gives() {
    let mut spec = spec("exit 0");
    spec.env.retain(|(name, _)| name != ROLE);
    spec.env
        .push((OsString::from(ROLE), OsString::from("exit 4")));
    let outcome = run(spec, limit(256)).unwrap();
    assert_eq!(outcome.exit, Exit::Code(4), "{outcome:?}");
}

#[cfg(windows)]
#[test]
fn a_grandchild_above_the_limit_is_held_by_the_job() {
    let outcome = run(spec("grandchild 128 20000"), limit(64)).unwrap();
    assert!(outcome.capped, "{outcome:?}");
    assert_eq!(outcome.exit, Exit::Code(REFUSED), "{outcome:?}");
}
