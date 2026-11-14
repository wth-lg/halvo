use std::collections::BTreeSet;

const FIRST: &str = "Begin file btree.c";
const LAST: &str = "End of btree.c";
const WRITES: [&str; 5] = ["memcpy", "memmove", "memset", "put2byte", "put4byte"];
const ADR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/adr/0013-database-build-zeroes-rebuilt-page-gap.md"
);

#[test]
fn every_byte_write_of_btree_c_has_a_row_in_the_table_of_adr_0013() {
    let source = std::fs::read(env!("HALVO_SQLCIPHER_PINNED_SOURCE")).unwrap();
    let lines: Vec<(usize, String)> = source
        .split(|byte| *byte == b'\n')
        .enumerate()
        .map(|(at, line)| (at + 1, String::from_utf8_lossy(line).into_owned()))
        .collect();
    let marker = |text: &str| -> usize {
        let found: Vec<usize> = lines
            .iter()
            .filter(|(_, line)| line.contains(text))
            .map(|(number, _)| *number)
            .collect();
        assert_eq!(
            found.len(),
            1,
            "the marker line {text:?} stands {} times",
            found.len()
        );
        found[0]
    };
    let (first, last) = (marker(FIRST), marker(LAST));
    let writes: BTreeSet<usize> = lines
        .iter()
        .filter(|(number, line)| {
            (first..=last).contains(number) && WRITES.iter().any(|word| line.contains(word))
        })
        .map(|(number, _)| *number)
        .collect();
    let adr = std::fs::read_to_string(ADR).unwrap();
    let mut rows = BTreeSet::new();
    for line in adr.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if let Some(number) = cells.get(1).and_then(|cell| cell.parse::<usize>().ok()) {
            assert_eq!(
                cells.len(),
                10,
                "the row of line {number} has not eight cells"
            );
            assert!(rows.insert(number), "line {number} has two rows");
        }
    }
    let missing: Vec<&usize> = writes.difference(&rows).collect();
    let extra: Vec<&usize> = rows.difference(&writes).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "lines of btree.c that write bytes and have no row: {missing:?}; rows of no such line: {extra:?}"
    );
}
