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
