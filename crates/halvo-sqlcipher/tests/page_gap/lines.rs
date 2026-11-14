//! The closed list of the lines that rules.rs can print (S-87 G1 (p)): each
//! line has an entry that names the control which makes its fault and requires
//! it, as table.rs holds the rows of ADR 0013. A test fails when a line of
//! rules.rs has no entry, when an entry names no line, or when the named
//! control does not stand or does not hold a fragment of the line's words.

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/page_gap");

/// (the line as rules.rs writes it, the control: `file::function`, a fragment
/// of the line that the control's body holds).
const LIST: [(&str, &str, &str); 17] = [
    ("page {number}: type byte {other} is no b-tree page", "rules::plants", "is no b-tree page"),
    ("page {number}: byte {at} of the freeblock at {free} (size {size}) is {:#04x}", "controls::four_rules", "of the freeblock at"),
    ("page {number}: byte {at} of the content area, in no cell and no freeblock, is {:#04x}", "controls::four_rules", "of the content area, in no cell and no freeblock"),
    ("page {number}: the gap's slot at {at} holds {stale:#06x}, no offset of a cell past the slot ({}..={})", "controls::four_rules", "no offset of a cell past the slot"),
    ("page {number}: byte {left} of the gap {pointers_end}..{content}, past its run of stale pointers that ends at {at}, is {:#04x}", "controls::four_rules", "past its run of stale pointers that ends at"),
    ("page 1: page size {size} above 4,096: the check reads no page of such a file", "rules::plants", "page size 8192 above 4,096"),
    ("page 1: header byte {at} is {:#04x}", "controls::a_low_and_a_high_byte_in_a_freelist_leaf_and_in_the_header_are_named", "header byte {at} is"),
    ("page 1: header byte {at}, reserved for expansion, is {:#04x}", "controls::a_low_and_a_high_byte_in_a_freelist_leaf_and_in_the_header_are_named", "reserved for expansion, is"),
    ("page {trunk}: freelist trunk names page {leaf}", "controls::a_draw_that_frees_a_second_trunk_leaves_no_text_and_its_plants_are_named", "freelist trunk names page"),
    ("page {leaf}: byte {at} of a freelist leaf is {:#04x}", "controls::a_low_and_a_high_byte_in_a_freelist_leaf_and_in_the_header_are_named", "of a freelist leaf is"),
    ("page {trunk}: the freelist trunk past its count holds at {at} a value above the page count", "controls::the_page_count_passes_in_a_trunk_and_a_tail_and_the_count_plus_1_is_named", "the freelist trunk past its count holds at"),
    ("page {number}: page type {kind}", "controls::a_leaf_whose_type_byte_is_0_on_disk_is_named_as_a_page_that_dbstat_reads_corrupted", "page type corrupted"),
    ("page {number}: in no tree and not on the freelist", "rules::plants", "in no tree and not on the freelist"),
    ("page 1: the check read {read} of the {count} pages of the file as a b-tree page, an overflow page, a freelist trunk or a freelist leaf", "rules::plants", "pages\", count + 1"),
    ("page {number}: the tail of an overflow page holds at {at} a value above the page count", "controls::the_page_count_passes_in_a_trunk_and_a_tail_and_the_count_plus_1_is_named", "the tail of an overflow page holds at"),
    ("page {number}: an overflow page that {walked} chains of cells walk, not one", "controls::a_cell_that_takes_the_overflow_chain_of_another_names_a_page_of_2_chains_and_a_page_of_0", "chains of cells walk, not one"),
    ("the log of the connection's file holds {length} bytes: the check reads the main file alone, so checkpoint the log with TRUNCATE first", "controls::the_check_names_a_log_of_the_connections_file_that_holds_frames", "the log of the connection's file holds "),
];

/// The first string literal after each `problems.push(format!(` of a source,
/// with its line continuations joined as the compiler joins them.
fn pushed_lines(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("problems.push(format!(") {
        rest = &rest[at + "problems.push(format!(".len()..];
        let start = rest.find('"').expect("a string literal after format!(") + 1;
        let mut text = String::new();
        let mut chars = rest[start..].chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('\n') => {
                        let tail: String = chars.clone().collect();
                        let skip = tail.len() - tail.trim_start().len();
                        for _ in 0..tail[..skip].chars().count() {
                            chars.next();
                        }
                    }
                    Some(other) => {
                        text.push('\\');
                        text.push(other);
                    }
                    None => break,
                },
                other => text.push(other),
            }
        }
        lines.push(text.replace("\\\"", "\""));
    }
    lines
}

/// The body of `fn name(` in `source`: from its line to the next line that
/// starts a test or a function at the left margin.
fn body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let at = source
        .find(&format!("fn {name}("))
        .filter(|&at| at == 0 || source[..at].ends_with('\n') || source[..at].ends_with("pub "))?;
    let rest = &source[at..];
    let end = ["\n#[test]", "\nfn ", "\npub fn ", "\npub(super) fn "]
        .iter()
        .filter_map(|next| rest[1..].find(next).map(|end| end + 1))
        .min()
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

#[test]
fn every_line_that_rules_rs_can_print_has_a_control_in_the_closed_list() {
    let rules = std::fs::read_to_string(format!("{DIR}/rules.rs")).unwrap();
    let printed = pushed_lines(&rules);
    let listed: Vec<&str> = LIST.iter().map(|(line, _, _)| *line).collect();
    let unlisted: Vec<&String> = printed
        .iter()
        .filter(|line| !listed.contains(&line.as_str()))
        .collect();
    let unprinted: Vec<&&str> = listed
        .iter()
        .filter(|line| !printed.iter().any(|p| p == **line))
        .collect();
    assert!(
        unlisted.is_empty() && unprinted.is_empty() && printed.len() == LIST.len(),
        "lines of rules.rs with no control in the list: {unlisted:#?}; entries of no line: {unprinted:#?}"
    );
    let mut faults = Vec::new();
    for (line, control, fragment) in LIST {
        let (file, name) = control.split_once("::").unwrap();
        let source = std::fs::read_to_string(format!("{DIR}/{file}.rs")).unwrap();
        match body(&source, name) {
            None => faults.push(format!("{line:?}: no function {control}")),
            Some(text) if !text.contains(fragment) => {
                faults.push(format!("{line:?}: {control} holds no {fragment:?}"))
            }
            Some(_) => {}
        }
    }
    let layout = std::fs::read_to_string(format!("{DIR}/layout.rs")).unwrap();
    if !layout.contains("rules::plants(") {
        faults.push("no test of layout.rs runs rules::plants".to_owned());
    }
    let controls = std::fs::read_to_string(format!("{DIR}/controls.rs")).unwrap();
    if !body(&controls, "the_four_rules_of_a_tree_page_name_their_plants_on_each_kind_of_page_with_a_low_and_a_high_byte")
        .is_some_and(|text| text.contains("four_rules("))
    {
        faults.push("no test of controls.rs runs four_rules".to_owned());
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}
