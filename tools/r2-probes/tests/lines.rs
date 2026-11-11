use std::path::{Path, PathBuf};
use std::process::Command;

const CRATE_DIR: &str = env!("CARGO_MANIFEST_DIR");

fn lines(kind: &str, extra: &[&str]) -> Vec<String> {
    let mut path: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    if let Some(dir) = Path::new(env!("CARGO")).parent() {
        path.insert(0, dir.to_path_buf());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_r2-probes"))
        .args(["lines", kind, "--crate-dir", CRATE_DIR])
        .args(extra)
        .env("PATH", std::env::join_paths(path).expect("PATH"))
        .output()
        .expect("r2-probes lines");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8")
        .lines()
        .map(str::to_string)
        .collect()
}

fn commands(lines: &[String]) -> Vec<&String> {
    lines
        .iter()
        .filter(|line| line.starts_with("command "))
        .collect()
}

#[test]
fn the_command_lines_name_every_script_that_made_a_cell_of_this_mac() {
    let lines = lines("layer-cost", &[]);
    let commands = commands(&lines);
    for script in [
        "tools/r2-probes/retime.sh",
        "tools/r2-probes/quiet-sets.sh",
        "tools/r2-probes/stamp-cells.py",
    ] {
        assert!(
            commands.iter().any(|line| line.contains(script)),
            "no command line names {script}"
        );
    }
}

#[test]
fn every_file_that_a_command_line_names_is_in_the_crate() {
    for kind in ["layer-cost", "input-visibility", "private-window"] {
        let lines = lines(kind, &[]);
        for line in commands(&lines) {
            for word in line.split([' ', ',']) {
                if let Some(rest) = word.strip_prefix("tools/r2-probes/") {
                    assert!(
                        Path::new(CRATE_DIR).join(rest).is_file(),
                        "{word}, named in `{line}`, is not in the crate"
                    );
                }
            }
        }
    }
}

fn file_line<'a>(lines: &'a [String], name: &str) -> &'a str {
    let prefix = format!("file {name} ");
    lines
        .iter()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no file line for {name}"))
}

fn key<'a>(line: &'a str, name: &str) -> &'a str {
    let prefix = format!("{name}=");
    line.split(' ')
        .find_map(|word| word.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no key {name} in `{line}`"))
}

#[test]
fn the_dictionary_line_says_where_the_file_comes_from() {
    let lines = lines("layer-cost", &[]);
    let source = key(file_line(&lines, "ppocrv6_tiny_dict.txt"), "source");
    for word in ["v0.7.0", "oar-ocr", "0.10.2", "PaddlePaddle"] {
        assert!(source.contains(word), "{word} is not in source={source}");
    }
}

const PLAYWRIGHT_FILES: &str = "https://cdn.playwright.dev/builds/cft/158.0.8127.18/win64/chrome-win64.zip\t215820284\tb796e8a6231463d1a150ee679fd7cb0168b3728162d2d19bf7bbd867d60d55e2
https://cdn.playwright.dev/dbazure/download/playwright/builds/ffmpeg/1013/ffmpeg-win64.zip\t1663577\t0c4ee6c694e9bffea27128ab543ae573466c5245d7d16c054e40115d7d743874
https://cdn.playwright.dev/builds/cft/158.0.8127.18/win64/chrome-headless-shell-win64.zip\t129186142\tfdd3721ad0299ddd419d170ce8ab89aecf8631d157b85215474b4a70c8dcd061
https://cdn.playwright.dev/dbazure/download/playwright/builds/winldd/1007/winldd-win64.zip\t128684\t0069f0d11d4ad6df068a068c003d22fe7dbec192a47bba64b2e115e9c8ce41d8
";

fn input_lines() -> Vec<String> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lines");
    std::fs::create_dir_all(&dir).expect("test directory");
    let list = dir.join("playwright-files.tsv");
    std::fs::write(&list, PLAYWRIGHT_FILES).expect("write the Playwright list");
    lines(
        "input-visibility",
        &["--playwright-files", &list.display().to_string()],
    )
}

#[test]
fn each_playwright_download_says_where_it_comes_from_and_how_it_was_hashed() {
    let lines = input_lines();
    for name in [
        "chrome-win64.zip",
        "ffmpeg-win64.zip",
        "chrome-headless-shell-win64.zip",
        "winldd-win64.zip",
    ] {
        let line = file_line(&lines, name);
        assert_eq!(key(line, "set"), "playwright-1.65.0");
        assert!(
            key(line, "source").contains("second_download_with_curl"),
            "`{line}`"
        );
    }
    for name in ["chrome-win64.zip", "chrome-headless-shell-win64.zip"] {
        let source = key(file_line(&lines, name), "source");
        assert!(source.contains("Chrome_for_Testing"), "source={source}");
        assert!(source.contains("158.0.8127.18"), "source={source}");
    }
}

#[test]
fn node_and_the_two_npm_tarballs_have_file_lines_that_match_the_lock() {
    let lines = input_lines();
    let node = file_line(&lines, "node-26.13.0-win32-x64.7z");
    assert!(key(node, "address").starts_with("https://github.com/actions/node-versions/"));
    let lock: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(Path::new(CRATE_DIR).join("input/package-lock.json"))
            .expect("package-lock.json"),
    )
    .expect("lock JSON");
    for package in ["playwright", "playwright-core"] {
        let entry = &lock["packages"][format!("node_modules/{package}")];
        let line = file_line(&lines, &format!("{package}-1.65.0.tgz"));
        assert_eq!(Some(key(line, "address")), entry["resolved"].as_str());
        assert_eq!(Some(key(line, "integrity")), entry["integrity"].as_str());
        assert_eq!(key(line, "sha256").len(), 64, "`{line}`");
        assert!(key(line, "size_bytes").parse::<u64>().is_ok(), "`{line}`");
    }
}
