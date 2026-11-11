use r2_probes::suite::{self, Options};
use r2_probes::trial::{self, HostedRun, How, Record};
use r2_probes::{pages, sys};
use serde_json::Value;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage:
  r2-probes suite --out DIR [--only CELL-PREFIX]... [--runs N] [--soffice PATH --soffice-version V]
            [--ocr-bin PATH --ocr-version V --ocr-arg ARG...] [--node PATH --input-dir DIR] [--debug-frames]
  r2-probes trial layer-cost|input-visibility|private-window [--artifact DIR]... [--records FILE]...
            [--lines FILE] --out FILE
  r2-probes stamp --in FILE --out FILE --cell NAME [--text KEY=VALUE]... [--number KEY=VALUE]...
  r2-probes lines layer-cost|input-visibility|private-window --crate-dir DIR
  r2-probes known TARGET
  r2-probes display";

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn values(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .collect()
}

fn options(args: &[String]) -> Result<Options, String> {
    Ok(Options {
        out: PathBuf::from(value(args, "--out").ok_or("the argument --out is missing")?),
        only: values(args, "--only"),
        runs: match value(args, "--runs") {
            Some(runs) => Some(runs.parse().map_err(|_| "--runs is not a number")?),
            None => None,
        },
        soffice: value(args, "--soffice").map(PathBuf::from),
        soffice_version: value(args, "--soffice-version").unwrap_or_default(),
        ocr_bin: value(args, "--ocr-bin").map(PathBuf::from),
        ocr_args: values(args, "--ocr-arg"),
        ocr_version: value(args, "--ocr-version").unwrap_or_default(),
        node: value(args, "--node").map(PathBuf::from),
        input_dir: value(args, "--input-dir").map(PathBuf::from),
        debug_frames: args.iter().any(|arg| arg == "--debug-frames"),
    })
}

fn trial_file(args: &[String]) -> Result<(), String> {
    let kind = args.first().map(String::as_str).unwrap_or("");
    let prefixes: &[&str] = match kind {
        "layer-cost" => &["address.", "accessibility.", "extract.", "engine."],
        "input-visibility" => &["input.", "listen-event-access."],
        "private-window" => &["private."],
        _ => return Err("the kind is layer-cost, input-visibility or private-window".to_string()),
    };
    let of_kind = |records: Vec<Record>| -> Vec<Record> {
        records
            .into_iter()
            .filter(|record| {
                prefixes
                    .iter()
                    .any(|prefix| record.cell.starts_with(prefix))
            })
            .collect()
    };
    let mut records = Vec::new();
    let mut hosted: Vec<HostedRun> = Vec::new();
    for dir in values(args, "--artifact") {
        let dir = Path::new(&dir);
        let found = of_kind(trial::read(&dir.join("records.jsonl"))?);
        if found.is_empty() {
            continue;
        }
        let meta = std::fs::read_to_string(dir.join("meta.json"))
            .map_err(|e| format!("{}: {e}", dir.join("meta.json").display()))?;
        let meta: Value = serde_json::from_str(&meta).map_err(|e| format!("meta.json: {e}"))?;
        let run: HostedRun = serde_json::from_value(meta["hosted"].clone())
            .map_err(|e| format!("meta.json: {e}"))?;
        if !hosted.contains(&run) {
            hosted.push(run);
        }
        records.extend(found);
    }
    for file in values(args, "--records") {
        records.extend(of_kind(trial::read(Path::new(&file))?));
    }
    let cells = trial::aggregate(&records);
    let other: Vec<String> = match value(args, "--lines") {
        Some(file) => std::fs::read_to_string(&file)
            .map_err(|e| format!("{file}: {e}"))?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    };
    let verdicts = match kind {
        "layer-cost" => trial::layer_verdicts(&cells),
        "input-visibility" => trial::input_verdicts(&cells),
        _ => trial::private_verdicts(&cells),
    };
    let out = PathBuf::from(value(args, "--out").ok_or("the argument --out is missing")?);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&out, trial::render(&cells, &hosted, &other, &verdicts))
        .map_err(|e| format!("{}: {e}", out.display()))?;
    println!("cells={} verdicts={}", cells.len(), verdicts.len());
    Ok(())
}

fn pair(text: &str) -> Result<(&str, &str), String> {
    text.split_once('=')
        .ok_or_else(|| format!("{text} is not KEY=VALUE"))
}

fn stamp(args: &[String]) -> Result<(), String> {
    let input = value(args, "--in").ok_or("the argument --in is missing")?;
    let out = PathBuf::from(value(args, "--out").ok_or("the argument --out is missing")?);
    let cell = value(args, "--cell").ok_or("the argument --cell is missing")?;
    let text = std::fs::read_to_string(&input).map_err(|e| format!("{input}: {e}"))?;
    let mut run = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let answer: Value = serde_json::from_str(line).map_err(|e| format!("{input}: {e}"))?;
        let mut record: Record = match answer.get("ok") {
            Some(ok) => serde_json::from_value(ok.clone()).map_err(|e| format!("{input}: {e}"))?,
            None => {
                let reason = answer["error"].as_str().unwrap_or("no record");
                trial::append(&out, &Record::missing(&cell, reason))?;
                continue;
            }
        };
        run += 1;
        record.cell = cell.clone();
        record.run = run;
        for entry in values(args, "--number") {
            let (key, number) = pair(&entry)?;
            let number: f64 = number
                .parse()
                .map_err(|_| format!("{entry} holds no number"))?;
            record = record.number(key, How::Max, number);
        }
        for entry in values(args, "--text") {
            let (key, text) = pair(&entry)?;
            record = record.text(key, text);
        }
        trial::append(&out, &record)?;
    }
    println!("records={run}");
    Ok(())
}

