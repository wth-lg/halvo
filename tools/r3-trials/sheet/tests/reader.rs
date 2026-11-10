use std::path::{Path, PathBuf};

use calamine::Data;
use r3_sheet_trial::book::{self, Book, Cell};
use r3_sheet_trial::corpus;
use r3_sheet_trial::mutate::{self, Mutation};
use r3_sheet_trial::parts::{self, Inventory, Kind};
use r3_sheet_trial::trial::{self, Cached};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("reader")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn entry_rules_name_each_archive_kind() {
    let cases = [
        ("xl/charts/chart1.xml", Some(Kind::Chart)),
        ("xl/charts/_rels/chart1.xml.rels", Some(Kind::Chart)),
        ("xl/drawings/drawing1.xml", Some(Kind::Drawing)),
        ("xl/drawings/vmlDrawing1.vml", Some(Kind::Drawing)),
        ("xl/pivotTables/pivotTable1.xml", Some(Kind::Pivot)),
        ("xl/pivotCache/pivotCacheDefinition1.xml", Some(Kind::Pivot)),
        ("xl/vbaProject.bin", Some(Kind::Macro)),
        ("xl/tables/table1.xml", Some(Kind::Table)),
        ("xl/comments1.xml", Some(Kind::Comment)),
        ("xl/Comments2.xml", Some(Kind::Comment)),
        ("xl/charts/", None),
        ("xl/drawings/", None),
        ("xl/commentsx/a.xml", None),
        ("xl/worksheets/sheet1.xml", None),
        ("xl/styles.xml", None),
    ];
    for (name, want) in cases {
        assert_eq!(parts::entry_kind(name), want, "{name}");
    }
}

fn rule(kind: Kind) -> parts::ElementRule {
    parts::element_rule(kind).unwrap()
}

#[test]
fn element_rules_find_children_of_the_parent() {
    let sheet = br#"<worksheet xmlns="x"><sheetData/><mergeCells count="2"><mergeCell ref="A1:D1"/><mergeCell ref="A10:B10"/></mergeCells><dataValidations count="1"><dataValidation type="whole" sqref="B2:B11"><formula1>0</formula1></dataValidation></dataValidations></worksheet>"#;
    assert_eq!(
        parts::element_instances(sheet, &rule(Kind::Merged)).unwrap(),
        vec!["A1:D1".to_string(), "A10:B10".to_string()]
    );
    assert_eq!(
        parts::element_instances(sheet, &rule(Kind::Validation)).unwrap(),
        vec!["B2:B11|whole".to_string()]
    );
    let styles = br##"<styleSheet><numFmts count="1"><numFmt numFmtId="164" formatCode="#,##0.00"/></numFmts></styleSheet>"##;
    assert_eq!(
        parts::element_instances(styles, &rule(Kind::Format)).unwrap(),
        vec!["#,##0.00".to_string()]
    );
    let workbook = br#"<x:workbook xmlns:x="y"><x:definedNames><x:definedName name="Rate">Input!$B$6</x:definedName></x:definedNames></x:workbook>"#;
    assert_eq!(
        parts::element_instances(workbook, &rule(Kind::DefinedName)).unwrap(),
        vec!["Rate|".to_string()]
    );
}

#[test]
fn an_empty_parent_or_a_stray_child_is_no_instance() {
    let sheet = br#"<worksheet><mergeCells count="0"/><dataValidations count="0"></dataValidations><mergeCell ref="Z1:Z2"/></worksheet>"#;
    assert!(parts::element_instances(sheet, &rule(Kind::Merged))
        .unwrap()
        .is_empty());
    assert!(parts::element_instances(sheet, &rule(Kind::Validation))
        .unwrap()
        .is_empty());
}

fn inventory(pairs: &[(Kind, &[&str])]) -> Inventory {
    let mut found = Inventory::default();
    for (kind, keys) in pairs {
        found
            .instances
            .insert(*kind, keys.iter().map(|k| k.to_string()).collect());
    }
    found
}

#[test]
fn a_kind_is_lost_when_the_saved_file_has_less_of_it() {
    let original = inventory(&[
        (Kind::Chart, &["xl/charts/chart1.xml"]),
        (
            Kind::Drawing,
            &["xl/drawings/drawing1.xml", "xl/drawings/vmlDrawing1.vml"],
        ),
        (Kind::Merged, &["A1:D1", "A10:B10"]),
        (Kind::Format, &["#,##0.00", "0.0%"]),
        (Kind::Table, &["xl/tables/table1.xml"]),
    ]);
    let saved = inventory(&[
        (Kind::Drawing, &["xl/drawings/vmlDrawing1.vml"]),
        (Kind::Merged, &["A1:D1"]),
        (Kind::Format, &["0.0%", "yyyy-mm-dd"]),
        (Kind::Table, &["xl/tables/table9.xml"]),
    ]);
    assert_eq!(
        parts::lost(&original, &saved),
        vec![Kind::Chart, Kind::Drawing, Kind::Merged, Kind::Format]
    );
    assert!(parts::lost(&original, &original).is_empty());
    assert_eq!(parts::list(&[Kind::Format, Kind::Chart]), "chart,format");
    assert_eq!(parts::list(&[]), "-");
}

