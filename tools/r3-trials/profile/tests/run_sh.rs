use std::path::{Path, PathBuf};
use std::process::Command;

const STAND_IN: &str = "#!/bin/sh\necho \"$0\" >>\"$R3_STAND_IN_CALLS\"\nexit 1\n";

fn write(path: &Path, text: &str, program: bool) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    #[cfg(unix)]
    if program {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    #[cfg(not(unix))]
    let _ = program;
}

struct Places {
    bin: PathBuf,
    calls: PathBuf,
    plain: PathBuf,
    program: PathBuf,
    missing: PathBuf,
}

fn places() -> Places {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("run-sh-reader");
    let bin = root.join("bin");
    for d in [&bin, &root.join("plain"), &root.join("program")] {
        std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
    }
    for name in ["node", "cargo", "rustc"] {
        write(&bin.join(name), STAND_IN, true);
    }
    let plain = root.join("plain").join("chrome");
    write(&plain, "no program\n", false);
    let program = root.join("program").join("chrome");
    write(&program, "#!/bin/sh\nexit 0\n", true);
    Places {
        bin,
        calls: root.join("calls"),
        plain,
        program,
        missing: root.join("missing").join("chrome"),
    }
}

fn run(p: &Places, args: &[&str]) -> (i32, String, bool) {
    write(&p.calls, "", false);
    let run_sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("run.sh");
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![p.bin.clone()];
    dirs.extend(std::env::split_paths(&path));
    let out = Command::new("sh")
        .arg(&run_sh)
        .args(args)
        .env("PATH", std::env::join_paths(dirs).expect("a PATH"))
        .env("R3_STAND_IN_CALLS", &p.calls)
        .output()
        .expect("sh starts");
    let err = String::from_utf8_lossy(&out.stderr);
    let refused = err
        .lines()
        .find_map(|l| l.strip_prefix("REFUSED "))
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("-")
        .to_string();
    let called = std::fs::read(&p.calls)
        .map(|b| !b.is_empty())
        .unwrap_or(true);
    (out.status.code().unwrap_or(-1), refused, called)
}

type Row<'a> = (&'a str, Vec<&'a str>, (i32, &'a str, bool));

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn the_argument_reader_follows_the_usage_line_of_a13_3() {
    let p = places();
    let (plain, program, missing) = (s(&p.plain), s(&p.program), s(&p.missing));
    let driver = s(&p.bin.join("driver"));
    let rows: Vec<Row> = vec![
        (
            "a_file_named_chrome_that_is_no_program_is_refused_as_a_browser",
            vec!["--item", "1", "--browser", &plain, "--port", "18191"],
            (2, "browser", false),
        ),
        (
            "driver_is_taken_and_the_browser_decides_the_refusal",
            vec![
                "--item",
                "1",
                "--browser",
                &missing,
                "--port",
                "18191",
                "--driver",
                &driver,
            ],
            (2, "browser", false),
        ),
        (
            "driver_is_taken_and_the_reader_goes_on",
            vec![
                "--item",
                "1",
                "--browser",
                &program,
                "--port",
                "18191",
                "--driver",
                &driver,
            ],
            (2, "port", true),
        ),
        (
            "driver_with_no_value_is_refused",
            vec![
                "--item",
                "1",
                "--browser",
                &program,
                "--port",
                "18191",
                "--driver",
            ],
            (2, "usage", false),
        ),
        (
            "item_3_with_another_port_is_refused_as_a_port",
            vec!["--item", "3", "--browser", &program, "--port", "18191"],
            (2, "port", false),
        ),
        (
            "item_3_with_another_port_and_no_browser_is_refused_as_a_browser",
            vec!["--item", "3", "--browser", &missing, "--port", "18191"],
            (2, "browser", false),
        ),
        (
            "item_3_on_its_port_goes_on",
            vec!["--item", "3", "--browser", &program, "--port", "18090"],
            (2, "port", true),
        ),
        (
            "an_unknown_argument_is_refused",
            vec![
                "--item",
                "1",
                "--browser",
                &program,
                "--port",
                "18191",
                "--other",
                "x",
            ],
            (2, "usage", false),
        ),
    ];
    let mut wrong = Vec::new();
    for (name, args, (code, word, called)) in &rows {
        let got = run(&p, args);
        if got != (*code, word.to_string(), *called) {
            wrong.push(format!(
                "{name}: got exit {} REFUSED {} stand_in_called={}, want exit {code} REFUSED {word} stand_in_called={called}",
                got.0, got.1, got.2
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_reader_hands_each_argument_on_to_the_program() {
    let p = places();
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("run-sh-reader");
    let pass = root.join("pass-bin");
    let scratch = root.join("scratch");
    for d in [&pass, &scratch] {
        std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
    }
    for name in ["node", "cargo"] {
        write(&pass.join(name), "#!/bin/sh\nexit 0\n", true);
    }
    write(
        &pass.join("rustc"),
        "#!/bin/sh\necho 'host: stand-in'\n",
        true,
    );
    let program = s(&p.program);
    let scratch = s(&scratch);
    let rows: [(&str, &[&str], &[&str]); 3] = [
        (
            "the_first_set",
            &["--item", "3", "--flags", "first", "--port", "18090"],
            &["--item 3 ", " --flags first"],
        ),
        (
            "the_control",
            &["--item", "3", "--sandbox-off", "--port", "18090"],
            &["--item 3 ", " --sandbox-off"],
        ),
        (
            "a_cookie_left_out",
            &["--item", "1", "--leave-out", "sid", "--port", "18191"],
            &["--item 1 ", " --leave-out sid"],
        ),
    ];
    let mut wrong = Vec::new();
    for (name, head, want) in rows {
        let mut args: Vec<&str> = head.to_vec();
        args.extend(["--browser", &program, "--scratch", &scratch]);
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut dirs = vec![pass.clone()];
        dirs.extend(std::env::split_paths(&path));
        let out = Command::new("sh")
            .arg("-x")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("run.sh"))
            .args(&args)
            .env("PATH", std::env::join_paths(dirs).expect("a PATH"))
            .env("RUSTC", pass.join("rustc"))
            .output()
            .expect("sh starts");
        let err = String::from_utf8_lossy(&out.stderr);
        let exec = err
            .lines()
            .rfind(|l| {
                l.trim_start_matches('+').starts_with(" exec ") && l.contains("profile-trial")
            })
            .unwrap_or("")
            .to_string();
        let ok = want.iter().all(|w| exec.contains(w))
            && !(name == "the_first_set" && exec.contains("--flags final"));
        if !ok {
            wrong.push(format!("{name}: the traced exec line is [{exec}]"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
