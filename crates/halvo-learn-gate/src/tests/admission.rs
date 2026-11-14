const ADMISSION: &str = include_str!("../../../../docs/r5/trials/admission.md");

/// One case line of `docs/r5/trials/admission.md` (contracts.md L7.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub case: String,
    pub arm: String,
    pub sort: String,
    pub occurrences: u32,
    pub drifts: u32,
    pub assisted: u32,
    pub failed: u32,
    pub harm: u32,
    pub reverted_good: u32,
}

fn field<'a>(pairs: &'a [(&'a str, &'a str)], key: &str) -> &'a str {
    pairs
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("a case line without {key}"))
}

/// The 64 case lines of the trial's block.
pub fn admission() -> Vec<Line> {
    let lines: Vec<Line> = ADMISSION
        .lines()
        .filter_map(|line| line.strip_prefix("case "))
        .map(|rest| {
            let pairs: Vec<(&str, &str)> = rest
                .split(' ')
                .map(|pair| pair.split_once('=').expect("a key=value pair"))
                .collect();
            let number = |key| field(&pairs, key).parse().expect("a count");
            Line {
                case: field(&pairs, "name").to_owned(),
                arm: field(&pairs, "arm").to_owned(),
                sort: field(&pairs, "sort").to_owned(),
                occurrences: number("occurrences"),
                drifts: number("drifts"),
                assisted: number("assisted"),
                failed: number("failed"),
                harm: number("harm"),
                reverted_good: number("reverted_good"),
            }
        })
        .collect();
    assert_eq!(lines.len(), 64, "admission.md holds 64 case lines");
    lines
}
