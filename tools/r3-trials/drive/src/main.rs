mod action;
mod body;
mod drive;
mod fixture;
mod lines;
mod model;
mod step;
mod words;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use drive::{Ctx, End, Prompts, Stop};
use lines::{Cell, Kind, Run};

const USAGE: &str = "r3-drive --repo DIR --model TOKEN --weights FILE --child FILE --settings FILE --task bill|order|reply --seed 1|2|3 --browser PATH --port N --machine NAME --run-dir DIR [--compile-variant NAME]";

#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    repo: PathBuf,
    model: String,
    weights: PathBuf,
    child: PathBuf,
    settings: PathBuf,
    task: String,
    seed: u8,
    browser: PathBuf,
    port: u16,
    machine: String,
    run_dir: PathBuf,
    /// A variant of the fixture for the compile run alone, which only the control passes (ruling
    /// CP-179, the Addition on H9 (c)); a cell of the trial passes none.
    compile_variant: Option<String>,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut seen: Vec<&str> = Vec::new();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let value = it.next().ok_or_else(|| format!("{flag} has no value"))?;
        if seen.contains(&flag.as_str()) {
            return Err(format!("{flag} is given twice"));
        }
        seen.push(flag.as_str());
        match flag.as_str() {
            "--repo" => a.repo = PathBuf::from(value),
            "--model" if action::is_name(value) && value != "none" && value != "synthetic" => {
                a.model = value.clone()
            }
            "--weights" => a.weights = PathBuf::from(value),
            "--child" => a.child = PathBuf::from(value),
            "--settings" => a.settings = PathBuf::from(value),
            "--task" if matches!(value.as_str(), "bill" | "order" | "reply") => {
                a.task = value.clone()
            }
            "--seed" if matches!(value.as_str(), "1" | "2" | "3") => {
                a.seed = value.parse().unwrap_or(1)
            }
            "--browser" => a.browser = PathBuf::from(value),
            "--port" => {
                a.port = value
                    .parse()
                    .ok()
                    .filter(|p| *p >= 1024)
                    .ok_or("--port takes 1024 to 65535")?
            }
            "--machine" if action::is_name(&value.replace('-', "_")) => a.machine = value.clone(),
            "--run-dir" => a.run_dir = PathBuf::from(value),
            "--compile-variant" if action::is_name(value) => {
                a.compile_variant = Some(value.clone())
            }
            "--model" | "--task" | "--seed" | "--machine" | "--compile-variant" => {
                return Err(format!("{flag} takes no {value}"))
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let wanted = [
        "--repo",
        "--model",
        "--weights",
        "--child",
        "--settings",
        "--task",
        "--seed",
        "--browser",
        "--port",
        "--machine",
        "--run-dir",
    ];
    if let Some(missing) = wanted.iter().find(|w| !seen.contains(w)) {
        return Err(format!("{missing} is missing"));
    }
    Ok(a)
}

/// The value of `key` in the level `medium` of the settings file of A12.5.
fn medium<T: std::str::FromStr>(settings: &str, key: &str) -> Option<T> {
    let mut section = "";
    for line in settings.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line;
        } else if section == "[medium]" {
            if let Some(v) = line.strip_prefix(key).and_then(|r| r.strip_prefix(" = ")) {
                return v.parse().ok();
            }
        }
    }
    None
}

/// `recoveries` of the level `medium`.
fn recoveries(settings: &str) -> Option<u32> {
    medium(settings, "recoveries")
}

fn out(line: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

/// The 1-minute load of the system from `sysctl -n vm.loadavg`, which prints `{ 1.23 4.56 7.89 }`.
fn load1() -> Option<f64> {
    let o = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "vm.loadavg"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    first_load(&String::from_utf8_lossy(&o.stdout))
}

fn first_load(text: &str) -> Option<f64> {
    text.split_whitespace().find(|w| *w != "{")?.parse().ok()
}

fn watch_signals() -> std::io::Result<Stop> {
    let (tx, rx) = tokio::sync::watch::channel(false);
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate())?;
        let mut int = signal(SignalKind::interrupt())?;
        tokio::spawn(async move {
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
            let _ = tx.send(true);
        });
    }
    #[cfg(not(unix))]
    {
        let mut c = tokio::signal::windows::ctrl_c()?;
        let mut b = tokio::signal::windows::ctrl_break()?;
        tokio::spawn(async move {
            tokio::select! {
                _ = c.recv() => {}
                _ = b.recv() => {}
            }
            let _ = tx.send(true);
        });
    }
    Ok(Stop(rx))
}

