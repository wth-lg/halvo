//! S6 driver. Hosts the loopback stub, runs the helper's untimed first run, the control
//! run, the trust command and the timed runs, then prints the rule line.
//!
//! Exit 0 with a MET or MISSED line. Exit 1 when the line does not count.

use r7_s5_spike::procstat::{self, Measured};
use r7_s5_spike::random_u64;
use r7_s5_spike::stats::summarize;
use r7_s5_spike::stub::{self, Entry, Stub};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

const USAGE: &str = "usage: bench --helper <path> --workdir <dir> --runner <label> \
     [--runs 30] [--limit-ms 1000] -- <trust command; {ca} and {leaf} become the certificate files>";

struct Options {
    helper: PathBuf,
    workdir: PathBuf,
    runner: String,
    runs: usize,
    limit: Duration,
    trust: Vec<String>,
}

fn options() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let (mut helper, mut workdir, mut runner) = (None, None, None);
    let (mut runs, mut limit_ms) = (30usize, 1000u64);
    let mut trust = Vec::new();
    while let Some(flag) = args.next() {
        if flag == "--" {
            trust = args.by_ref().collect();
            break;
        }
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--helper" => helper = Some(PathBuf::from(value)),
            "--workdir" => workdir = Some(PathBuf::from(value)),
            "--runner" => runner = Some(value),
            "--runs" => runs = value.parse().map_err(|_| "--runs needs a number")?,
            "--limit-ms" => limit_ms = value.parse().map_err(|_| "--limit-ms needs a number")?,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    if trust.is_empty() || runs == 0 {
        return Err("a trust command and at least one run are needed".to_string());
    }
    Ok(Options {
        helper: helper.ok_or("--helper is needed")?,
        workdir: workdir.ok_or("--workdir is needed")?,
        runner: runner.ok_or("--runner is needed")?,
        runs,
        limit: Duration::from_millis(limit_ms),
        trust,
    })
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(options) => options,
        Err(problem) => {
            eprintln!("{problem}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match measure(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => {
            println!("S6 RULE {}: DOES NOT COUNT ({reason})", options.runner);
            ExitCode::FAILURE
        }
    }
}

fn helper(options: &Options, args: &[&str]) -> Result<Measured, String> {
    procstat::run(Command::new(&options.helper).args(args))
        .map_err(|error| format!("could not run {}: {error}", options.helper.display()))
}

/// The stub's entries after `seen`, waiting up to five seconds for the first one.
fn entries_after(stub: &Stub, seen: usize) -> Vec<Entry> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let entries = stub.entries();
        if entries.len() > seen || Instant::now() >= deadline {
            return entries.into_iter().skip(seen).collect();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn phases(stdout: &str) -> Option<HashMap<&str, f64>> {
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix("phases_us "))?;
    line.split_whitespace()
        .map(|pair| {
            let (name, micros) = pair.split_once('=')?;
            Some((name, micros.parse::<f64>().ok()? / 1000.0))
        })
        .collect()
}

fn median(sample: &[f64]) -> f64 {
    summarize(sample).map_or(f64::NAN, |summary| summary.median)
}

