use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use r2_probes::frame::Frame;
use r2_probes::text::{distance, error_rate, fold, median};
use r2_probes::tree::{walk, Tree};

#[test]
fn fold_drops_the_ends_and_folds_runs_of_white_space() {
    assert_eq!(fold("  a \t b\r\n c  "), "a b c");
    assert_eq!(fold(" \n "), "");
}

#[test]
fn distance_counts_edits() {
    let chars = |s: &str| s.chars().collect::<Vec<char>>();
    assert_eq!(distance(&chars("kitten"), &chars("sitting")), 3);
    assert_eq!(distance(&chars(""), &chars("abc")), 3);
    assert_eq!(distance(&chars("abc"), &chars("")), 3);
    assert_eq!(distance(&chars("same"), &chars("same")), 0);
}

#[test]
fn error_rate_counts_unicode_scalar_values_of_the_folded_texts() {
    let rate = error_rate("  naive\n", "na\u{ef}ve").unwrap();
    assert_eq!(rate.distance, 1);
    assert_eq!(rate.known_chars, 5);
    assert!((rate.percent() - 20.0).abs() < 1e-9);
    let exact = error_rate("one  two", " one two ").unwrap();
    assert_eq!(exact.distance, 0);
    assert_eq!(exact.known_chars, 7);
}

#[test]
fn error_rate_needs_a_known_text() {
    assert!(error_rate("text", "  ").is_none());
}

#[test]
fn median_takes_the_middle_or_the_mean_of_the_two_middles() {
    assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
    assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
    assert_eq!(median(&[]), None);
}

struct Fake {
    nodes: Vec<(String, String, Vec<usize>)>,
}

impl Fake {
    fn add(&mut self, name: &str, value: &str, parent: Option<usize>) -> usize {
        let id = self.nodes.len();
        self.nodes
            .push((name.to_string(), value.to_string(), Vec::new()));
        if let Some(parent) = parent {
            self.nodes[parent].2.push(id);
        }
        id
    }
}

impl Tree for Fake {
    type Node = usize;
    fn name(&self, node: &usize) -> String {
        self.nodes[*node].0.clone()
    }
    fn value(&self, node: &usize) -> String {
        self.nodes[*node].1.clone()
    }
    fn children(&self, node: &usize) -> Vec<usize> {
        self.nodes[*node].2.clone()
    }
}

#[test]
fn walk_writes_a_parent_before_its_children_in_document_order() {
    let mut tree = Fake { nodes: Vec::new() };
    let window = tree.add("Window", "", None);
    let group = tree.add("", "", Some(window));
    tree.add("First", "1", Some(group));
    tree.add("", "only a value", Some(group));
    tree.add("Tab\there", "line\r\nbreak", Some(window));
    let read = walk(&tree, &window);
    assert_eq!(
        read.text,
        "Window\nFirst\t1\n\tonly a value\nTab here\tline  break"
    );
    assert_eq!(read.lines, 4);
    assert_eq!(read.visited, 5);
}

#[test]
fn walk_leaves_out_a_node_below_level_16() {
    let mut tree = Fake { nodes: Vec::new() };
    let root = tree.add("level 1", "", None);
    let mut parent = root;
    for level in 2..=20 {
        parent = tree.add(&format!("level {level}"), "", Some(parent));
    }
    let read = walk(&tree, &root);
    assert_eq!(read.lines, 16);
    assert!(read.text.ends_with("level 16"));
    assert_eq!(read.visited, 16);
}

#[test]
fn walk_ends_after_1600_lines_and_does_not_count_empty_nodes() {
    let mut tree = Fake { nodes: Vec::new() };
    let root = tree.add("", "", None);
    for index in 0..2000 {
        if index % 2 == 0 {
            tree.add("", "", Some(root));
        }
        tree.add(&format!("row {index}"), "", Some(root));
    }
    let read = walk(&tree, &root);
    assert_eq!(read.lines, 1600);
    assert_eq!(read.text.lines().count(), 1600);
    assert!(read.text.ends_with("row 1599"));
    assert!(!read.text.ends_with('\n'));
}