/// Ends the model children that this process started and that still run, by their recorded
/// number and creation time: what a signal leaves behind while a request is answered.
fn end_own_children(child: &std::path::Path, since_us: u64) -> usize {
    let me = std::process::id();
    let name = child.file_name();
    let table = r3_web::procs::table().unwrap_or_default();
    table
        .iter()
        .filter(|p| {
            p.ppid == me
                && p.started_us >= since_us
                && p.exe.as_deref().and_then(|e| e.file_name()) == name
        })
        .filter(|p| r3_web::procs::kill(p))
        .count()
}

fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

async fn cell(args: Args, ctx: Ctx) -> ExitCode {
    let cell = Cell {
        model: &args.model,
        machine: &args.machine,
        task: &args.task,
        seed: args.seed,
    };
    let since = now_us();
    let mut server = match fixture::Server::start(&args.repo, args.port, args.seed) {
        Ok(s) => s,
        Err(e) => {
            ctx.trace(&format!("fixture: {e}"));
            out(&lines::notrun(&cell, "fixture"));
            return ExitCode::from(1);
        }
    };
    ctx.trace(&format!("fixture pid={}", server.pid()));
    let before = |kind: Kind, i: u32| {
        if args.machine == "mac" {
            if let Some(l) = load1() {
                out(&lines::load(&cell, kind, i, l));
            }
        }
    };
    let result: Result<(), End> = async {
        if args.task == "reply" {
            before(Kind::Write, 1);
            let (run, bodies) = drive::write(&ctx).await?;
            out(&lines::attempt(&cell, &run));
            out(&lines::reply(&cell, bodies, &run));
            return Ok(());
        }
        let mut runs: Vec<Run> = Vec::new();
        before(Kind::Compile, 1);
        let (compiled, steps) = drive::compile(&ctx).await?;
        out(&lines::attempt(&cell, &compiled));
        runs.push(compiled);
        let playbook: Vec<String> = steps.iter().map(|s| s.to_json(&args.task)).collect();
        let text: String = playbook.iter().map(|l| format!("{l}\n")).collect();
        let _ = std::fs::write(ctx.run_dir.join("playbook.jsonl"), text);
        let parses = step::playbook_parses(&playbook, step::category(&args.task));
        for (kind, i) in [
            (Kind::Replay, 1),
            (Kind::Replay, 2),
            (Kind::Replay, 3),
            (Kind::Recover, 1),
        ] {
            let run = if parses {
                before(kind, i);
                drive::replay(&ctx, &steps, kind, i).await?
            } else {
                Run::missed(kind, i)
            };
            out(&lines::attempt(&cell, &run));
            runs.push(run);
        }
        out(&lines::drive(&cell, &runs));
        Ok(())
    }
    .await;
    let gone = server.stop();
    let ended = end_own_children(&args.child, since);
    ctx.trace(&format!(
        "stop fixture_gone={gone} model_children_ended={ended}"
    ));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(End::Signal) => {
            out(&lines::notrun(&cell, "signal"));
            ExitCode::from(1)
        }
        Err(End::NotRun(cause)) => {
            out(&lines::notrun(&cell, cause));
            ExitCode::from(1)
        }
    }
}

