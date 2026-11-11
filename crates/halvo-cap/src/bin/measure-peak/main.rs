mod args;
mod control;
mod report;

use std::ffi::OsString;
use std::io::Write;
use std::process::{ExitCode, Stdio};

use halvo_cap::{ChildSpec, Exit, Limit, Outcome, UnixNanos, PLATFORM};

use args::Form;

fn refuse(cause: &str, detail: &str) -> ExitCode {
    eprintln!("REFUSED {cause}: {detail}");
    ExitCode::from(2)
}

fn status(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn measure(limit: Limit, runs: u64, command: Vec<OsString>) -> ExitCode {
    let mode = match halvo_cap::check_supported(limit) {
        Ok(mode) => mode,
        Err(e) => return refuse("cap-unsupported", &e.to_string()),
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(
        out,
        "measure-peak platform={PLATFORM} mode={} limit_bytes={} runs={runs}",
        mode.as_str(),
        limit.bytes()
    );
    let _ = out.flush();
    let mut outcomes = Vec::new();
    for n in 1..=runs {
        let mut spec = ChildSpec::new(command[0].clone(), command[1..].iter().cloned());
        spec.stdout = Stdio::from(std::io::stderr());
        spec.stderr = Stdio::from(std::io::stderr());
        let outcome = halvo_cap::run(spec, limit).unwrap_or_else(|e| {
            eprintln!("measure-peak: run {n}: {e}");
            let at = UnixNanos::now();
            Outcome {
                exit: Exit::NotStarted,
                peak_bytes: 0,
                capped: false,
                start: at,
                end: at,
                mode,
            }
        });
        let _ = writeln!(out, "{}", report::run_line(n, &outcome));
        let _ = out.flush();
        outcomes.push(outcome);
    }
    let peaks: Vec<u64> = outcomes.iter().map(|o| o.peak_bytes).collect();
    let checks = report::Checks::of(&outcomes, limit);
    let _ = writeln!(out, "median peak_bytes={}", report::median(&peaks));
    let _ = writeln!(out, "{}", checks.line());
    let _ = out.flush();
    measure_status(&checks)
}

fn measure_status(checks: &report::Checks) -> ExitCode {
    status(checks.all_ok())
}

fn control_checks_line(ok: bool) -> String {
    format!("checks control={}", report::word(ok))
}

fn control(limit: Limit) -> ExitCode {
    let Some(allocate) = control::allocate_bytes(limit) else {
        return refuse("usage", "--limit plus 16 MiB is above 2^64 - 1");
    };
    let mode = match halvo_cap::check_supported(limit) {
        Ok(mode) => mode,
        Err(e) => return refuse("cap-unsupported", &e.to_string()),
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(
        out,
        "measure-peak platform={PLATFORM} mode={} limit_bytes={} control=yes",
        mode.as_str(),
        limit.bytes()
    );
    let _ = out.flush();
    let verdict = control::run(limit);
    let stop_ms = verdict
        .stop_ms
        .map_or_else(|| "-".to_string(), |ms| ms.to_string());
    let ok = verdict.ok(cfg!(windows));
    let _ = writeln!(
        out,
        "control allocate_bytes={allocate} outcome={} stop_ms={stop_ms} peak_bytes={}",
        verdict.word.as_str(),
        verdict.peak_bytes
    );
    let _ = writeln!(out, "{}", control_checks_line(ok));
    let _ = out.flush();
    status(ok)
}

fn main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args::parse(&argv) {
        Err(detail) => refuse("usage", &detail),
        Ok(Form::ControlChild { limit }) => match control::allocate_bytes(limit) {
            Some(target) => control::child(limit, target),
            None => refuse("usage", "--limit plus 16 MiB is above 2^64 - 1"),
        },
        Ok(Form::Control { limit }) => control(limit),
        Ok(Form::Measure {
            limit,
            runs,
            command,
        }) => measure(limit, runs, command),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_of_a_measurement_needs_every_check() {
        for (exit, cap, median) in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
        ] {
            let checks = report::Checks { exit, cap, median };
            assert_eq!(
                measure_status(&checks),
                ExitCode::from(1),
                "{exit} {cap} {median}"
            );
        }
        let checks = report::Checks {
            exit: true,
            cap: true,
            median: true,
        };
        assert_eq!(measure_status(&checks), ExitCode::SUCCESS);
    }

    #[test]
    fn the_checks_line_of_the_control_follows_its_verdict() {
        assert_eq!(control_checks_line(true), "checks control=ok");
        assert_eq!(control_checks_line(false), "checks control=miss");
    }
}
