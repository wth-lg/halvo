use std::collections::BTreeSet;

const FIRST: usize = 75174;
const LAST: usize = 83341;
const WRITES: [&str; 5] = ["memcpy", "memmove", "memset", "put2byte", "put4byte"];
const ADR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/adr/0013-database-build-zeroes-rebuilt-page-gap.md"
);

#[test]
fn every_byte_write_of_the_page_code_has_a_row_in_the_table_of_adr_0013() {
    let source = std::fs::read(env!("HALVO_SQLCIPHER_PINNED_SOURCE")).unwrap();
    let writes: BTreeSet<usize> = source
        .split(|byte| *byte == b'\n')
        .enumerate()
        .map(|(at, line)| (at + 1, String::from_utf8_lossy(line).into_owned()))
        .filter(|(number, line)| {
            (FIRST..=LAST).contains(number) && WRITES.iter().any(|word| line.contains(word))
        })
        .map(|(number, _)| number)
        .collect();
    let adr = std::fs::read_to_string(ADR).unwrap();
    let mut rows = BTreeSet::new();
    for line in adr.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if let Some(number) = cells.get(1).and_then(|cell| cell.parse::<usize>().ok()) {
            assert_eq!(
                cells.len(),
                9,
                "the row of line {number} has not seven cells"
            );
            assert!(rows.insert(number), "line {number} has two rows");
        }
    }
    let missing: Vec<&usize> = writes.difference(&rows).collect();
    let extra: Vec<&usize> = rows.difference(&writes).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "lines of the page code that write bytes and have no row: {missing:?}; rows of no such line: {extra:?}"
    );
}
