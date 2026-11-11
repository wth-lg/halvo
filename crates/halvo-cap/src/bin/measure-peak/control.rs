use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ExitCode, Stdio};
use std::time::Duration;

use halvo_cap::{ChildSpec, Exit, Limit, Outcome, UnixNanos};

const EXTRA: u64 = 16_777_216;
const STEP: u64 = 1_048_576;
const WAIT_AFTER_ALL: Duration = Duration::from_secs(4);
const SIGKILL: i32 = 9;
const STOP_MS_BOUND: u64 = 2000;

static FILL: [u8; 65_536] = [0xa5; 65_536];

pub fn allocate_bytes(limit: Limit) -> Option<u64> {
    limit.bytes().checked_add(EXTRA)
}

fn refused(out: &mut impl Write, total: u64) -> ExitCode {
    let _ = writeln!(out, "refused allocated_bytes={total}");
    let _ = out.flush();
    ExitCode::SUCCESS
}

pub fn child(limit: Limit, target: u64) -> ExitCode {
    let mut out = std::io::stdout().lock();
    let _ = out.flush();
    let mut total = 0u64;
    let mut held: Vec<Vec<u8>> = Vec::new();
    let blocks = usize::try_from(target.div_ceil(STEP)).unwrap_or(usize::MAX);
    if held.try_reserve_exact(blocks).is_err() {
        return refused(&mut out, total);
    }
    let mut crossed = false;
    while total < target {
        let size = (target - total).min(STEP) as usize;
        let mut block: Vec<u8> = Vec::new();
        if block.try_reserve_exact(size).is_err() {
            return refused(&mut out, total);
        }
        while block.len() < size {
            let n = (size - block.len()).min(FILL.len());
            block.extend_from_slice(&FILL[..n]);
        }
        held.push(block);
        total += size as u64;
        if !crossed && total > limit.bytes() {
            crossed = true;
            let _ = writeln!(out, "crossed allocated_bytes={total}");
            let _ = out.flush();
        }
    }
    std::hint::black_box(&held);
    std::thread::sleep(WAIT_AFTER_ALL);
    let _ = writeln!(out, "completed allocated_bytes={total}");
    let _ = out.flush();
    ExitCode::SUCCESS
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub crossed_at: Option<UnixNanos>,
    pub refused: bool,
    pub completed: bool,
}

fn read_lines(reader: impl Read) -> Seen {
    let mut seen = Seen::default();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return seen,
            Ok(_) => {}
        }
        let at = UnixNanos::now();
        if line.starts_with("crossed ") && seen.crossed_at.is_none() {
            seen.crossed_at = Some(at);
        } else if line.starts_with("refused ") {
            seen.refused = true;
        } else if line.starts_with("completed ") {
            seen.completed = true;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    Refused,
    Stopped,
    Completed,
    Failed,
}

impl Word {
    pub fn as_str(self) -> &'static str {
        match self {
            Word::Refused => "refused",
            Word::Stopped => "stopped",
            Word::Completed => "completed",
            Word::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub word: Word,
    pub stop_ms: Option<u64>,
    pub peak_bytes: u64,
}

impl Verdict {
    pub fn ok(&self, windows: bool) -> bool {
        if windows {
            self.word == Word::Refused
        } else {
            self.word == Word::Stopped && self.stop_ms.is_some_and(|ms| ms <= STOP_MS_BOUND)
        }
    }