#[test]
fn halve_averages_blocks_of_two_by_two_pixels() {
    let frame = Frame {
        width: 4,
        height: 2,
        bgra: vec![
            0, 0, 0, 255, 100, 100, 100, 255, 10, 20, 30, 255, 10, 20, 30, 255, 100, 100, 100, 255,
            200, 200, 200, 255, 10, 20, 30, 255, 10, 20, 30, 255,
        ],
    };
    let half = frame.halve();
    assert_eq!((half.width, half.height), (2, 1));
    assert_eq!(half.bgra, vec![100, 100, 100, 255, 10, 20, 30, 255]);
}

#[test]
fn a_frame_survives_a_png_file() {
    let frame = Frame {
        width: 2,
        height: 2,
        bgra: vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255],
    };
    let held = leaf("r2-probes-frame");
    let path = held.path().join("frame.png");
    frame.save_png(&path).unwrap();
    let back = Frame::load_png(&path).unwrap();
    assert_eq!((back.width, back.height), (2, 2));
    assert_eq!(back.bgra, frame.bgra);
}

struct Leaf(PathBuf);

impl Leaf {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Leaf {
    fn drop(&mut self) {
        if let Some(line) = remove(&self.0) {
            let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("{line}\n"));
        }
    }
}

fn remove(dir: &Path) -> Option<String> {
    let error = std::fs::remove_dir_all(dir).err()?;
    Some(format!("leaf not removed: {}: {error}", dir.display()))
}

fn leaf_name(word: &str, pid: u32, clock: u128, count: u64) -> String {
    format!("{word}-{pid}-{clock}-{count}")
}

fn make_leaf(dir: PathBuf) -> Result<Leaf, String> {
    let parent = dir
        .parent()
        .ok_or_else(|| format!("no parent: {}", dir.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Leaf(dir))
}

fn leaf(word: &str) -> Leaf {
    static CLOCK: OnceLock<u128> = OnceLock::new();
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let clock = *CLOCK.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    });
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let name = leaf_name(word, std::process::id(), clock, count);
    make_leaf(Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn leaf_two_calls_give_two_paths() {
    let first = leaf("r2-probes-leaf");
    let second = leaf("r2-probes-leaf");
    assert_ne!(first.path(), second.path());
    assert!(first.path().is_dir() && second.path().is_dir());
    let stem = |held: &Leaf| {
        let name = held
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        name[..name.rfind('-').unwrap()].to_owned()
    };
    assert_eq!(stem(&first), stem(&second));
}

#[test]
fn leaf_name_differs_by_the_clock_reading_the_counter_and_the_process_id() {
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 7, 2, 0));
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 7, 1, 1));
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 8, 1, 0));
    assert!(leaf_name("w", 7, 1, 0).starts_with("w-7-"));
}

#[test]
fn leaf_that_exists_fails_with_its_path() {
    let first = leaf("r2-probes-leaf");
    let error = make_leaf(first.path().to_path_buf())
        .err()
        .expect("a second making of one leaf fails");
    assert!(
        error.contains(&first.path().display().to_string()),
        "{error}"
    );
}

#[test]
fn leaf_is_gone_after_its_drop() {
    let held = leaf("r2-probes-leaf");
    let path = held.path().to_path_buf();
    std::fs::write(path.join("file"), "x").unwrap();
    drop(held);
    assert!(!path.exists(), "{}", path.display());
}

#[test]
fn leaf_removal_that_fails_gives_one_line_with_the_path() {
    let held = leaf("r2-probes-leaf");
    let path = held.path().to_path_buf();
    drop(held);
    let line = remove(&path).expect("a line for a removal that fails");
    assert!(
        line.contains(&path.display().to_string()) && !line.contains('\n'),
        "{line}"
    );
}

#[test]
fn leaf_lies_under_the_target_directory() {
    let held = leaf("r2-probes-leaf");
    assert!(
        held.path().starts_with(env!("CARGO_TARGET_TMPDIR")),
        "{}",
        held.path().display()
    );
}

const LEAF_CHILD: &str = "HALVO_LEAF_CHILD";