fn cell(value: Data, formula: Option<&str>) -> Cell {
    Cell {
        value,
        formula: formula.map(str::to_string),
    }
}

fn ledger(d2: Data, d7_cached: Data, d7_formula: &str) -> Book {
    let mut cells = std::collections::BTreeMap::new();
    cells.insert((0, 0), cell(Data::String("Id".into()), None));
    cells.insert((1, 0), cell(Data::String("INV-1".into()), None));
    if d2 != Data::Empty {
        cells.insert((1, 3), cell(d2, None));
    }
    cells.insert((6, 3), cell(d7_cached, Some(d7_formula)));
    Book {
        sheets: vec![("Ledger".to_string(), cells)],
    }
}

#[test]
fn sameness_compares_values_and_formula_text_but_not_cached_results() {
    let target = book::parse_ref("D2").unwrap();
    let before = ledger(Data::Empty, Data::Float(0.0), "SUM(D2:D6)");
    let changed = ledger(Data::Float(42.0), Data::Float(0.0), "SUM(D2:D6)");
    assert!(book::same_except(&before, &changed, "Ledger", target));
    let recached = ledger(Data::Float(42.0), Data::Float(42.0), "SUM(D2:D6)");
    assert!(book::same_except(&before, &recached, "Ledger", target));
    let new_text = ledger(Data::Float(42.0), Data::Float(0.0), "SUM(D2:D5)");
    assert!(!book::same_except(&before, &new_text, "Ledger", target));
    let mut other = changed.clone();
    other.sheets[0]
        .1
        .insert((1, 0), cell(Data::String("INV-9".into()), None));
    assert!(!book::same_except(&before, &other, "Ledger", target));
    let mut extra = changed.clone();
    extra.sheets[0]
        .1
        .insert((9, 9), cell(Data::Float(1.0), None));
    assert!(!book::same_except(&before, &extra, "Ledger", target));
    let mut value_for_formula = changed.clone();
    value_for_formula.sheets[0]
        .1
        .insert((6, 3), cell(Data::Float(0.0), None));
    assert!(!book::same_except(
        &before,
        &value_for_formula,
        "Ledger",
        target
    ));
    let renamed = Book {
        sheets: vec![("Other".to_string(), changed.sheets[0].1.clone())],
    };
    assert!(!book::same_except(&before, &renamed, "Ledger", target));
}

#[test]
fn cached_results_name_fresh_stale_absent_and_none() {
    let spec = corpus::spec("formulas.xlsx").unwrap();
    let with = |cached: Data| ledger(Data::Float(42.0), cached, "SUM(D2:D6)");
    assert_eq!(
        trial::cached_results(&with(Data::Float(42.0)), &spec),
        Cached::Fresh
    );
    assert_eq!(
        trial::cached_results(&with(Data::Float(0.0)), &spec),
        Cached::Stale
    );
    assert_eq!(
        trial::cached_results(&with(Data::Float(7.0)), &spec),
        Cached::Stale
    );
    assert_eq!(
        trial::cached_results(&with(Data::Empty), &spec),
        Cached::Absent
    );
    assert_eq!(
        trial::cached_results(&with(Data::String(String::new())), &spec),
        Cached::Absent
    );
    let values = corpus::spec("values.xlsx").unwrap();
    assert_eq!(
        trial::cached_results(&with(Data::Float(42.0)), &values),
        Cached::None
    );
}

#[test]
fn removing_each_part_of_a_corpus_workbook_makes_lost_name_it() {
    let dir = scratch("remove");
    let mut seen = Vec::new();
    for spec in corpus::specs() {
        let original = corpus_dir().join(spec.file);
        for kind in parts::inventory(&original).unwrap().kinds() {
            let mutant = dir.join(format!("{kind}-{}", spec.file));
            mutate::apply(&original, &mutant, &Mutation::RemoveKind(kind)).unwrap();
            let judgement = trial::judge(&original, &mutant, &spec).unwrap();
            assert!(judgement.lost.contains(&kind), "{} {kind}", spec.file);
            seen.push(kind);
        }
    }
    seen.sort();
    seen.dedup();
    assert_eq!(
        parts::list(&seen),
        "chart,drawing,table,comment,validation,merged,format,defined_name"
    );
}

#[test]
fn changing_one_other_cell_makes_other_cells_differ() {
    let dir = scratch("cell");
    for spec in corpus::specs() {
        let original = corpus_dir().join(spec.file);
        let mutant = dir.join(spec.file);
        let other = if spec.cell == "A2" { "A3" } else { "A2" };
        mutate::apply(
            &original,
            &mutant,
            &Mutation::ReplaceCell {
                sheet: spec.sheet.into(),
                cell: other.into(),
            },
        )
        .unwrap();
        let judgement = trial::judge(&original, &mutant, &spec).unwrap();
        assert!(!judgement.other_cells_same, "{}", spec.file);
        assert!(judgement.lost.is_empty(), "{}", spec.file);
    }
}
