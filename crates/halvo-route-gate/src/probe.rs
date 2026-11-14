//! The command `probe` of contract T13.3: N picks, N admissions and N steps of the spend machine on
//! the release data, with no allocation per round, so that the peak of its process bounds what the
//! router adds to a process that links it.

#![deny(unsafe_code)]

use std::hint::black_box;
use std::io::Write;

use halvo_route::admit::{admit, estimate, Room};
use halvo_route::cap::DefaultCap;
use halvo_route::pick::{pick, Pick};
use halvo_route::policy::{Policy, CELL_COUNT};
use halvo_route::prices::Prices;
use halvo_route::release;
use halvo_route::ruling::Ruling;
use halvo_route::spend::{step, Account, Hold, Move};
use halvo_route::tier::{CallKind, CategoryId, Tier};

/// What a round reads: a ruling, a policy table, a default cap and prices.
pub struct Data<'a> {
    pub ruling: Ruling,
    pub policy: &'a Policy,
    pub cap: Option<DefaultCap>,
    pub prices: &'a Prices,
}

impl Data<'static> {
    /// The compiled-in data of `halvo_route::release` and `Prices::release()`.
    pub fn release() -> Data<'static> {
        Data {
            ruling: release::ruling(),
            policy: release::policy(),
            cap: release::cap(),
            prices: Prices::release(),
        }
    }
}

const BODY_BYTES: u64 = 4096;
const MAX_OUTPUT_TOKENS: u64 = 1024;

/// What `run` made: the number of rounds and whether every round held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    pub rounds: u64,
    pub ok: bool,
}

/// Makes `rounds` rounds on `data`, round `i` from 0: the pick of the cell `i` mod 44 of
/// `halvo_route::policy::cells()` under the ceiling `Tier::ALL[i mod 3]`; the admission of a task
/// of that cell's category, with the estimate of `admit::estimate`, the room that the cap and the
/// spend machine's account give and one running task of the same estimate; and one step of the
/// spend machine on one hold of the low-cost tier's amount for 4,096 body bytes and 1,024 output
/// tokens, which goes `reserve`, `dispatch`, `settle` at that amount and starts a new hold on a new
/// account every third round. It allocates nothing. It returns the number of rounds that it made,
/// and `ok` true when no pick names a tier that the ruling does not allow and every step goes
/// through.
pub fn run(data: &Data<'_>, rounds: u64) -> Probe {
    let fresh = Account {
        cap_minor: data.cap.map(|cap| cap.amount),
        active: true,
        spent: 0,
        held: 0,
    };
    let amount = data
        .prices
        .hold(Tier::LowCostHosted, BODY_BYTES, MAX_OUTPUT_TOKENS);
    let mut account = fresh;
    let mut hold = Hold::new();
    let mut ok = true;
    let mut made = 0;
    for round in 0..rounds {
        let round = black_box(round);
        let cell = (round % CELL_COUNT as u64) as usize;
        let category = CategoryId::ALL[cell / CallKind::ALL.len()];
        let kind = CallKind::ALL[cell % CallKind::ALL.len()];
        let ceiling = Tier::ALL[(round % Tier::ALL.len() as u64) as usize];
        if let Pick::Tier { tier, .. } = black_box(pick(data.policy, category, kind, ceiling)) {
            ok &= data.ruling.allowed().contains(&tier);
        }
        let room = data.cap.map(|cap| Room {
            cap_minor: cap.amount,
            spent: account.spent,
            held: account.held,
            period_end_ms: 0,
        });
        let estimate = estimate(data.policy, category);
        black_box(admit(&data.ruling, room.as_ref(), estimate, &[estimate]));
        let mv = match round % 3 {
            0 => {
                account = fresh;
                hold = Hold::new();
                Move::Reserve { amount }
            }
            1 => Move::Dispatch,
            _ => Move::Settle { cost: amount },
        };
        match black_box(step(&account, &hold, mv)) {
            Ok((next_account, next_hold)) => {
                account = next_account;
                hold = next_hold;
            }
            Err(_) => ok = false,
        }
        made += 1;
    }
    Probe { rounds: made, ok }
}

/// The command `route-gate probe --rounds N` on the release data. It prints `probe rounds=<N>
/// ok=<yes|no>` and returns 0 for `ok=yes` and 1 for `ok=no` or a failed write of that line, with
/// nothing on `err`; or it prints one line `REFUSED usage: <detail>` on `err`, nothing on `out`,
/// and returns 2.
pub fn command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    command_on(&Data::release(), args, out, err)
}

fn command_on(data: &Data<'_>, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let rounds = match rounds(args) {
        Ok(rounds) => rounds,
        Err(detail) => {
            let _ = writeln!(err, "REFUSED usage: {detail}");
            return 2;
        }
    };
    let probe = run(data, rounds);
    let word = if probe.ok { "yes" } else { "no" };
    match writeln!(out, "probe rounds={} ok={word}", probe.rounds).and_then(|()| out.flush()) {
        Ok(()) if probe.ok => 0,
        _ => 1,
    }
}