fn read(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("REFUSED usage: {e}; {USAGE}");
            return ExitCode::from(2);
        }
    };
    let prompts_dir = args.repo.join("tools/r3-trials/drive/prompts");
    let goal = read(&prompts_dir.join(format!("goal-{}.txt", args.task)));
    let boundary = read(&prompts_dir.join("boundary.txt"));
    let data = read(&args.repo.join(format!(
        "tools/fixture-server/variants/data/seed-{}.json",
        args.seed
    )))
    .and_then(|t| serde_json::from_str(&t).ok());
    let settings = read(&args.settings);
    let recoveries = settings.as_deref().and_then(recoveries);
    let reply_tokens = settings
        .as_deref()
        .and_then(|s| medium::<u64>(s, "reply_tokens"));
    let (Some(goal), Some(boundary), Some(data), Some(recoveries), Some(reply_tokens)) =
        (goal, boundary, data, recoveries, reply_tokens)
    else {
        eprintln!(
            "REFUSED usage: a prompt file, the seed file or the settings file cannot be read"
        );
        return ExitCode::from(2);
    };
    let trace = match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(args.run_dir.join("trace.txt"))
    {
        Ok(f) => f,
        Err(_) => {
            eprintln!("REFUSED usage: --run-dir must be a directory with no trace.txt");
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => {
            out(&lines::notrun(
                &Cell {
                    model: &args.model,
                    machine: &args.machine,
                    task: &args.task,
                    seed: args.seed,
                },
                "runtime",
            ));
            return ExitCode::from(1);
        }
    };
    rt.block_on(async move {
        let stop = match watch_signals() {
            Ok(s) => s,
            Err(_) => {
                out(&lines::notrun(
                    &Cell {
                        model: &args.model,
                        machine: &args.machine,
                        task: &args.task,
                        seed: args.seed,
                    },
                    "runtime",
                ));
                return ExitCode::from(1);
            }
        };
        let ctx = Ctx {
            port: args.port,
            seed: args.seed,
            data,
            task: args.task.clone(),
            browser: args.browser.clone(),
            run_dir: args.run_dir.clone(),
            model: model::Spec {
                program: args.child.clone(),
                weights: args.weights.clone(),
                settings: args.settings.clone(),
                stderr: args.run_dir.join("model-stderr.txt"),
            },
            prompts: Prompts { boundary, goal },
            recoveries,
            reply_tokens,
            compile_variants: args.compile_variant.clone().into_iter().collect(),
            stop,
            trace: std::sync::Mutex::new(trace),
            began: std::time::Instant::now(),
            clock: Box::new(drive::SystemClock),
        };
        cell(args, ctx).await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(extra: &[(&str, &str)]) -> Vec<String> {
        let mut base: Vec<(&str, &str)> = vec![
            ("--repo", "r"),
            ("--model", "minicpm5_2b"),
            ("--weights", "w"),
            ("--child", "c"),
            ("--settings", "s"),
            ("--task", "bill"),
            ("--seed", "2"),
            ("--browser", "b"),
            ("--port", "18090"),
            ("--machine", "hosted-win"),
            ("--run-dir", "d"),
        ];
        for (k, v) in extra {
            if let Some(slot) = base.iter_mut().find(|(b, _)| b == k) {
                slot.1 = v;
            } else {
                base.push((k, v));
            }
        }
        base.iter()
            .flat_map(|(k, v)| [k.to_string(), v.to_string()])
            .collect()
    }

    #[test]
    fn the_arguments_are_read_and_checked() {
        let a = parse(&argv(&[])).unwrap();
        assert_eq!(
            (a.seed, a.port, a.task.as_str(), a.machine.as_str()),
            (2, 18090, "bill", "hosted-win")
        );
        for bad in [
            ("--task", "fill"),
            ("--seed", "4"),
            ("--port", "80"),
            ("--port", "x"),
            ("--model", "none"),
            ("--model", "Big"),
            ("--other", "x"),
        ] {
            assert!(parse(&argv(&[bad])).is_err(), "{bad:?}");
        }
        let mut twice = argv(&[]);
        twice.extend(["--seed".to_string(), "1".to_string()]);
        assert!(parse(&twice).is_err());
        assert_eq!(parse(&argv(&[])).unwrap().compile_variant, None);
        assert_eq!(
            parse(&argv(&[("--compile-variant", "slow_charge")]))
                .unwrap()
                .compile_variant
                .as_deref(),
            Some("slow_charge")
        );
        assert!(parse(&argv(&[("--compile-variant", "Slow charge")])).is_err());
        let short: Vec<String> = argv(&[]).into_iter().take(20).collect();
        assert!(parse(&short).is_err());
    }

    #[test]
    fn the_load_is_the_first_figure_of_sysctl() {
        assert_eq!(first_load("{ 141.99 127.28 126.28 }\n"), Some(141.99));
        assert_eq!(first_load(""), None);
    }

    #[test]
    fn recoveries_come_from_the_level_medium() {
        let text = "driver = \"none\"\n[low]\nrecoveries = 1\n[medium]\naction_tokens = 256\nrecoveries = 2\n[high]\nrecoveries = 3\n";
        assert_eq!(recoveries(text), Some(2));
        assert_eq!(recoveries("[low]\nrecoveries = 1\n"), None);
        let text = "[medium]\nreply_tokens = 512\nreply_tokens_x = 3\n";
        assert_eq!(medium::<u64>(text, "reply_tokens"), Some(512));
        assert_eq!(
            medium::<u64>("[medium]\nreply_tokens_x = 3\n", "reply_tokens"),
            None
        );
    }
}
