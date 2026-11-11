use r2_probes::frame::Frame;
use r2_probes::text::{distance, error_rate, fold, join_pieces, median};
use r2_probes::tree::{walk, Tree};

#[test]
fn fold_drops_the_ends_and_folds_runs_of_white_space() {
    assert_eq!(fold("  a \t b\r\n c  "), "a b c");
    assert_eq!(fold(" \n "), "");
}

#[test]
fn pieces_are_joined_by_one_space() {
    assert_eq!(
        join_pieces(&["a  b".to_string(), " c".to_string()]),
        "a b c"
    );
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
    let dir = std::env::temp_dir().join(format!("r2-probes-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("frame.png");
    frame.save_png(&path).unwrap();
    let back = Frame::load_png(&path).unwrap();
    assert_eq!((back.width, back.height), (2, 2));
    assert_eq!(back.bgra, frame.bgra);
}
