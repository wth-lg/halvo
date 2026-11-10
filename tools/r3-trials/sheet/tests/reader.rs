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
        vec!["B2:B11|whole||0|".to_string()]
    );
    let styles = br##"<styleSheet><numFmts count="1"><numFmt numFmtId="164" formatCode="#,##0.00"/></numFmts></styleSheet>"##;
    assert_eq!(
        parts::element_instances(styles, &rule(Kind::Format)).unwrap(),
        vec!["#,##0.00".to_string()]
    );
    let workbook = br#"<x:workbook xmlns:x="y"><x:definedNames><x:definedName name="Rate">Input!$B$6</x:definedName></x:definedNames></x:workbook>"#;
    assert_eq!(
        parts::element_instances(workbook, &rule(Kind::DefinedName)).unwrap(),
        vec!["Rate||Input!$B$6".to_string()]
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
        (Kind::Chart, &["entry"]),
        (Kind::Drawing, &["entry", "entry"]),
        (Kind::Merged, &["A1:D1", "A10:B10"]),
        (Kind::Format, &["#,##0.00", "0.0%"]),
        (Kind::Table, &["entry"]),
    ]);
    let saved = inventory(&[
        (Kind::Drawing, &["entry"]),
        (Kind::Merged, &["A1:D1"]),
        (Kind::Format, &["0.0%", "yyyy-mm-dd"]),
        (Kind::Table, &["entry"]),
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

#[test]
fn a_validation_is_keyed_by_its_bounds_and_a_name_by_its_target() {
    let with = br#"<worksheet><dataValidations count="1"><dataValidation type="whole" operator="between" sqref="B2:B11"><formula1>0</formula1><formula2>24</formula2></dataValidation></dataValidations></worksheet>"#;
    let without = br#"<worksheet><dataValidations count="1"><dataValidation type="whole" operator="between" sqref="B2:B11"/></dataValidations></worksheet>"#;
    let keys = |xml: &[u8]| parts::element_instances(xml, &rule(Kind::Validation)).unwrap();
    assert_eq!(keys(with), vec!["B2:B11|whole|between|0|24".to_string()]);
    assert_eq!(keys(without), vec!["B2:B11|whole|between||".to_string()]);
    let broken = br#"<workbook><definedNames><definedName name="Rate">#REF!</definedName></definedNames></workbook>"#;
    assert_eq!(
        parts::element_instances(broken, &rule(Kind::DefinedName)).unwrap(),
        vec!["Rate||#REF!".to_string()]
    );
}

fn rewrite_entry(src: &Path, dst: &Path, entry: &str, from: &str, to: &str) {
    use std::io::{Read, Write};
    let mut archive = zip::ZipArchive::new(std::fs::File::open(src).unwrap()).unwrap();
    let mut out = zip::ZipWriter::new(std::fs::File::create(dst).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let name = file.name().unwrap().to_string();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if name == entry {
            let text = String::from_utf8(bytes).unwrap();
            assert!(text.contains(from), "{entry} lacks {from}");
            bytes = text.replace(from, to).into_bytes();
        }
        out.start_file(name.as_str(), options).unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap();
}

#[test]
fn the_reader_keys_decoded_values() {
    let dir = scratch("decoded");
    let sheet = "xl/worksheets/sheet1.xml";
    let list = "<formula1>\"yes,no\"</formula1>";
    let upper = "<formula2>24</formula2>";
    let name = "Input!$B$6</definedName>";
    let cases = [
        (
            "list-escaped",
            "validation.xlsx",
            sheet,
            list,
            "<formula1>&quot;yes,no&quot;</formula1>",
            vec![],
        ),
        (
            "list-cdata",
            "validation.xlsx",
            sheet,
            list,
            "<formula1><![CDATA[\"yes,no\"]]></formula1>",
            vec![],
        ),
        (
            "list-changed",
            "validation.xlsx",
            sheet,
            list,
            "<formula1>\"yes,no&amp;\"</formula1>",
            vec![Kind::Validation],
        ),
        (
            "upper-charref-changed",
            "validation.xlsx",
            sheet,
            upper,
            "<formula2>24&#48;</formula2>",
            vec![Kind::Validation],
        ),
        (
            "upper-cdata-changed",
            "validation.xlsx",
            sheet,
            upper,
            "<formula2><![CDATA[240]]></formula2>",
            vec![Kind::Validation],
        ),
        (
            "upper-charref-same",
            "validation.xlsx",
            sheet,
            upper,
            "<formula2>&#50;&#x34;</formula2>",
            vec![],
        ),
        (
            "upper-cdata-same",
            "validation.xlsx",
            sheet,
            upper,
            "<formula2><![CDATA[24]]></formula2>",
            vec![],
        ),
        (
            "sqref-charref-same",
            "validation.xlsx",
            sheet,
            "sqref=\"B2:B11\"",
            "sqref=\"B2&#58;B11\"",
            vec![],
        ),
        (
            "name-charref-same",
            "two-sheets.xlsx",
            "xl/workbook.xml",
            name,
            "Input!&#36;B&#36;6</definedName>",
            vec![],
        ),
        (
            "name-changed",
            "two-sheets.xlsx",
            "xl/workbook.xml",
            name,
            "Input!$B$6&amp;</definedName>",
            vec![Kind::DefinedName],
        ),
        (
            "format-charref-same",
            "formats.xlsx",
            "xl/styles.xml",
            "formatCode=\"0.0%\"",
            "formatCode=\"0.0&#37;\"",
            vec![],
        ),
        (
            "rels-target-case",
            "chart.xlsx",
            "xl/drawings/_rels/drawing1.xml.rels",
            "Target=\"../charts/chart1.xml\"",
            "Target=\"../Charts/chart1.xml\"",
            vec![],
        ),
    ];
    let mut wrong = Vec::new();
    for (label, file, entry, from, to, want) in cases {
        let original = corpus_dir().join(file);
        let saved = dir.join(format!("{label}.xlsx"));
        rewrite_entry(&original, &saved, entry, from, to);
        let spec = corpus::spec(file).unwrap();
        let judgement = trial::judge(&original, &saved, &spec).unwrap();
        if judgement.lost != want {
            wrong.push(format!(
                "{label}: lost={} want {}",
                parts::list(&judgement.lost),
                parts::list(&want)
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("; "));
}

#[test]
fn an_entry_that_nothing_links_to_is_lost() {
    let dir = scratch("unlinked");
    let original = corpus_dir().join("chart.xlsx");
    let spec = corpus::spec("chart.xlsx").unwrap();
    let cases = [
        (
            "<drawing r:id=\"rId1\"/>",
            "drawing",
            vec![Kind::Chart, Kind::Drawing],
        ),
        (
            "<legacyDrawing r:id=\"rId2\"/>",
            "legacy",
            vec![Kind::Drawing],
        ),
        ("<tablePart r:id=\"rId3\"/>", "table", vec![Kind::Table]),
    ];
    for (element, name, want) in cases {
        let saved = dir.join(format!("{name}.xlsx"));
        let text = String::from_utf8(
            std::process::Command::new("unzip")
                .args(["-p"])
                .arg(&original)
                .arg("xl/worksheets/sheet1.xml")
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert!(text.contains(element), "{element}");
        rewrite_entry(&original, &saved, "xl/worksheets/sheet1.xml", element, "");
        let judgement = trial::judge(&original, &saved, &spec).unwrap();
        for kind in want {
            assert!(
                judgement.lost.contains(&kind),
                "{name}: {:?}",
                judgement.lost
            );
        }
    }
}

#[test]
fn an_array_formula_that_loses_its_range_differs() {
    let dir = scratch("array");
    let original = dir.join("array.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet().set_name("Values").unwrap();
    sheet.write_number(1, 1, 3.0).unwrap();
    sheet.write_number(2, 1, 4.0).unwrap();
    sheet.write_array_formula(1, 4, 2, 4, "=B2:B3*2").unwrap();
    workbook.save(&original).unwrap();
    let saved = dir.join("plain.xlsx");
    rewrite_entry(
        &original,
        &saved,
        "xl/worksheets/sheet1.xml",
        " t=\"array\" ref=\"E2:E3\"",
        "",
    );
    let spec = corpus::spec("values.xlsx").unwrap();
    let judgement = trial::judge(&original, &saved, &spec).unwrap();
    assert!(!judgement.other_cells_same);
    let same = trial::judge(&original, &original, &spec).unwrap();
    assert!(same.other_cells_same);
}

#[test]
fn a_panic_in_a_library_is_a_miss_and_the_other_cells_run() {
    let dir = scratch("panic");
    let corpus = dir.join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    for file in corpus::FILES {
        if file != "values.xlsx" {
            std::fs::copy(corpus_dir().join(file), corpus.join(file)).unwrap();
        }
    }
    rewrite_entry(
        &corpus_dir().join("values.xlsx"),
        &corpus.join("values.xlsx"),
        "xl/worksheets/sheet1.xml",
        "<c r=\"B5\"",
        "<c r=\"5B\"",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_r3-sheet-trial"))
        .args(["run", "--candidate", "umya", "--corpus"])
        .arg(&corpus)
        .arg("--out")
        .arg(dir.join("out"))
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        stderr, "miss candidate=umya file=values.xlsx cause=panic\n",
        "{stderr}"
    );
    let ran: Vec<&str> = stdout
        .lines()
        .map(|line| line.split_whitespace().nth(2).unwrap_or(""))
        .collect();
    let others: Vec<String> = corpus::FILES
        .iter()
        .filter(|file| **file != "values.xlsx")
        .map(|file| format!("file={file}"))
        .collect();
    assert_eq!(ran, others, "{stdout}");
}

#[test]
fn the_binary_refuses_a_missing_flag() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_r3-sheet-trial"))
        .arg("run")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1);
    assert!(stderr.starts_with("REFUSED usage: "), "{stderr}");
}