fn leaf_child(test: &str, role: &str, nocapture: bool) -> std::process::Output {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([test, "--exact", "--test-threads=1"])
        .env(LEAF_CHILD, role);
    if nocapture {
        command.arg("--nocapture");
    }
    let out = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("test result: ok. 1 passed;"),
        "{role}: {}\n{stdout}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn leaf_parts(path: &Path) -> (String, u32, u128, u64) {
    let name = path.file_name().unwrap().to_str().unwrap();
    let mut parts = name.rsplitn(4, '-');
    let count = parts.next().unwrap_or_default().parse();
    let clock = parts.next().unwrap_or_default().parse();
    let pid = parts.next().unwrap_or_default().parse();
    let word = parts.next().unwrap_or_default().to_owned();
    match (pid, clock, count) {
        (Ok(pid), Ok(clock), Ok(count)) => (word, pid, clock, count),
        _ => panic!("not word-pid-clock-count: {name}"),
    }
}

fn leaf_clock_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn leaf_from_leaf_holds_the_word_the_process_id_and_the_clock() {
    if std::env::var(LEAF_CHILD).as_deref() != Ok("clock") {
        leaf_child(
            "leaf_from_leaf_holds_the_word_the_process_id_and_the_clock",
            "clock",
            false,
        );
        return;
    }
    let before = leaf_clock_now();
    let held = leaf("r2-probes-leaf-clock");
    let after = leaf_clock_now();
    let (word, pid, clock, _) = leaf_parts(held.path());
    assert_eq!(word, "r2-probes-leaf-clock");
    assert_eq!(pid, std::process::id());
    assert!(
        before <= clock && clock <= after,
        "{before} {clock} {after}"
    );
}

#[test]
fn leaf_refuses_the_directory_that_the_next_leaf_names() {
    if std::env::var(LEAF_CHILD).as_deref() != Ok("next") {
        leaf_child(
            "leaf_refuses_the_directory_that_the_next_leaf_names",
            "next",
            false,
        );
        return;
    }
    let first = leaf("r2-probes-leaf-next");
    let (word, pid, clock, count) = leaf_parts(first.path());
    let next = first
        .path()
        .with_file_name(format!("{word}-{pid}-{clock}-{}", count + 1));
    std::fs::create_dir(&next).unwrap();
    let _next = Leaf(next.clone());
    std::fs::write(next.join("kept"), "kept\n").unwrap();
    let caught = std::panic::catch_unwind(|| leaf("r2-probes-leaf-next"));
    let message = caught
        .err()
        .and_then(|payload| payload.downcast::<String>().ok())
        .expect("leaf() fails on the directory that it names");
    assert!(message.contains(&next.display().to_string()), "{message}");
    assert_eq!(
        std::fs::read_to_string(next.join("kept")).unwrap(),
        "kept\n"
    );
    assert_eq!(std::fs::read_dir(&next).unwrap().count(), 1);
}

#[test]
fn leaf_whose_path_is_gone_drops_without_a_panic() {
    let held = leaf("r2-probes-leaf-gone");
    std::fs::remove_dir_all(held.path()).unwrap();
    let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(held)));
    assert!(dropped.is_ok());
}

#[test]
fn leaf_removal_line_reaches_the_standard_error_of_the_process() {
    if std::env::var(LEAF_CHILD).as_deref() == Ok("line") {
        let held = leaf("r2-probes-leaf-line");
        std::fs::remove_dir_all(held.path()).unwrap();
        drop(held);
        return;
    }
    let place = Path::new(env!("CARGO_TARGET_TMPDIR")).join("r2-probes-leaf-line-");
    for nocapture in [false, true] {
        let out = leaf_child(
            "leaf_removal_line_reaches_the_standard_error_of_the_process",
            "line",
            nocapture,
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<&str> = stderr
            .lines()
            .filter(|line| line.starts_with("leaf not removed: "))
            .collect();
        assert_eq!(lines.len(), 1, "nocapture {nocapture}: {stderr}");
        assert!(
            lines[0].contains(&place.display().to_string()),
            "{}",
            lines[0]
        );
    }
}