    fn failed() -> Verdict {
        Verdict {
            word: Word::Failed,
            stop_ms: None,
            peak_bytes: 0,
        }
    }
}

pub fn classify(windows: bool, outcome: &Outcome, seen: &Seen) -> Verdict {
    let verdict = |word, stop_ms| Verdict {
        word,
        stop_ms,
        peak_bytes: outcome.peak_bytes,
    };
    let ended_by_itself = matches!(outcome.exit, Exit::Code(_));
    if windows {
        if outcome.capped && seen.refused {
            return verdict(Word::Refused, None);
        }
    } else if outcome.exit == Exit::Signal(SIGKILL)
        && outcome.capped
        && !seen.refused
        && !seen.completed
    {
        let stop_ms = match seen.crossed_at {
            Some(at) if at < outcome.end => ((outcome.end.0 - at.0) / 1_000_000) as u64,
            _ => 0,
        };
        return verdict(Word::Stopped, Some(stop_ms));
    }
    if seen.completed && !seen.refused && ended_by_itself {
        return verdict(Word::Completed, None);
    }
    verdict(Word::Failed, None)
}

pub fn run(limit: Limit) -> Verdict {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("measure-peak: control: current_exe: {e}");
            return Verdict::failed();
        }
    };
    let (reader, writer) = match std::io::pipe() {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("measure-peak: control: pipe: {e}");
            return Verdict::failed();
        }
    };
    let mut spec = ChildSpec::new(
        exe,
        [
            "--control-child".to_string(),
            "--limit".to_string(),
            limit.bytes().to_string(),
        ],
    );
    spec.stdout = Stdio::from(writer);
    spec.stderr = Stdio::from(std::io::stderr());
    let reading = std::thread::spawn(move || read_lines(reader));
    let ran = halvo_cap::run(spec, limit);
    let seen = reading.join().unwrap_or_default();
    match ran {
        Ok(outcome) => classify(cfg!(windows), &outcome, &seen),
        Err(e) => {
            eprintln!("measure-peak: control: {e}");
            Verdict::failed()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: i64 = 1_000_000;

    fn ended(exit: Exit, capped: bool, end_ms: i64) -> Outcome {
        Outcome {
            exit,
            peak_bytes: 300,
            capped,
            start: UnixNanos(0),
            end: UnixNanos(end_ms * MS),
            mode: halvo_cap::Mode::Poll,
        }
    }

    fn seen(crossed_ms: Option<i64>, refused: bool, completed: bool) -> Seen {
        Seen {
            crossed_at: crossed_ms.map(|ms| UnixNanos(ms * MS)),
            refused,
            completed,
        }
    }

    #[test]
    fn on_macos_a_kill_within_two_seconds_of_the_crossing_is_ok() {
        let v = classify(
            false,
            &ended(Exit::Signal(9), true, 2100),
            &seen(Some(100), false, false),
        );
        assert_eq!(
            (v.word, v.stop_ms, v.ok(false)),
            (Word::Stopped, Some(2000), true)
        );
        let v = classify(
            false,
            &ended(Exit::Signal(9), true, 2101),
            &seen(Some(100), false, false),
        );
        assert_eq!(
            (v.word, v.stop_ms, v.ok(false)),
            (Word::Stopped, Some(2001), false)
        );
    }

    #[test]
    fn on_macos_a_kill_before_the_crossing_was_read_has_stop_ms_zero() {
        let v = classify(
            false,
            &ended(Exit::Signal(9), true, 50),
            &seen(None, false, false),
        );
        assert_eq!(
            (v.word, v.stop_ms, v.ok(false)),
            (Word::Stopped, Some(0), true)
        );
        let v = classify(
            false,
            &ended(Exit::Signal(9), true, 50),
            &seen(Some(60), false, false),
        );
        assert_eq!((v.word, v.stop_ms), (Word::Stopped, Some(0)));
    }

    #[test]
    fn on_macos_a_kill_that_follows_no_read_above_the_limit_is_a_miss() {
        let v = classify(
            false,
            &ended(Exit::Signal(9), false, 300),
            &seen(Some(100), false, false),
        );
        assert_eq!(
            (v.word, v.stop_ms, v.ok(false)),
            (Word::Failed, None, false)
        );
    }

    #[test]
    fn a_control_that_completes_or_dies_otherwise_is_a_miss() {
        let cases = [
            (
                ended(Exit::Code(0), false, 4300),
                seen(Some(100), false, true),
                Word::Completed,
            ),
            (
                ended(Exit::Code(0), true, 4300),
                seen(Some(100), false, true),
                Word::Completed,
            ),
            (
                ended(Exit::Signal(15), false, 300),
                seen(Some(100), false, false),
                Word::Failed,
            ),
            (
                ended(Exit::Signal(9), true, 300),
                seen(Some(100), true, false),
                Word::Failed,
            ),
            (
                ended(Exit::Signal(9), true, 4300),
                seen(Some(100), false, true),
                Word::Failed,
            ),
            (
                ended(Exit::Code(0), false, 300),
                seen(Some(100), false, false),
                Word::Failed,
            ),
            (
                ended(Exit::NotStarted, false, 0),
                seen(None, false, false),
                Word::Failed,
            ),
            (
                ended(Exit::Code(0), false, 300),
                seen(None, true, false),
                Word::Failed,
            ),
        ];
        for (outcome, seen, word) in cases {
            let v = classify(false, &outcome, &seen);
            assert_eq!(v.word, word, "{outcome:?} {seen:?}");
            assert!(!v.ok(false), "{outcome:?} {seen:?}");
        }
    }

    #[test]
    fn on_windows_only_a_refusal_that_the_job_reported_is_ok() {
        let v = classify(
            true,
            &ended(Exit::Code(0), true, 300),
            &seen(None, true, false),
        );
        assert_eq!((v.word, v.stop_ms, v.ok(true)), (Word::Refused, None, true));
        let cases = [
            (
                ended(Exit::Code(0), false, 300),
                seen(None, true, false),
                Word::Failed,
            ),
            (
                ended(Exit::Code(0), true, 300),
                seen(None, false, false),
                Word::Failed,
            ),
            (
                ended(Exit::Code(0), false, 4300),
                seen(Some(100), false, true),
                Word::Completed,
            ),
            (
                ended(Exit::Code(1), true, 300),
                seen(None, false, false),
                Word::Failed,
            ),
        ];
        for (outcome, seen, word) in cases {
            let v = classify(true, &outcome, &seen);
            assert_eq!(v.word, word, "{outcome:?} {seen:?}");
            assert!(!v.ok(true), "{outcome:?} {seen:?}");
        }
    }

    #[test]
    fn the_lines_of_the_child_are_read_with_the_time_of_the_crossing() {
        let text =
            b"crossed allocated_bytes=2\ncrossed allocated_bytes=3\nrefused allocated_bytes=3\n";
        let seen = read_lines(&text[..]);
        assert!(seen.crossed_at.is_some());
        assert!(seen.refused);
        assert!(!seen.completed);
        let seen = read_lines(&b"completed allocated_bytes=5\n"[..]);
        assert_eq!(
            (seen.crossed_at, seen.refused, seen.completed),
            (None, false, true)
        );
    }
}