fn rounds(args: &[String]) -> Result<u64, String> {
    let mut found = None;
    let mut words = args.iter();
    while let Some(name) = words.next() {
        if name != "--rounds" {
            return Err(format!("unknown argument {}", printable(name)));
        }
        let Some(value) = words.next() else {
            return Err("--rounds has no value".to_owned());
        };
        if found.replace(value).is_some() {
            return Err("--rounds twice".to_owned());
        }
    }
    let Some(text) = found else {
        return Err("--rounds is required".to_owned());
    };
    let canonical =
        !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && !text.starts_with('0');
    match text.parse::<u64>() {
        Ok(value) if canonical => Ok(value),
        _ => Err("--rounds takes a whole number from 1".to_owned()),
    }
}

fn printable(word: &str) -> &str {
    if word.starts_with("--") && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        word
    } else {
        "of another form"
    }
}

#[cfg(test)]
#[forbid(unsafe_code)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::Ordering;

    use halvo_route::policy::{Basis, PickCell, Row};
    use halvo_route::tier::Level;

    use super::*;

    const HOSTED_CELLS: usize = 30;

    const CHECK_ROUNDS: u64 = 100_000;

    const SCRIPT: &str = include_str!("../../../scripts/r6/ci.d/70-r6-memory.sh");

    const CHILD: &str = "HALVO_ROUTE_GATE_PROBE_ALLOCATION_CHILD";

    use super::counting::{
        this_thread, ALLOCATIONS, BYTES, FIRST_ALLOCATIONS, FIRST_BYTES, FIRST_THREAD,
    };

    fn first_thread() -> (u64, u64) {
        (
            FIRST_ALLOCATIONS.load(Ordering::SeqCst),
            FIRST_BYTES.load(Ordering::SeqCst),
        )
    }

    fn other_threads() -> (u64, u64) {
        (
            ALLOCATIONS.load(Ordering::SeqCst),
            BYTES.load(Ordering::SeqCst),
        )
    }

    fn since(before: (u64, u64), after: (u64, u64)) -> (u64, u64) {
        (after.0 - before.0, after.1 - before.1)
    }

    mod token {
        use std::sync::atomic::Ordering;

        use super::{this_thread, FIRST_THREAD};

        pub(super) struct Apart(());

        pub(super) const ON_THE_FIRST_THREAD: &str =
            "the test runs on the process's first thread, whose calls the windows leave out, so \
             nothing was measured; the test harness runs a test there when it cannot start a \
             thread for it";
        pub(super) const NO_FIRST_THREAD: &str =
            "no thread made a call of the allocator before the test began";

        pub(super) fn refusal(first: usize, me: usize, first_calls: u64) -> Option<&'static str> {
            if first_calls == 0 {
                return Some(NO_FIRST_THREAD);
            }
            if me == first {
                return Some(ON_THE_FIRST_THREAD);
            }
            None
        }

        pub(super) fn apart_from_the_harness(start: (u64, u64)) -> Apart {
            if let Some(why) = refusal(FIRST_THREAD.load(Ordering::SeqCst), this_thread(), start.0)
            {
                panic!("{why}");
            }
            Apart(())
        }
    }

    use token::{apart_from_the_harness, refusal, Apart, NO_FIRST_THREAD, ON_THE_FIRST_THREAD};

    struct Window {
        first: (u64, u64),
        others: (u64, u64),
    }

    fn allocations<T>(_: &Apart, work: impl FnOnce() -> T) -> (Window, T) {
        let before = (first_thread(), other_threads());
        let held = work();
        let after = (first_thread(), other_threads());
        let window = Window {
            first: since(before.0, after.0),
            others: since(before.1, after.1),
        };
        (window, held)
    }

    fn one_round_and_many<T>(
        apart: &Apart,
        rounds: impl Fn(u64) -> T,
    ) -> ((Window, T), (Window, T)) {
        black_box(rounds(1));
        let one = allocations(apart, || rounds(1));
        let many = allocations(apart, || rounds(CHECK_ROUNDS));
        (one, many)
    }

    fn the_same_count(one: &Window, many: &Window) -> bool {
        many.others == one.others
    }

    fn window_line(name: &str, window: &Window) -> String {
        format!(
            "window {name} first_calls={} first_bytes={} other_calls={} other_bytes={}",
            window.first.0, window.first.1, window.others.0, window.others.1
        )
    }

    fn print_windows(start: (u64, u64), windows: [(&str, &Window); 2]) {
        let end = first_thread();
        let on_first = this_thread() == FIRST_THREAD.load(Ordering::SeqCst);
        for (name, window) in windows {
            println!("{}", window_line(name, window));
        }
        println!(
            "first_thread start_calls={} start_bytes={} end_calls={} end_bytes={} \
             test_on_first_thread={on_first}",
            start.0, start.1, end.0, end.1
        );
    }

    fn in_a_child_alone(name: &str) -> bool {
        if std::env::var_os(CHILD).is_some() {
            return true;
        }
        let output = Command::new(std::env::current_exe().expect("the test binary"))
            .args(["--exact", name, "--test-threads", "1", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .expect("the test binary runs");
        let out = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        let text =
            format!("the child's standard output:\n{out}\nthe child's standard error:\n{err}");
        assert!(output.status.success(), "{text}");
        assert!(out.contains("test result: ok. 1 passed"), "{text}");
        false
    }

    fn hosted_policy() -> Policy {
        let base = Policy::hand_back_all().to_text();
        let cell = "utility_bill_payment\tcompile\thand_back\t-\tuntested\t0\t0\n";
        let hosted = base.replacen(
            cell,
            "utility_bill_payment\tcompile\tlow_cost_hosted\tlow\tline_met\t4\t4\n",
            1,
        );
        assert_ne!(hosted, base);
        Policy::parse_for(&hosted, &Tier::ALL).expect("a hosted table")
    }

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn call(args: &[&str]) -> (u8, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = command(&words(args), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).expect("utf-8"),
            String::from_utf8(err).expect("utf-8"),
        )
    }

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("closed"))
        }
    }

    #[test]
    fn probe_allocates_nothing_per_round() {
        let start = first_thread();
        if !in_a_child_alone("probe::tests::probe_allocates_nothing_per_round") {
            return;
        }
        let apart = apart_from_the_harness(start);
        let data = Data::release();
        let (one, many) = one_round_and_many(&apart, |rounds| run(&data, rounds));
        print_windows(start, [("rounds=1", &one.0), ("rounds=many", &many.0)]);
        assert_eq!(
            (one.1, many.1),
            (
                Probe {
                    rounds: 1,
                    ok: true
                },
                Probe {
                    rounds: CHECK_ROUNDS,
                    ok: true
                }
            )
        );
        assert!(
            the_same_count(&one.0, &many.0),
            "1 round {:?}, many rounds {:?}",
            one.0.others,
            many.0.others
        );
    }

    #[test]
    fn the_measure_sees_a_round_that_allocates() {
        let start = first_thread();
        if !in_a_child_alone("probe::tests::the_measure_sees_a_round_that_allocates") {
            return;
        }
        let apart = apart_from_the_harness(start);
        let data = Data::release();
        let late = |rounds: u64| rounds == CHECK_ROUNDS;
        let measuring = one_round_and_many(&apart, |rounds| {
            let probe = run(&data, rounds);
            if late(rounds) {
                black_box(Box::new(rounds));
            }
            probe
        });
        let started = one_round_and_many(&apart, |rounds| {
            let probe = run(&data, rounds);
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        if late(rounds) {
                            black_box(Box::new(rounds));
                        }
                    })
                    .join()
            })
            .expect("the started thread ends");
            probe
        });
        let sized = one_round_and_many(&apart, |rounds| {
            black_box(Vec::<u8>::with_capacity(
                usize::try_from(rounds).expect("a size"),
            ));
            run(&data, rounds)
        });
        for (name, (one, many)) in [
            ("measuring", measuring),
            ("started", started),
            ("sized", sized),
        ] {
            println!("{}", window_line(&format!("{name}:rounds=1"), &one.0));
            println!("{}", window_line(&format!("{name}:rounds=many"), &many.0));
            assert!(
                !the_same_count(&one.0, &many.0),
                "{name}: 1 round {:?}, many rounds {:?}",
                one.0.others,
                many.0.others
            );
        }
    }

    #[test]
    fn the_allocation_test_makes_the_rounds_of_the_check() {
        let lines: Vec<String> = SCRIPT
            .lines()
            .filter(|line| line.starts_with("rounds="))
            .map(str::to_owned)
            .collect();
        assert_eq!(lines, vec![format!("rounds={CHECK_ROUNDS}")]);
    }

    const STARTS: [&str; 10] = [
        "process",
        "Process",
        "Command",
        "spawn",
        "fork",
        "exec",
        "extern",
        "asm!",
        "include!(",
        "#[path",
    ];

    const ALLOWED: [&str; 2] = ["std::process::ExitCode", "std::process::id()"];

    const TEST_TARGETS: [&str; 3] = ["tests/", "benches/", "examples/"];

    fn tool(program: &str, dir: &Path, args: &[&str]) -> String {
        let output = Command::new(program)
            .current_dir(dir)
            .args(args)
            .output()
            .expect(program);
        assert!(
            output.status.success(),
            "{program} {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("UTF-8 output")
    }

    fn crate_dirs(tree: &str) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = tree
            .lines()
            .filter_map(|line| {
                let line = line.strip_suffix(" (*)").unwrap_or(line);
                let open = line.find(" (")?;
                line[open + 2..].strip_suffix(')').map(PathBuf::from)
            })
            .filter(|dir| dir.is_dir())
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    fn code_outside_tests(text: &str) -> Result<(String, Vec<String>), String> {
        let lines: Vec<&str> = text.lines().collect();
        let mut code = Vec::new();
        let mut modules = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim_start();
            i += 1;
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed != "#[cfg(test)]" {
                code.push(line);
                continue;
            }
            let indent = &line[..line.len() - trimmed.len()];
            let at_indent = |other: &str| {
                other.starts_with(indent)
                    && !other[indent.len()..].starts_with(char::is_whitespace)
                    && other.len() > indent.len()
            };
            while i < lines.len() && at_indent(lines[i]) && lines[i].trim_start().starts_with("#[")
            {
                i += 1;
            }
            let Some(item) = lines.get(i).filter(|item| at_indent(item)) else {
                return Err(format!("line {i}: no item after #[cfg(test)]"));
            };
            i += 1;
            let item = item.trim_start();
            if item.ends_with(';') || item.ends_with(',') {
                let words: Vec<&str> = item.trim_end_matches(';').split_whitespace().collect();
                if let [.., "mod", name] = words.as_slice() {
                    modules.push((*name).to_owned());
                }
                continue;
            }
            loop {
                let Some(next) = lines.get(i) else {
                    return Err(format!("line {i}: an item under #[cfg(test)] has no end"));
                };
                i += 1;
                if !at_indent(next) {
                    continue;
                }
                let next = next.trim_start();
                if next.starts_with(['}', ']', ')']) && next.ends_with(['}', ';', ',', ')']) {
                    break;
                }
                if !next.starts_with(')') {
                    return Err(format!(
                        "line {i}: an item under #[cfg(test)] ends where it cannot"
                    ));
                }
            }
        }
        Ok((code.join("\n"), modules))
    }

    fn module_paths(path: &str, name: &str) -> [String; 2] {
        let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
        let root = matches!(file, "mod.rs" | "lib.rs" | "main.rs")
            || path == "build.rs"
            || dir == "src/bin";
        let mut base = if root {
            dir.to_owned()
        } else {
            format!("{dir}/{}", file.trim_end_matches(".rs"))
        };
        if base.starts_with('/') {
            base.remove(0);
        }
        let base = if base.is_empty() {
            String::new()
        } else {
            format!("{base}/")
        };
        [format!("{base}{name}.rs"), format!("{base}{name}/")]
    }

    fn process_words(code: &str) -> Vec<&'static str> {
        let mut code = code.to_owned();
        for allowed in ALLOWED {
            code = code.replace(allowed, "");
        }
        STARTS
            .into_iter()
            .filter(|word| code.contains(word))
            .collect()
    }

    fn read_crate(files: &[(String, Option<String>)]) -> (usize, Vec<String>) {
        let mut misses = Vec::new();
        let mut read = Vec::new();
        let mut tests = Vec::new();
        for (path, text) in files {
            if !path.ends_with(".rs") || TEST_TARGETS.iter().any(|top| path.starts_with(top)) {
                continue;
            }
            let Some(text) = text else {
                misses.push(format!("{path}: not covered"));
                continue;
            };
            match code_outside_tests(text) {
                Ok((code, modules)) => {
                    for name in modules {
                        tests.extend(module_paths(path, &name));
                    }
                    read.push((path, code));
                }
                Err(detail) => misses.push(format!("{path}: not covered, {detail}")),
            }
        }
        let mut count = 0;
        for (path, code) in read {
            let test_file = tests.iter().any(|test| {
                path == test || (test.ends_with('/') && path.starts_with(test.as_str()))
            });
            if test_file {
                continue;
            }
            count += 1;
            for word in process_words(&code) {
                misses.push(format!("{path}: names {word}"));
            }
        }
        (count, misses)
    }

    fn rust_files_below(dir: &Path, top: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("a directory of the crate") {
            let path = entry.expect("an entry of the crate").path();
            if path.is_dir() {
                rust_files_below(&path, top, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative = path.strip_prefix(top).expect("below the crate");
                let words: Vec<String> = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.push(words.join("/"));
            }
        }
    }

    fn outside_the_commit<'a>(present: &'a [String], listed: &[&str]) -> Vec<&'a str> {
        present
            .iter()
            .map(String::as_str)
            .filter(|path| !listed.contains(path))
            .collect()
    }

    #[test]
    fn the_probe_names_no_process() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let tree = tool(
            env!("CARGO"),
            manifest,
            &[
                "tree",
                "--locked",
                "-e",
                "normal,no-proc-macro",
                "--target",
                "all",
                "-p",
                env!("CARGO_PKG_NAME"),
                "--prefix",
                "none",
                "--format",
                "{p}",
            ],
        );
        let dirs = crate_dirs(&tree);
        let this = manifest.canonicalize().expect("the crate's directory");
        assert!(
            dirs.iter()
                .any(|dir| dir.canonicalize().ok().as_deref() == Some(this.as_path())),
            "cargo tree does not name this crate: {tree}"
        );
        let mut misses = Vec::new();
        for dir in &dirs {
            let listed = tool(
                "git",
                dir,
                &["ls-tree", "-r", "-z", "--name-only", "HEAD", "--", "."],
            );
            let names: Vec<&str> = listed.split('\0').filter(|name| !name.is_empty()).collect();
            let files: Vec<(String, Option<String>)> = names
                .iter()
                .map(|name| {
                    (
                        (*name).to_owned(),
                        std::fs::read_to_string(dir.join(name)).ok(),
                    )
                })
                .collect();
            let (count, crate_misses) = read_crate(&files);
            assert!(count > 0, "no file of {} was read", dir.display());
            misses.extend(
                crate_misses
                    .into_iter()
                    .map(|miss| format!("{}: {miss}", dir.display())),
            );
            let mut present = Vec::new();
            rust_files_below(dir, dir, &mut present);
            for path in outside_the_commit(&present, &names) {
                misses.push(format!(
                    "{}: {path}: not covered, not in the commit",
                    dir.display()
                ));
            }
        }
        assert!(misses.is_empty(), "{misses:#?}");
    }

    fn one_file(path: &str, text: &str) -> Vec<(String, Option<String>)> {
        vec![(path.to_owned(), Some(text.to_owned()))]
    }

    #[test]
    fn the_read_finds_a_process_start_in_a_new_file() {
        let files = one_file(
            "src/start.rs",
            "pub fn go() {\n    let _ = std::process::Command::new(\"x\");\n}\n",
        );
        let (count, misses) = read_crate(&files);
        assert_eq!(count, 1);
        assert!(
            misses.contains(&"src/start.rs: names process".to_owned()),
            "{misses:?}"
        );
        assert!(
            misses.contains(&"src/start.rs: names Command".to_owned()),
            "{misses:?}"
        );
    }

    #[test]
    fn the_read_allows_the_exit_code_and_the_process_id() {
        let text = "use std::process::ExitCode;\nfn n() -> u32 {\n    std::process::id()\n}\n";
        assert_eq!(read_crate(&one_file("src/main.rs", text)), (1, Vec::new()));
        let other = "fn n() {\n    std::process::exit(1)\n}\n";
        assert_eq!(
            read_crate(&one_file("src/main.rs", other)).1,
            vec!["src/main.rs: names process"]
        );
    }

    #[test]
    fn the_read_leaves_out_test_code_and_comments_alone() {
        let text = "// Command\n#[cfg(test)]\nmod tests {\n    fn f() {\n        Command::new(\"x\");\n    }\n}\n#[cfg(test)]\nmod more;\n";
        let mut files = one_file("src/lib.rs", text);
        files.push(("src/more.rs".to_owned(), Some("fn spawn() {}\n".to_owned())));
        files.push((
            "tests/cli.rs".to_owned(),
            Some("fn spawn() {}\n".to_owned()),
        ));
        assert_eq!(read_crate(&files), (1, Vec::new()));
        let after = "#[cfg(test)]\nmod tests {\n}\nfn go() {\n    fork();\n}\n";
        assert_eq!(
            read_crate(&one_file("src/lib.rs", after)).1,
            vec!["src/lib.rs: names fork"]
        );
    }

    #[test]
    fn a_test_module_file_is_found_beside_its_parent() {
        assert_eq!(
            module_paths("src/lib.rs", "tests"),
            ["src/tests.rs".to_owned(), "src/tests/".to_owned()]
        );
        assert_eq!(
            module_paths("src/spendsim/mod.rs", "tests"),
            [
                "src/spendsim/tests.rs".to_owned(),
                "src/spendsim/tests/".to_owned()
            ]
        );
        assert_eq!(
            module_paths("src/probe.rs", "deep"),
            ["src/probe/deep.rs".to_owned(), "src/probe/deep/".to_owned()]
        );
        assert_eq!(
            module_paths("build.rs", "tests"),
            ["tests.rs".to_owned(), "tests/".to_owned()]
        );
        assert_eq!(
            module_paths("src/bin/tool/main.rs", "tests"),
            [
                "src/bin/tool/tests.rs".to_owned(),
                "src/bin/tool/tests/".to_owned()
            ]
        );
        assert_eq!(
            module_paths("src/bin/tool.rs", "tests"),
            ["src/bin/tests.rs".to_owned(), "src/bin/tests/".to_owned()]
        );
    }

    #[test]
    fn a_file_that_the_read_cannot_place_is_not_covered() {
        let files = vec![("src/odd.rs".to_owned(), None)];
        assert_eq!(read_crate(&files).1, vec!["src/odd.rs: not covered"]);
        let open = "#[cfg(test)]\nmod tests {\n    fn f() {}\n";
        assert!(
            read_crate(&one_file("src/lib.rs", open)).1[0].starts_with("src/lib.rs: not covered")
        );
        let early = "#[cfg(test)]\nconst A: [u8; 1] = [\nfn go() {}\n";
        assert!(
            read_crate(&one_file("src/lib.rs", early)).1[0].starts_with("src/lib.rs: not covered")
        );
    }

    #[test]
    fn a_rust_file_that_the_commit_does_not_hold_is_not_covered() {
        let present = vec!["src/lib.rs".to_owned(), "src/new.rs".to_owned()];
        assert_eq!(
            outside_the_commit(&present, &["src/lib.rs"]),
            vec!["src/new.rs"]
        );
        assert!(outside_the_commit(&present, &["src/lib.rs", "src/new.rs"]).is_empty());
    }

    #[test]
    fn the_read_goes_on_after_an_item_that_a_bracket_or_a_parenthesis_ends() {
        let text = "#[cfg(test)]\nconst A: [u8; 1] = [\n    1,\n];\n#[cfg(test)]\nfn f(\n    a: u8,\n) -> u8 {\n    a\n}\nfn go() {\n    fork();\n}\n";
        assert_eq!(
            read_crate(&one_file("src/lib.rs", text)).1,
            vec!["src/lib.rs: names fork"]
        );
    }

    #[test]
    fn code_after_an_item_that_does_not_end_is_not_covered() {
        let text = "#[cfg(test)]\nconst A: [u8; 1] = [\nfn go() {\n    fork();\n}\n";
        let misses = read_crate(&one_file("src/lib.rs", text)).1;
        assert_eq!(misses.len(), 1, "{misses:?}");
        assert!(
            misses[0].starts_with("src/lib.rs: not covered"),
            "{misses:?}"
        );
    }

    #[test]
    fn the_read_reads_rust_files_alone() {
        let files = one_file("src/notes.txt", "Command\n");
        assert_eq!(read_crate(&files), (0, Vec::new()));
    }

    #[test]
    fn a_test_module_with_a_directory_leaves_out_every_file_below_it() {
        let mut files = one_file("src/lib.rs", "#[cfg(test)]\nmod deep;\n");
        files.push((
            "src/deep/mod.rs".to_owned(),
            Some("mod inner;\n".to_owned()),
        ));
        files.push((
            "src/deep/inner.rs".to_owned(),
            Some("fn spawn() {}\n".to_owned()),
        ));
        assert_eq!(read_crate(&files), (1, Vec::new()));
    }

    #[test]
    fn the_crate_set_is_the_packages_with_a_path() {
        let here = env!("CARGO_MANIFEST_DIR");
        let tree = format!("halvo-route-gate v0.0.0 ({here})\nserde v1.0.229\nhalvo-route-gate v0.0.0 ({here}) (*)\nother v1.0.0 (/no/such/dir)\n");
        assert_eq!(crate_dirs(&tree), vec![PathBuf::from(here)]);
    }

    #[test]
    fn the_probe_runs_on_the_release_data() {
        let data = Data::release();
        assert!(std::ptr::eq(data.policy, release::policy()));
        assert_eq!(*data.policy, *release::policy());
        assert_eq!(data.ruling, release::ruling());
        assert_eq!(data.cap, release::cap());
        assert!(std::ptr::eq(data.prices, Prices::release()));
    }

    #[test]
    fn probe_visits_every_cell_that_can_hold_a_hosted_pick() {
        let mut hosted = 0;
        for index in 0..CELL_COUNT {
            let mut rows = *Policy::hand_back_all().rows();
            rows[index] = Row {
                pick: PickCell::Tier(Tier::Frontier),
                level: Some(Level::Low),
                basis: Basis::LineMet,
                k: 4,
                n: 4,
            };
            let Ok(policy) = Policy::from_rows(rows, &Tier::ALL) else {
                continue;
            };
            hosted += 1;
            let data = Data {
                policy: &policy,
                ..Data::release()
            };
            assert!(!run(&data, CELL_COUNT as u64 * 3).ok, "cell {index}");
        }
        assert_eq!(hosted, HOSTED_CELLS);
    }

    #[test]
    fn the_count_sees_allocations_of_every_thread_but_the_first() {
        let start = first_thread();
        if !in_a_child_alone(
            "probe::tests::the_count_sees_allocations_of_every_thread_but_the_first",
        ) {
            return;
        }
        let apart = apart_from_the_harness(start);
        let (boxes, _) = allocations(&apart, || {
            std::hint::black_box(Box::new(1_u64)) == Box::new(1)
        });
        let (started, _) = allocations(&apart, || {
            std::thread::scope(|scope| {
                scope
                    .spawn(|| std::hint::black_box(vec![0_u8; 4096]))
                    .join()
            })
        });
        print_windows(start, [("boxes", &boxes), ("started", &started)]);
        assert_eq!(boxes.others, (2, 16));
        let count = started.others;
        assert!(count.0 >= 1 && count.1 >= 4096, "{count:?}");
    }

    #[test]
    fn a_child_that_fails_on_purpose() {
        if std::env::var_os(CHILD).is_some() {
            assert_eq!(std::hint::black_box((1, 2)), (1, 3));
        }
    }

    #[test]
    fn a_failed_child_says_what_it_saw() {
        let failed = std::panic::catch_unwind(|| {
            in_a_child_alone("probe::tests::a_child_that_fails_on_purpose")
        })
        .expect_err("the child fails");
        let text = failed.downcast_ref::<String>().expect("a panic text");
        assert!(text.contains("left: (1, 2)"), "{text}");
        assert!(text.contains("right: (1, 3)"), "{text}");
        assert!(
            text.contains("test result: FAILED. 0 passed; 1 failed"),
            "{text}"
        );
    }

    #[test]
    fn a_child_that_prints_a_pass_and_fails() {
        if std::env::var_os(CHILD).is_some() {
            println!("test result: ok. 1 passed");
            assert_eq!(std::hint::black_box((1, 2)), (1, 3));
        }
    }

    #[test]
    fn a_child_that_prints_a_pass_and_fails_is_refused() {
        let failed = std::panic::catch_unwind(|| {
            in_a_child_alone("probe::tests::a_child_that_prints_a_pass_and_fails")
        })
        .expect_err("the child fails");
        let text = failed.downcast_ref::<String>().expect("a panic text");
        assert!(text.contains("test result: ok. 1 passed\n"), "{text}");
        assert!(
            text.contains("test result: FAILED. 0 passed; 1 failed"),
            "{text}"
        );
    }

    #[test]
    fn a_child_that_runs_no_test_fails_the_parent() {
        let failed = std::panic::catch_unwind(|| in_a_child_alone("probe::tests::no_such_test"))
            .expect_err("a child that runs no test fails");
        let text = failed.downcast_ref::<String>().expect("a panic text");
        assert!(text.contains("test result: ok. 0 passed"), "{text}");
    }

    #[test]
    fn a_window_leaves_out_the_calls_of_the_first_thread() {
        let start = first_thread();
        if !in_a_child_alone("probe::tests::a_window_leaves_out_the_calls_of_the_first_thread") {
            return;
        }
        let apart = apart_from_the_harness(start);
        let size = 1 << 20;
        FIRST_THREAD.store(this_thread(), Ordering::SeqCst);
        let (window, _) = allocations(&apart, || black_box(vec![0_u8; size]));
        println!("{}", window_line("mark-moved", &window));
        let size = size as u64;
        assert!(
            window.first.0 >= 1 && window.first.1 >= size && window.first.1 < 2 * size,
            "first {:?}, others {:?}",
            window.first,
            window.others
        );
        assert!(window.others.1 < size, "{:?}", window.others);
    }

    #[test]
    fn a_window_is_refused_when_no_thread_called_before_the_test() {
        let refused = std::panic::catch_unwind(|| apart_from_the_harness((0, 0)))
            .err()
            .expect("the guard refuses");
        assert_eq!(
            refused.downcast_ref::<String>().map(String::as_str),
            Some(NO_FIRST_THREAD)
        );
    }

    #[test]
    fn a_window_leaves_out_only_a_first_thread_that_is_not_the_test() {
        assert_eq!(refusal(7, 8, 1), None);
        assert_eq!(refusal(7, 7, 1), Some(ON_THE_FIRST_THREAD));
        assert_eq!(refusal(7, 8, 0), Some(NO_FIRST_THREAD));
        assert_eq!(refusal(0, 8, 0), Some(NO_FIRST_THREAD));
    }

    #[test]
    fn the_refusal_on_the_first_thread_says_that_nothing_was_measured_and_why() {
        assert!(
            ON_THE_FIRST_THREAD.contains("nothing was measured"),
            "{ON_THE_FIRST_THREAD}"
        );
        assert!(
            ON_THE_FIRST_THREAD.contains(
                "the test harness runs a test there when it cannot start a thread for it"
            ),
            "{ON_THE_FIRST_THREAD}"
        );
    }

    #[test]
    fn a_test_on_the_first_thread_is_refused_at_the_call() {
        let start = first_thread();
        if !in_a_child_alone("probe::tests::a_test_on_the_first_thread_is_refused_at_the_call") {
            return;
        }
        FIRST_THREAD.store(this_thread(), Ordering::SeqCst);
        let refused = std::panic::catch_unwind(|| apart_from_the_harness(start))
            .err()
            .expect("the guard refuses");
        assert_eq!(
            refused.downcast_ref::<String>().map(String::as_str),
            Some(ON_THE_FIRST_THREAD)
        );
    }

    #[test]
    fn a_window_line_names_each_count() {
        let window = Window {
            first: (1, 2),
            others: (3, 4),
        };
        assert_eq!(
            window_line("w", &window),
            "window w first_calls=1 first_bytes=2 other_calls=3 other_bytes=4"
        );
    }

    #[test]
    fn the_same_count_reads_the_other_threads_alone() {
        let window = |first, others| Window { first, others };
        assert!(the_same_count(
            &window((0, 0), (5, 40)),
            &window((3, 208), (5, 40))
        ));
        assert!(!the_same_count(
            &window((0, 0), (5, 40)),
            &window((0, 0), (6, 40))
        ));
        assert!(!the_same_count(
            &window((0, 0), (5, 40)),
            &window((0, 0), (5, 48))
        ));
        assert!(!the_same_count(
            &window((1, 8), (5, 40)),
            &window((0, 0), (6, 48))
        ));
    }

    #[test]
    fn probe_holds_on_the_release_data() {
        assert!(run(&Data::release(), 44 * 3 * 2).ok);
        assert!(run(&Data::release(), 1).ok);
        assert!(run(&Data::release(), 0).ok);
    }

    #[test]
    fn probe_misses_a_hosted_pick_that_the_ruling_does_not_allow() {
        let policy = hosted_policy();
        let data = Data {
            policy: &policy,
            ..Data::release()
        };
        assert!(!run(&data, 44 * 3).ok);
        assert!(run(&Data::release(), 44 * 3).ok);
    }

    #[test]
    fn probe_misses_a_refused_step() {
        let no_cap = Data {
            cap: None,
            ..Data::release()
        };
        assert!(!run(&no_cap, 1).ok);
        let dear = Prices::parse(
            "tier\tinput\toutput\tsource\tdate\n\
             on_device\t0\t0\tdevice\t2026-11-10\n\
             low_cost_hosted\t5000000000000\t0\tfixture\t2026-11-10\n\
             frontier\t0\t0\tfixture\t2026-11-10\n",
        )
        .expect("a price file");
        let over = Data {
            prices: &dear,
            ..Data::release()
        };
        assert!(!run(&over, 1).ok);
        let tiny = DefaultCap {
            amount: 1,
            ..release::cap().expect("the release cap")
        };
        let small = Data {
            cap: Some(tiny),
            ..Data::release()
        };
        assert!(!run(&small, 1).ok);
    }

    #[test]
    fn probe_prints_its_line() {
        assert_eq!(
            call(&["--rounds", "3"]),
            (0, "probe rounds=3 ok=yes\n".to_owned(), String::new())
        );
        assert_eq!(
            call(&["--rounds", "100000"]),
            (0, "probe rounds=100000 ok=yes\n".to_owned(), String::new())
        );
    }

    #[test]
    fn probe_prints_a_miss() {
        let policy = hosted_policy();
        let data = Data {
            policy: &policy,
            ..Data::release()
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = command_on(&data, &words(&["--rounds", "132"]), &mut out, &mut err);
        assert_eq!(
            (code, out.as_slice(), err.as_slice()),
            (1, b"probe rounds=132 ok=no\n".as_slice(), b"".as_slice())
        );
    }

    #[test]
    fn probe_fails_a_write_with_nothing_on_standard_error() {
        let mut err = Vec::new();
        let code = command(&words(&["--rounds", "3"]), &mut Broken, &mut err);
        assert_eq!((code, err.as_slice()), (1, b"".as_slice()));
    }

    #[test]
    fn probe_refusals_of_usage() {
        for (args, detail) in [
            (&[][..], "--rounds is required"),
            (&["--rounds"][..], "--rounds has no value"),
            (&["--rounds", "1", "--rounds", "2"][..], "--rounds twice"),
            (&["--seed", "1"][..], "unknown argument --seed"),
            (&["--Rounds", "1"][..], "unknown argument of another form"),
            (&["rounds", "1"][..], "unknown argument of another form"),
            (
                &["--rounds", "0"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "01"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "-1"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "+1"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", ""][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "1x"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "18446744073709551616"][..],
                "--rounds takes a whole number from 1",
            ),
        ] {
            assert_eq!(
                call(args),
                (2, String::new(), format!("REFUSED usage: {detail}\n")),
                "{args:?}"
            );
        }
    }
}

#[cfg(test)]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    struct Counting;

    pub(super) static FIRST_THREAD: AtomicUsize = AtomicUsize::new(0);
    pub(super) static FIRST_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    pub(super) static FIRST_BYTES: AtomicU64 = AtomicU64::new(0);
    pub(super) static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    pub(super) static BYTES: AtomicU64 = AtomicU64::new(0);

    thread_local! {
        static MARK: u8 = const { 0 };
    }

    pub(super) fn this_thread() -> usize {
        MARK.with(|mark| std::ptr::from_ref(mark).addr())
    }

    #[allow(unsafe_code)]
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let me = this_thread();
            let first =
                match FIRST_THREAD.compare_exchange(0, me, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => me,
                    Err(first) => first,
                };
            let (calls, bytes) = if me == first {
                (&FIRST_ALLOCATIONS, &FIRST_BYTES)
            } else {
                (&ALLOCATIONS, &BYTES)
            };
            calls.fetch_add(1, Ordering::SeqCst);
            bytes.fetch_add(layout.size() as u64, Ordering::SeqCst);
            System.alloc(layout)
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout)
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;
}
