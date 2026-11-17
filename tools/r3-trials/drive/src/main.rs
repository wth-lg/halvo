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

    #[test]
    fn the_fence_is_on_in_this_test_target() {
        assert!(
            r3_web::procs::fence::is_on(),
            "the fence is off in the test target of r3-drive"
        );
    }

    struct Tree {
        manifest: String,
        files: Vec<(String, String)>,
    }

    fn rust_files(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                rust_files(root, &path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let relative = path.strip_prefix(root).expect("a path below the root");
                let name: Vec<String> = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                let text = std::fs::read_to_string(&path).expect("a source file");
                out.push((name.join("/"), text));
            }
        }
    }

    fn tree_of(root: &std::path::Path) -> Tree {
        let mut files = Vec::new();
        for dir in ["src", "tests", "examples", "benches"] {
            rust_files(root, &root.join(dir), &mut files);
        }
        files.sort();
        Tree {
            manifest: std::fs::read_to_string(root.join("Cargo.toml")).expect("the manifest"),
            files,
        }
    }

    const SECTIONS: [&str; 6] = [
        "[package]",
        "[workspace]",
        "[[bin]]",
        "[dependencies]",
        "[dev-dependencies]",
        "[profile.release]",
    ];
    const PACKAGE_KEYS: [&str; 5] = ["name", "version", "description", "edition", "publish"];

    fn manifest_targets(manifest: &str) -> Result<Vec<String>, String> {
        let mut section = "";
        let mut paths = Vec::new();
        for line in manifest.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                if !SECTIONS.contains(&line) {
                    return Err(format!("a table that the lint does not read: {line}"));
                }
                section = line;
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("a line that the lint does not read: {line}"));
            };
            let (key, value) = (key.trim(), value.trim());
            match section {
                "[package]" if !PACKAGE_KEYS.contains(&key) => {
                    return Err(format!("a package key that the lint does not read: {key}"));
                }
                "[workspace]" => {
                    return Err(format!(
                        "a workspace key that the lint does not read: {key}"
                    ));
                }
                "[[bin]]" => match key {
                    "name" => {}
                    "path" => paths.push(value.trim_matches('"').to_string()),
                    _ => return Err(format!("a target key that the lint does not read: {key}")),
                },
                _ => {}
            }
        }
        Ok(paths)
    }

    fn target_roots(tree: &Tree) -> Result<Vec<String>, String> {
        let mut roots = manifest_targets(&tree.manifest)?;
        for (path, _) in &tree.files {
            let parts: Vec<&str> = path.split('/').collect();
            let root = match parts.as_slice() {
                ["src", "main.rs"] | ["src", "lib.rs"] => true,
                ["src", "bin", file] | ["tests", file] => file.ends_with(".rs"),
                ["src", "bin", _, "main.rs"] | ["tests", _, "main.rs"] => true,
                _ => false,
            };
            if root {
                roots.push(path.clone());
            }
        }
        roots.sort();
        roots.dedup();
        match roots
            .iter()
            .find(|r| !tree.files.iter().any(|(p, _)| p == *r))
        {
            Some(missing) => Err(format!("a target path that does not stand: {missing}")),
            None => Ok(roots),
        }
    }

    fn declares_r3_web(source: &str) -> bool {
        let name = concat!("r3", "_web");
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        source.match_indices(name).any(|(i, _)| {
            let (before, after) = (&source[..i], &source[i + name.len()..]);
            if before.chars().next_back().is_some_and(ident)
                || after.chars().next().is_some_and(ident)
            {
                return false;
            }
            let word: String = before
                .trim_end()
                .chars()
                .rev()
                .take_while(|c| ident(*c) || *c == '!')
                .collect::<Vec<char>>()
                .into_iter()
                .rev()
                .collect();
            [
                "mod",
                "struct",
                "enum",
                "fn",
                "const",
                "static",
                "type",
                "trait",
                "union",
                "as",
                "macro_rules!",
            ]
            .contains(&word.as_str())
                || ((before.trim_end().ends_with("::") || before.trim_end().ends_with(['{', ',']))
                    && after.trim_start().starts_with([';', ',', '}']))
        })
    }

    fn opens_the_fence(source: &str) -> bool {
        let lower = source.to_lowercase();
        let packed: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
        [
            concat!("adm", "it"),
            concat!("from", "_env"),
            concat!("r3_web_fence", "_below"),
        ]
        .iter()
        .any(|word| lower.contains(word))
            || packed.contains(concat!("fence", "::*"))
            || packed.contains(concat!("fence", "::var"))
    }

    fn lint(tree: &Tree) -> Vec<String> {
        let mut problems = Vec::new();
        match target_roots(tree) {
            Err(why) => problems.push(why),
            Ok(roots) => {
                for root in roots {
                    let source = tree.files.iter().find(|(p, _)| *p == root).map(|(_, s)| s);
                    if !source.is_some_and(|s| reads_the_fence(s)) {
                        problems.push(format!("a test target with no fence test: {root}"));
                    }
                }
            }
        }
        if !gives_the_fence(&tree.manifest) {
            problems.push("the manifest does not give the tests the feature test-fence".into());
        }
        for (path, source) in &tree.files {
            if declares_r3_web(source) {
                problems.push(format!("an item named after the crate r3-web: {path}"));
            }
            if opens_the_fence(source) {
                problems.push(format!("a call that opens the fence: {path}"));
            }
        }
        problems
    }

    fn indent(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    fn attribute_above(lines: &[&str], n: usize) -> bool {
        lines[..n]
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
            .is_some_and(|l| l.trim_end().ends_with(']'))
    }

    fn in_the_test_module(lines: &[&str], n: usize) -> bool {
        let Some(m) = (0..n)
            .rev()
            .find(|&i| !lines[i].trim().is_empty() && indent(lines[i]) < indent(lines[n]))
        else {
            return false;
        };
        lines[m] == "mod tests {"
            && m > 0
            && lines[m - 1] == "#[cfg(test)]"
            && !attribute_above(lines, m - 1)
            && lines[..n]
                .iter()
                .map(|l| l.matches("/*").count())
                .sum::<usize>()
                == lines[..n]
                    .iter()
                    .map(|l| l.matches("*/").count())
                    .sum::<usize>()
    }

    fn reads_the_fence(source: &str) -> bool {
        let lines: Vec<&str> = source.lines().collect();
        if lines.iter().any(|l| l.trim_start().starts_with("#![")) {
            return false;
        }
        (0..lines.len()).any(|n| {
            let alone = lines[n].trim() == "#[test]"
                && indent(lines[n]) == 4
                && !attribute_above(&lines, n)
                && in_the_test_module(&lines, n);
            let Some(signature) = lines.get(n + 1).map(|l| l.trim()) else {
                return false;
            };
            let named = signature
                .strip_prefix("fn ")
                .and_then(|s| s.strip_suffix("() {"))
                .is_some_and(|name| {
                    !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                });
            if !alone || !named {
                return false;
            }
            let indent = lines[n + 1].len() - lines[n + 1].trim_start().len();
            let body: String = lines[n + 2..]
                .iter()
                .take_while(|l| !(l.len() - l.trim_start().len() == indent && l.trim() == "}"))
                .flat_map(|l| l.chars())
                .filter(|c| !c.is_whitespace())
                .collect();
            body.strip_prefix("assert!(r3_web::procs::fence::is_on(),\"")
                .and_then(|rest| rest.strip_suffix("\");"))
                .is_some_and(|text| !text.contains('"') && !text.contains('\\'))
        })
    }

    #[test]
    fn every_test_target_asserts_that_its_fence_is_on() {
        let tree = tree_of(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
        assert!(tree.files.len() >= 10, "source files: {}", tree.files.len());
        assert_eq!(
            target_roots(&tree),
            Ok(vec![
                "src/bin/r3-standin.rs".to_string(),
                "src/main.rs".to_string()
            ])
        );
        assert_eq!(lint(&tree), Vec::<String>::new());
    }

    const FENCED: &str = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn the_fence_is_on_in_this_test_target() {\n        assert!(r3_web::procs::fence::is_on(), \"off\");\n    }\n}\n";

    fn fixture(manifest: &str, files: &[(&str, &str)]) -> Tree {
        let mut files: Vec<(String, String)> = files
            .iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect();
        files.sort();
        Tree {
            manifest: manifest.to_string(),
            files,
        }
    }

    fn the_manifest() -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("the manifest")
    }

    #[test]
    fn the_lint_finds_each_kind_of_test_target() {
        let manifest = the_manifest();
        assert_eq!(
            lint(&fixture(&manifest, &[("src/main.rs", FENCED)])),
            Vec::<String>::new()
        );
        for target in [
            "src/lib.rs",
            "src/bin/x.rs",
            "src/bin/y/main.rs",
            "tests/t.rs",
            "tests/u/main.rs",
        ] {
            let bare = fixture(
                &manifest,
                &[("src/main.rs", FENCED), (target, "fn main() {}\n")],
            );
            assert_eq!(
                lint(&bare),
                vec![format!("a test target with no fence test: {target}")]
            );
            let fenced = fixture(&manifest, &[("src/main.rs", FENCED), (target, FENCED)]);
            assert_eq!(lint(&fenced), Vec::<String>::new(), "{target}");
        }
        for not_a_target in [
            "src/body.rs",
            "src/bin/y/other.rs",
            "tests/u/helper.rs",
            "examples/e.rs",
            "benches/b.rs",
        ] {
            let tree = fixture(
                &manifest,
                &[("src/main.rs", FENCED), (not_a_target, "fn x() {}\n")],
            );
            assert_eq!(lint(&tree), Vec::<String>::new(), "{not_a_target}");
        }
        let other = manifest.replace(
            "[[bin]]\nname = \"r3-drive\"\npath = \"src/main.rs\"\n",
            "[[bin]]\nname = \"r3-drive\"\npath = \"src/main.rs\"\n\n[[bin]]\nname = \"other\"\npath = \"other/start.rs\"\n",
        );
        assert_ne!(other, manifest);
        let tree = fixture(
            &other,
            &[
                ("src/main.rs", FENCED),
                ("other/start.rs", "fn main() {}\n"),
            ],
        );
        assert_eq!(
            lint(&tree),
            vec!["a test target with no fence test: other/start.rs".to_string()]
        );
        let gone = fixture(&other, &[("src/main.rs", FENCED)]);
        assert_eq!(
            lint(&gone),
            vec!["a target path that does not stand: other/start.rs".to_string()]
        );
    }

    #[test]
    fn the_lint_refuses_a_manifest_that_it_does_not_read() {
        let manifest = the_manifest();
        let files = [
            ("src/main.rs", FENCED),
            ("t/x.rs", "fn x() {}\n"),
            ("examples/e.rs", "fn main() {}\n"),
        ];
        let bin = "[[bin]]\nname = \"r3-drive\"\npath = \"src/main.rs\"\n";
        let package_end = "publish = false\n";
        for (name, changed) in [
            (
                "l01 a test table, path without spaces",
                manifest.replace(
                    bin,
                    &format!("{bin}\n[[test]]\nname = \"t\"\npath=\"t/x.rs\"\n"),
                ),
            ),
            (
                "l02 a test table, path with a dot",
                manifest.replace(
                    bin,
                    &format!("{bin}\n[[test]]\nname = \"t\"\npath = \"./t/x.rs\"\n"),
                ),
            ),
            (
                "l03 harness = false on the bin",
                manifest.replace(bin, &format!("{bin}harness = false\n")),
            ),
            (
                "l03 harness = false on a test",
                manifest.replace(
                    bin,
                    &format!("{bin}\n[[test]]\nname = \"t\"\npath = \"t/x.rs\"\nharness = false\n"),
                ),
            ),
            (
                "l05 an inline target table",
                manifest.replace(
                    package_end,
                    &format!("{package_end}test = [{{ name = \"t\", path = \"t/x.rs\" }}]\n"),
                ),
            ),
            (
                "l06 a test table, path after a tab",
                manifest.replace(
                    bin,
                    &format!("{bin}\n[[test]]\nname = \"t\"\npath\t= \"t/x.rs\"\n"),
                ),
            ),
            (
                "l10 an example with test = true",
                manifest.replace(
                    bin,
                    &format!("{bin}\n[[example]]\nname = \"e\"\ntest = true\n"),
                ),
            ),
            (
                "a lib table",
                manifest.replace(bin, &format!("{bin}\n[lib]\npath = \"t/x.rs\"\n")),
            ),
            (
                "a bench table",
                manifest.replace(bin, &format!("{bin}\n[[bench]]\nname = \"b\"\n")),
            ),
            (
                "autotests in the package",
                manifest.replace(package_end, &format!("{package_end}autotests = false\n")),
            ),
            (
                "a bin path with a dot",
                manifest.replace("path = \"src/main.rs\"", "path = \"./src/main.rs\""),
            ),
            (
                "a key in the workspace",
                manifest.replace("[workspace]\n", "[workspace]\nmembers = [\"t\"]\n"),
            ),
        ] {
            assert_ne!(changed, manifest, "{name}");
            assert!(!lint(&fixture(&changed, &files)).is_empty(), "{name}");
        }
        assert_eq!(lint(&fixture(&manifest, &files)), Vec::<String>::new());
    }

    #[test]
    fn the_lint_refuses_an_item_named_after_the_crate() {
        let name = concat!("r3", "_web");
        for item in [
            format!("mod {name} {{ pub mod procs {{ pub mod fence {{ pub fn is_on() -> bool {{ true }} }} }} }}"),
            format!("use crate::stand_in as {name};"),
            format!("extern crate std as {name};"),
            format!("use crate::shadow::{name};"),
            format!("use crate::shadow::{{{name}, other}};"),
            format!("fn {name}() {{}}"),
            format!("macro_rules! {name} {{ () => {{}} }}"),
            format!("struct {name};"),
        ] {
            assert!(declares_r3_web(&item), "{item}");
            let tree = fixture(&the_manifest(), &[("src/main.rs", &format!("{item}\n{FENCED}"))]);
            assert_eq!(lint(&tree).len(), 1, "{item}");
        }
        for not_an_item in [
            format!("use {name}::procs;"),
            format!("let x = {name}::procs::table();"),
            format!("extern crate {name};"),
            format!("let {name}x = 1;"),
            FENCED.to_string(),
        ] {
            assert!(!declares_r3_web(&not_an_item), "{not_an_item}");
        }
    }

    #[test]
    fn the_lint_refuses_each_spelling_that_opens_the_fence() {
        let a = concat!("adm", "it");
        for source in [
            format!("use r3_web::procs::fence::{{is_on, {a}_below}};"),
            format!("use r3_web::procs::fence as f;\nlet _x = f::{a}_below(dir);"),
            format!(
                "std::env::set_var(\"{}\", dir);\nr3_web::procs::fence::{}();",
                concat!("R3_WEB_FENCE", "_BELOW"),
                concat!("from", "_env")
            ),
            format!(
                "use r3_web::procs::fence::{};\nlet _x = {a}_process(&row);",
                "*"
            ),
            format!("let _x = r3_web::procs::fence :: {a}_below(dir);"),
            format!(
                "let _x = r3_web::procs::fence::{}::default();",
                concat!("Adm", "its")
            ),
            format!(
                "std::env::set_var(r3_web::procs::fence::{}, dir);",
                concat!("V", "AR")
            ),
            format!("use r3_web::procs::fence :: {};", "*"),
            format!("r3_web::procs::fence::{}();", concat!("from", "_env")),
            format!(
                "std::env::set_var(\"{}\", dir);",
                concat!("R3_WEB_FENCE", "_BELOW")
            ),
        ] {
            assert!(opens_the_fence(&source), "{source}");
            let tree = fixture(
                &the_manifest(),
                &[("src/main.rs", FENCED), ("src/other.rs", &source)],
            );
            assert_eq!(
                lint(&tree),
                vec!["a call that opens the fence: src/other.rs".to_string()]
            );
        }
        assert!(!opens_the_fence(FENCED));
    }

    fn gives_the_fence(manifest: &str) -> bool {
        let lines: Vec<&str> = manifest.lines().collect();
        lines.iter().enumerate().any(|(n, l)| {
            *l == r#"r3-web = { path = "../web", default-features = false, features = ["test-fence"] }"#
                && lines[..n].iter().rev().find(|h| h.starts_with('[')) == Some(&"[dev-dependencies]")
        })
    }

    #[test]
    fn the_manifest_check_reads_the_section_and_the_feature_of_the_line() {
        let manifest = the_manifest();
        assert!(gives_the_fence(&manifest));
        for section in [
            "[target.'cfg(windows)'.dev-dependencies]",
            "[build-dependencies]",
            "[dependencies]",
        ] {
            let moved = manifest.replace("[dev-dependencies]", section);
            assert!(!gives_the_fence(&moved), "{section}");
        }
        let without = manifest.replace(
            r#"default-features = false, features = ["test-fence"] }"#,
            "default-features = false }",
        );
        assert_ne!(without, manifest);
        assert!(!gives_the_fence(&without));
        assert_eq!(
            lint(&fixture(&without, &[("src/main.rs", FENCED)])),
            vec!["the manifest does not give the tests the feature test-fence".to_string()]
        );
    }

    #[test]
    fn the_fence_lint_refuses_a_test_that_is_not_compiled_or_not_run_here() {
        let right = "    #[test]\n    fn the_fence_is_on_in_this_test_target() {\n        assert!(r3_web::procs::fence::is_on(), \"off\");\n    }\n";
        assert!(reads_the_fence(&format!(
            "#[cfg(test)]\nmod tests {{\n{right}}}\n"
        )));
        for wrong in [
            format!("#[cfg(test)]\nmod tests {{\n    #[cfg(windows)]\n    /// The fence is on.\n{right}}}\n"),
            format!("#[cfg(test)]\nmod tests {{\n    #[cfg(\n        windows\n    )]\n{right}}}\n"),
            format!("#[cfg(test)]\nmod tests {{\n    #[cfg(windows)]\n\n{right}}}\n"),
            format!("#![cfg(windows)]\n#[cfg(test)]\nmod tests {{\n{right}}}\n"),
            format!("#[cfg(windows)]\n#[cfg(test)]\nmod tests {{\n{right}}}\n"),
            format!("#[cfg(test)]\nmod tests {{\n    #[cfg(windows)]\n    mod inner {{\n    {right}    }}\n}}\n"),
            format!("#[cfg(test)]\nmod other {{\n{right}}}\n"),
            format!("mod tests {{\n{right}}}\n"),
            format!("#![cfg(windows)]\n\nuse std::io;\n\n#[cfg(test)]\nmod tests {{\n{right}}}\n"),
            format!("use std::io;\nmod tests {{\n{right}}}\n"),
            format!("/*\n#[cfg(test)]\nmod tests {{\n{right}}}\n*/\n"),
            format!("#[cfg(test)]\nmod tests {{\n    /*\n{right}    */\n}}\n"),
            format!("macro_rules! never {{\n    () => {{\n{right}    }};\n}}\n"),
            "#[test]\nfn the_fence_is_on_in_this_test_target() {\n    assert!(r3_web::procs::fence::is_on(), \"off\");\n}\n".to_string(),
            "#[cfg(test)]\nmod tests {\n        #[test]\n        fn the_fence_is_on_in_this_test_target() {\n            assert!(r3_web::procs::fence::is_on(), \"off\");\n        }\n}\n".to_string(),
        ] {
            assert!(!reads_the_fence(&wrong), "{wrong}");
        }
    }

    #[test]
    fn the_fence_lint_takes_the_assertion_alone() {
        let test = |body: &str| {
            format!("#[cfg(test)]\nmod tests {{\n    #[test]\n    fn the_fence_is_on_in_this_test_target() {{\n{body}\n    }}\n}}\n")
        };
        let right = "        assert!(\n            r3_web::procs::fence::is_on(),\n            \"the fence is off\"\n        );";
        assert!(reads_the_fence(&test(right)));
        assert!(reads_the_fence(
            &test(right).replace("the_fence_is_on_in_this_test_target", "any_name")
        ));
        for wrong in [
            "",
            "        assert!(true, \"the fence is off\");",
            "        let _ = r3_web::procs::fence::is_on();",
            "        assert!(!r3_web::procs::fence::is_on(), \"the fence is off\");",
            "        assert!(r3_web::procs::fence::is_on() || true, \"the fence is off\");",
            "        r3_web::procs::fence::on();\n        assert!(r3_web::procs::fence::is_on(), \"x\");",
            "        assert!(r3_web::procs::fence::is_on(), \"x\");\n        assert!(true, \"y\");",
            "        assert!(r3_web::procs::fence::is_on(), \"a\" );\n        return;",
        ] {
            assert!(!reads_the_fence(&test(wrong)), "{wrong}");
        }
        for attribute in [
            "#[ignore = \"a reason\"]".to_string(),
            ["#[", "ignore", "]"].concat(),
        ] {
            let ignored =
                test(right).replace("    #[test]\n", &format!("    #[test]\n    {attribute}\n"));
            assert!(!reads_the_fence(&ignored), "{attribute}");
        }
        let gated = test(right).replace("    #[test]\n", "    #[cfg(windows)]\n    #[test]\n");
        assert!(!reads_the_fence(&gated));
        let no_attribute = test(right).replace("    #[test]\n", "");
        assert!(!reads_the_fence(&no_attribute));
        assert!(!reads_the_fence(
            "// fn the_fence_is_on_in_this_test_target() { assert!(r3_web::procs::fence::is_on(), \"x\"); }"
        ));
    }
}