fn measure(options: &Options) -> Result<(), String> {
    let pki = stub::generate_pki().map_err(|error| format!("no test CA: {error}"))?;
    let ca_path = options.workdir.join("s6-test-ca.pem");
    let leaf_path = options.workdir.join("s6-leaf.pem");
    let record_path = options.workdir.join("s6-stub-record.txt");
    std::fs::write(&ca_path, &pki.ca_pem)
        .and_then(|()| std::fs::write(&leaf_path, &pki.leaf_pem))
        .map_err(|error| format!("could not write the certificates: {error}"))?;
    let stub = Stub::start(&pki, Some(&record_path))
        .map_err(|error| format!("could not start the stub: {error}"))?;
    let url = stub.url("/v1/ping");
    println!(
        "S6 stub: {url} listening on {:?}, test CA {:?} in {}",
        stub.listening,
        pki.ca_name,
        ca_path.display()
    );

    let item = format!("synthetic-s6-item-{:016x}", random_u64());
    let first = helper(options, &["store", &item])?;
    print!("{}", first.stdout);
    if first.exit_code != Some(0) {
        return Err("the helper's first run could not store its item".to_string());
    }
    println!("S6 first run (untimed): the helper stored its item");

    let seen = stub.entries().len();
    let control = helper(options, &["run", &url, "control"])?;
    let stub_saw = entries_after(&stub, seen);
    println!(
        "S6 control (before the runner trusts the test CA): exit={:?} stub_saw={stub_saw:?} helper_said={:?}",
        control.exit_code,
        control.stdout.trim()
    );
    if control.exit_code == Some(0) || stub.bearer_of("control").is_some() {
        return Err("the control run did not fail before the CA was trusted".to_string());
    }
    let ended_at_handshake = !stub_saw.is_empty()
        && stub_saw
            .iter()
            .all(|entry| matches!(entry, Entry::NoRequest(_)));
    if !ended_at_handshake {
        return Err(
            "the control run did not end at the stub's TLS handshake, so its failure shows no trust decision"
                .to_string(),
        );
    }
    println!("S6 control: failed at the TLS handshake, as required");

    let trust: Vec<String> = options
        .trust
        .iter()
        .map(|arg| match arg.as_str() {
            "{ca}" => ca_path.display().to_string(),
            "{leaf}" => leaf_path.display().to_string(),
            _ => arg.clone(),
        })
        .collect();
    println!("S6 trust: {}", trust.join(" "));
    let trusted = Command::new(&trust[0])
        .args(&trust[1..])
        .status()
        .map_err(|error| format!("could not run the trust command: {error}"))?;
    if !trusted.success() {
        return Err(format!("the trust command ended with {trusted}"));
    }

    let mut ok = 0;
    let (mut wall_ms, mut rss_mib) = (Vec::new(), Vec::new());
    let mut phase_ms: HashMap<&str, Vec<f64>> = HashMap::new();
    let mut outside_main_ms = Vec::new();
    let outputs: Vec<(String, Measured)> = (1..=options.runs)
        .map(|index| {
            let run_id = format!("run-{index:02}");
            let measured = helper(options, &["run", &url, &run_id])?;
            Ok((run_id, measured))
        })
        .collect::<Result<_, String>>()?;
    for (run_id, measured) in &outputs {
        let recorded = stub.bearer_of(run_id);
        let run_ok = measured.exit_code == Some(0) && recorded.as_deref() == Some(item.as_str());
        ok += usize::from(run_ok);
        let wall = measured.wall.as_secs_f64() * 1000.0;
        let rss = measured.peak_rss_bytes as f64 / (1024.0 * 1024.0);
        wall_ms.push(wall);
        rss_mib.push(rss);
        let detail = match phases(&measured.stdout) {
            Some(run_phases) => {
                for (name, value) in &run_phases {
                    phase_ms.entry(name).or_default().push(*value);
                }
                if let Some(total) = run_phases.get("total") {
                    outside_main_ms.push(wall - total);
                }
                measured.stdout.trim().to_string()
            }
            None => format!(
                "exit={:?} in_stub_record={} helper_said={:?}",
                measured.exit_code,
                recorded.is_some(),
                measured.stdout.trim()
            ),
        };
        println!(
            "S6 {run_id}: {} wall_ms={wall:.1} peak_rss_mib={rss:.1} {detail}",
            if run_ok { "ok" } else { "NOT OK" }
        );
    }

    println!("S6 stub record ({}):", record_path.display());
    print!(
        "{}",
        std::fs::read_to_string(&record_path).unwrap_or_default()
    );

    let runs = options.runs;
    let wall = summarize(&wall_ms).ok_or("no runs")?;
    let rss = summarize(&rss_mib).ok_or("no runs")?;
    println!(
        "S6 summary runner={} runs={runs} ok={ok}/{runs}",
        options.runner
    );
    println!(
        "S6 wall time per run in ms, from just before the spawn to the reaped exit, on the driver's monotonic clock: min={:.1} median={:.1} p90={:.1} max={:.1}",
        wall.min, wall.median, wall.p90, wall.max
    );
    println!(
        "S6 peak RSS per run in MiB: min={:.1} median={:.1} p90={:.1} max={:.1}",
        rss.min, rss.median, rss.p90, rss.max
    );
    let phase = |name: &str| median(phase_ms.get(name).map_or(&[][..], Vec::as_slice));
    println!(
        "S6 median ms inside the helper's main: key-store read={:.1} client build={:.1} request={:.1} total={:.1}; outside main (spawn, load, exit)={:.1}",
        phase("read"),
        phase("client"),
        phase("request"),
        phase("total"),
        median(&outside_main_ms)
    );

    let worst = wall.max / 1000.0;
    let limit = options.limit.as_secs_f64();
    let verdict = if ok == runs && worst <= limit {
        "MET"
    } else {
        "MISSED"
    };
    println!(
        "S6 RULE {}: {verdict} (ok={ok}/{runs}, worst wall time {worst:.3} s, limit {limit:.3} s, control failed before trust as required)",
        options.runner
    );

    let removed = helper(options, &["delete"])?;
    print!("{}", removed.stdout);
    Ok(())
}