fn lines(args: &[String]) -> Result<(), String> {
    let kind = args.first().map(String::as_str).unwrap_or("");
    let dir =
        PathBuf::from(value(args, "--crate-dir").ok_or("the argument --crate-dir is missing")?);
    if kind == "layer-cost" {
        let output = std::process::Command::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--all-features",
            ])
            .current_dir(&dir)
            .output()
            .map_err(|e| format!("cargo metadata: {e}"))?;
        let meta: Value =
            serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
        let root = meta["resolve"]["root"].as_str().unwrap_or("");
        let empty = Vec::new();
        let node = meta["resolve"]["nodes"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .find(|node| node["id"] == root)
            .ok_or("cargo metadata names no root package")?;
        let mut crates: Vec<String> = node["deps"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|dep| {
                meta["packages"]
                    .as_array()?
                    .iter()
                    .find(|package| package["id"] == dep["pkg"])
            })
            .map(|package| {
                format!(
                    "crate {} {} {}",
                    package["name"].as_str().unwrap_or(""),
                    package["version"].as_str().unwrap_or(""),
                    package["license"].as_str().unwrap_or("")
                )
            })
            .collect();
        crates.sort();
        for line in crates {
            println!("{line}");
        }
        let list = std::fs::read_to_string(dir.join("files.tsv"))
            .map_err(|e| format!("files.tsv: {e}"))?;
        for line in list.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            if let [set, name, address, size, sha256] = fields[..] {
                println!(
                    "file {name} set={set} address={address} size_bytes={size} sha256={sha256}"
                );
            }
        }
        println!("command cells of a hosted run: sh tools/r2-probes/run-hosted.sh OUT");
        println!("command cells of this Mac: sh tools/r2-probes/retime.sh ENGINE FRAMES OUT [FILES], one set in one call of the helper for timed runs");
        println!("command deciding cell of this Mac: sh tools/r2-probes/quiet-sets.sh WORKTREE apple-vision-accurate FRAMES OUT, three sets of retime.sh in one call of the helper for timed runs");
        println!("command records of this Mac: python3 -I tools/r2-probes/stamp-cells.py WORKTREE EVIDENCE OUT, which runs r2-probes stamp once for each cell of this Mac");
        println!("note stored_bytes: the probe of run 42889376963 counted the engine's pieces joined by one space and saved that text with runs of white space folded, so in the 8 PP-OCRv6 cells of calc and writer (40 runs) the saved text is 2 to 8 bytes shorter than stored_bytes; the figures stand as measured, and the probe now saves the text that it counts, for a later run");
        println!("note version: Chrome on macos-15 updated itself during run 42889376963, so its meta.json names 157.0.8103.39 and the 9 cells address.macos-15.chrome and accessibility.macos-15.* of the four pages name 157.0.8103.40, which the probe read from the build on disk when it listed the browsers before each group of cells, and the extraction runs in Chrome name no build; the figures stand as measured, and the probe now records in each run the build on disk when that run started the browser (browser_version in an extraction run), for a later run");
    } else if kind == "input-visibility" {
        println!("command cells of a hosted run: sh tools/r2-probes/run-hosted.sh OUT");
    } else if kind == "private-window" {
        println!("command cells of a hosted run: sh tools/r2-probes/run-hosted.sh OUT");
        println!("note version: Chrome on macos-15 updated itself during run 42889376963, so its meta.json names 157.0.8103.39 and the cell private.macos-15.chrome names 157.0.8103.40, which the probe read from the build on disk when it listed the browsers before the private-window cells; the figures stand as measured, and the probe now records in each run the build on disk when that run started the browser, for a later run");
    } else {
        return Err("the kind is layer-cost, input-visibility or private-window".to_string());
    }
    println!("command this file: r2-probes trial {kind} --artifact MACOS --artifact WINDOWS [--records THIS-MAC] --lines LINES --out FILE");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("child") => std::process::exit(suite::child(&args[1..])),
        Some("suite") => options(&args[1..]).and_then(suite::run),
        Some("trial") => trial_file(&args[1..]),
        Some("stamp") => stamp(&args[1..]),
        Some("lines") => lines(&args[1..]),
        Some("known") => match args.get(1).and_then(|target| pages::known_text(target)) {
            Some(text) => {
                println!("{text}");
                Ok(())
            }
            None => Err("no such page or document".to_string()),
        },
        Some("display") => {
            let (width, height) = sys::display_size();
            println!("{width}x{height}");
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    };
    if let Err(error) = result {
        eprintln!("REFUSED usage: {error}");
        std::process::exit(2);
    }
}
