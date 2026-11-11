use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use calamine::Data;
use r3_sheet_trial::book;
use r3_sheet_trial::corpus;
use r3_sheet_trial::parts::{self, Kind};

fn corpus_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

#[test]
fn the_generator_writes_the_committed_corpus() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("corpus");
    corpus::generate(&dir).unwrap();
    for file in corpus::FILES {
        let fresh = std::fs::read(dir.join(file)).unwrap();
        let committed = std::fs::read(corpus_dir().join(file)).unwrap();
        assert!(
            fresh == committed,
            "{file} differs from the generator's output"
        );
    }
}

#[test]
fn formulas_ledger_holds_the_cells_of_a13_4() {
    let read = book::read(&corpus_dir().join("formulas.xlsx")).unwrap();
    assert_eq!(read.sheets.len(), 1);
    let (name, cells) = &read.sheets[0];
    assert_eq!(name, "Ledger");
    for col in 0..4u32 {
        let header = cells.get(&(0, col)).unwrap();
        assert!(matches!(&header.value, Data::String(s) if !s.is_empty()));
        assert!(header.formula.is_none());
    }
    for row in 1..=5u32 {
        let id = cells.get(&(row, 0)).unwrap();
        assert_eq!(id.value, Data::String(format!("INV-{row}")));
        for col in 1..4u32 {
            assert!(
                cells.get(&(row, col)).is_none(),
                "{}",
                book::cell_ref((row, col))
            );
        }
    }
    let total = cells.get(&(6, 3)).unwrap();
    assert_eq!(total.formula.as_deref(), Some("SUM(D2:D6)"));
    assert_eq!(cells.len(), 4 + 5 + 1);
}

#[test]
fn each_workbook_holds_its_part_kinds() {
    let kinds = |file: &str| parts::inventory(&corpus_dir().join(file)).unwrap().kinds();
    assert_eq!(kinds("values.xlsx"), vec![]);
    assert_eq!(kinds("formulas.xlsx"), vec![]);
    assert_eq!(kinds("formats.xlsx"), vec![Kind::Format]);
    assert_eq!(kinds("merged.xlsx"), vec![Kind::Merged]);
    assert_eq!(kinds("validation.xlsx"), vec![Kind::Validation]);
    assert_eq!(
        kinds("chart.xlsx"),
        vec![Kind::Chart, Kind::Drawing, Kind::Table, Kind::Comment]
    );
    assert_eq!(kinds("two-sheets.xlsx"), vec![Kind::DefinedName]);
    assert_eq!(kinds("rows-5000.xlsx"), vec![]);
}

#[test]
fn two_sheets_has_a_cross_sheet_formula_and_a_defined_name() {
    let path = corpus_dir().join("two-sheets.xlsx");
    let read = book::read(&path).unwrap();
    let names: Vec<&str> = read.sheets.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["Input", "Summary"]);
    let total = read.cell("Summary", (0, 1)).unwrap();
    assert_eq!(total.formula.as_deref(), Some("SUM(Input!B2:B5)"));
    let inventory = parts::inventory(&path).unwrap();
    assert_eq!(
        inventory.instances[&Kind::DefinedName],
        vec!["Rate||Input!$B$6".to_string()]
    );
}

#[test]
fn rows_5000_has_five_thousand_data_rows() {
    let read = book::read(&corpus_dir().join("rows-5000.xlsx")).unwrap();
    let cells = read.sheet("Rows").unwrap();
    let data_rows: std::collections::BTreeSet<u32> = cells
        .keys()
        .map(|(row, _)| *row)
        .filter(|row| *row > 0)
        .collect();
    assert_eq!(data_rows.len(), 5000);
    assert_eq!(data_rows.last(), Some(&5000));
}

type At = (String, (u32, u32));

fn references(formula: &str, sheet: &str, names: &BTreeMap<String, String>) -> Vec<(At, At)> {
    let mut found = Vec::new();
    let part = |c: char| c.is_ascii_alphanumeric() || "$!:_.'".contains(c);
    for token in formula.split(|c: char| !part(c)).filter(|t| !t.is_empty()) {
        if let Some(text) = names.get(token) {
            found.extend(references(text, sheet, &BTreeMap::new()));
            continue;
        }
        let (on, range) = match token.rsplit_once('!') {
            Some((on, range)) => (on.trim_matches('\'').to_string(), range),
            None => (sheet.to_string(), token),
        };
        let range = range.replace('$', "");
        let (first, last) = range.split_once(':').unwrap_or((&range, &range));
        if let (Some(a), Some(b)) = (book::parse_ref(first), book::parse_ref(last)) {
            found.push(((on.clone(), a), (on, b)));
        }
    }
    found
}

fn defined_names(path: &Path) -> BTreeMap<String, String> {
    let entries = parts::read_entries(path).unwrap();
    let rule = parts::element_rule(Kind::DefinedName).unwrap();
    let xml = entries.get("xl/workbook.xml").unwrap();
    parts::element_instances(xml, &rule)
        .unwrap()
        .into_iter()
        .map(|key| {
            let mut fields = key.splitn(3, '|');
            let name = fields.next().unwrap().to_string();
            (name, fields.nth(1).unwrap().to_string())
        })
        .collect()
}

#[test]
fn every_formula_cell_that_reads_the_changed_cell_is_named() {
    for spec in corpus::specs() {
        let path = corpus_dir().join(spec.file);
        let read = book::read(&path).unwrap();
        let names = defined_names(&path);
        let target: At = (spec.sheet.to_string(), book::parse_ref(spec.cell).unwrap());
        let mut reads = BTreeSet::from([target.clone()]);
        loop {
            let before = reads.len();
            for (sheet, cells) in &read.sheets {
                for (pos, cell) in cells {
                    let Some(formula) = &cell.formula else {
                        continue;
                    };
                    let hit = references(formula, sheet, &names)
                        .iter()
                        .any(|((s, a), (_, b))| {
                            reads.iter().any(|(on, (row, col))| {
                                on == s && (a.0..=b.0).contains(row) && (a.1..=b.1).contains(col)
                            })
                        });
                    if hit {
                        reads.insert((sheet.clone(), *pos));
                    }
                }
            }
            if reads.len() == before {
                break;
            }
        }
        reads.remove(&target);
        let named: BTreeSet<At> = spec
            .dependents
            .iter()
            .map(|d| (d.sheet.to_string(), book::parse_ref(d.cell).unwrap()))
            .collect();
        assert_eq!(reads, named, "{}", spec.file);
    }
}

#[test]
fn every_spec_names_a_target_and_its_old_value() {
    for spec in corpus::specs() {
        let read = book::read(&corpus_dir().join(spec.file)).unwrap();
        let target = book::parse_ref(spec.cell).unwrap();
        let old = read
            .cell(spec.sheet, target)
            .and_then(|c| book::number(&c.value));
        assert_eq!(old, spec.before, "{}", spec.file);
        assert_ne!(Some(spec.new), spec.before, "{}", spec.file);
        for dependent in &spec.dependents {
            let at = book::parse_ref(dependent.cell).unwrap();
            let cell = read.cell(dependent.sheet, at).unwrap();
            assert!(cell.formula.is_some(), "{} {}", spec.file, dependent.cell);
            assert_eq!(
                book::number(&cell.value),
                Some(dependent.before),
                "{}",
                spec.file
            );
            assert_ne!(dependent.before, dependent.after, "{}", spec.file);
        }
    }
}
