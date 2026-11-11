use halvo_route_gate::table::{Fault, Table, TableError, HEADER};

const BILL_1: &str = "bill\tutility_bill_payment\tcompile\ton_device\tm_small\tmac\t1\tdev\t1\tyes\t12\t30\t900\t40\tmedium\ts1";
const BILL_2: &str = "bill\tutility_bill_payment\tcompile\ton_device\tm_small\tmac\t2\tdev\t1\tno\t9\t25\t880\t35\tmedium\ts1";
const BILL_3: &str = "bill\tutility_bill_payment\tcompile\ton_device\tm_small\tmac\t3\theld\t1\tyes\t10\t28\t870\t41\tmedium\ts1";
const GEN_1: &str =
    "gen\temail\twrite\tfrontier\tsynthetic\tgen_a\t1\tdev\t1\tyes\t5\t6\t7\t8\tmedium\tgenerated";
const GEN_2: &str =
    "gen\temail\twrite\tfrontier\tsynthetic\tgen_b\t1\tdev\t1\tno\t5\t6\t7\t8\tmedium\tgenerated";

fn text(rows: &[&str]) -> String {
    let mut text = format!("{HEADER}\n");
    for row in rows {
        text.push_str(row);
        text.push('\n');
    }
    text
}

fn cell(row: &str, at: usize, value: &str) -> String {
    let mut cells: Vec<&str> = row.split('\t').collect();
    cells[at] = value;
    cells.join("\t")
}

fn refused(text: &str) -> TableError {
    match Table::parse(text) {
        Ok(table) => panic!("the text parsed to {} rows: {text:?}", table.rows().len()),
        Err(error) => error,
    }
}

#[test]
fn table_refusals() {
    let fault = |fault, line| TableError { fault, line };
    let cases: Vec<(&str, String, TableError)> = vec![
        ("empty text", String::new(), fault(Fault::Text, 1)),
        (
            "no last LF",
            text(&[BILL_1, BILL_2]).trim_end().to_owned(),
            fault(Fault::Text, 3),
        ),
        (
            "carriage return",
            text(&[BILL_1, BILL_2]).replacen("s1\n", "s1\r\n", 1),
            fault(Fault::Text, 2),
        ),
        (
            "blank line",
            text(&[BILL_1, "", BILL_2]),
            fault(Fault::Text, 3),
        ),
        (
            "another header",
            text(&[BILL_1]).replacen("tokens_out", "tokens", 1),
            fault(Fault::Header, 1),
        ),
        (
            "byte-order mark",
            format!("\u{feff}{}", text(&[BILL_1])),
            fault(Fault::Header, 1),
        ),
        (
            "fifteen cells",
            text(&[BILL_1, BILL_2.rsplit_once('\t').expect("cells").0]),
            fault(Fault::Cell, 3),
        ),
        (
            "unknown task",
            text(&[&cell(BILL_1, 0, "pay")]),
            fault(Fault::Cell, 2),
        ),
        (
            "unknown category",
            text(&[&cell(BILL_1, 1, "bills")]),
            fault(Fault::Cell, 2),
        ),
        (
            "unknown kind",
            text(&[&cell(BILL_1, 2, "drive")]),
            fault(Fault::Cell, 2),
        ),
        (
            "unknown tier",
            text(&[&cell(BILL_1, 3, "hosted")]),
            fault(Fault::Cell, 2),
        ),
        (
            "model of 33 bytes",
            text(&[&cell(BILL_1, 4, &"m".repeat(33))]),
            fault(Fault::Cell, 2),
        ),
        (
            "model with a capital",
            text(&[&cell(BILL_1, 4, "M_small")]),
            fault(Fault::Cell, 2),
        ),
        (
            "model with a hyphen",
            text(&[&cell(BILL_1, 4, "m-small")]),
            fault(Fault::Cell, 2),
        ),
        (
            "empty model",
            text(&[&cell(BILL_1, 4, "")]),
            fault(Fault::Cell, 2),
        ),
        (
            "unknown machine",
            text(&[&cell(BILL_1, 5, "hosted-mac")]),
            fault(Fault::Cell, 2),
        ),
        (
            "seed 4",
            text(&[&cell(BILL_1, 6, "4")]),
            fault(Fault::Cell, 2),
        ),
        (
            "seed 0",
            text(&[&cell(BILL_1, 6, "0")]),
            fault(Fault::Cell, 2),
        ),
        (
            "unknown split",
            text(&[&cell(BILL_1, 7, "test")]),
            fault(Fault::Cell, 2),
        ),
        (
            "attempt 0",
            text(&[&cell(BILL_1, 8, "0")]),
            fault(Fault::Cell, 2),
        ),
        (
            "attempt with a leading zero",
            text(&[&cell(BILL_1, 8, "01")]),
            fault(Fault::Cell, 2),
        ),
        (
            "pass of another word",
            text(&[&cell(BILL_1, 9, "true")]),
            fault(Fault::Cell, 2),
        ),
        (
            "negative steps",
            text(&[&cell(BILL_1, 10, "-1")]),
            fault(Fault::Cell, 2),
        ),
        (
            "wall_s with a point",
            text(&[&cell(BILL_1, 11, "2.5")]),
            fault(Fault::Cell, 2),
        ),
        (
            "tokens beyond u64",
            text(&[&cell(BILL_1, 12, "18446744073709551616")]),
            fault(Fault::Cell, 2),
        ),
        (
            "empty tokens_out",
            text(&[&cell(BILL_1, 13, "")]),
            fault(Fault::Cell, 2),
        ),
        (
            "unknown level",
            text(&[&cell(BILL_1, 14, "mid")]),
            fault(Fault::Cell, 2),
        ),
        (
            "unknown source",
            text(&[&cell(BILL_1, 15, "s3")]),
            fault(Fault::Cell, 2),
        ),
        (
            "recorded row on a generated machine",
            text(&[&cell(BILL_1, 5, "gen_a")]),
            fault(Fault::Cell, 2),
        ),
        (
            "recorded row with the source generated",
            text(&[&cell(BILL_1, 15, "generated")]),
            fault(Fault::Cell, 2),
        ),
        (
            "generated row on a recorded machine",
            text(&[&cell(GEN_1, 5, "mac")]),
            fault(Fault::Cell, 2),
        ),
        (
            "generated row with a recorded source",
            text(&[&cell(GEN_1, 15, "s1")]),
            fault(Fault::Cell, 2),
        ),
        (
            "split dev on seed 3",
            text(&[BILL_1, &cell(BILL_3, 7, "dev")]),
            fault(Fault::Split, 3),
        ),
        (
            "split held on seed 1",
            text(&[&cell(BILL_1, 7, "held")]),
            fault(Fault::Split, 2),
        ),
        (
            "generated after recorded",
            text(&[BILL_1, GEN_1]),
            fault(Fault::Mixed, 3),
        ),
        (
            "recorded after generated",
            text(&[
                GEN_1,
                &cell(GEN_1, 0, "reply")
                    .replace("gen_a", "mac")
                    .replace("generated", "s1"),
            ]),
            fault(Fault::Mixed, 3),
        ),
        (
            "rows out of order",
            text(&[BILL_2, BILL_1]),
            fault(Fault::Order, 3),
        ),
        (
            "one row twice",
            text(&[BILL_1, BILL_1]),
            fault(Fault::Key, 3),
        ),
        (
            "two rows of one key",
            text(&[&cell(BILL_1, 9, "no"), BILL_1]),
            fault(Fault::Key, 3),
        ),
        (
            "two levels in a model cell",
            text(&[BILL_1, &cell(BILL_2, 14, "high")]),
            fault(Fault::Level, 3),
        ),
    ];
    for (name, text, want) in cases {
        assert_eq!(refused(&text), want, "case {name}");
    }
}

#[test]
fn table_reads_what_it_writes() {
    for rows in [vec![], vec![BILL_1, BILL_2, BILL_3], vec![GEN_1, GEN_2]] {
        let text = text(&rows);
        let table = Table::parse(&text).expect("parses");
        assert_eq!(table.to_text(), text);
        assert_eq!(table.rows().len(), rows.len());
        assert_eq!(Table::from_rows(table.rows()).as_ref(), Ok(&table));
    }
}

#[test]
fn generated_and_recorded() {
    assert!(!Table::parse(&text(&[])).expect("parses").is_generated());
    assert!(!Table::parse(&text(&[BILL_1]))
        .expect("parses")
        .is_generated());
    assert!(Table::parse(&text(&[GEN_1]))
        .expect("parses")
        .is_generated());
}

#[test]
fn levels_may_differ_outside_model_cells_and_between_models() {
    let replay_1 = cell(&cell(BILL_1, 2, "replay"), 14, "low");
    let replay_2 = cell(&cell(BILL_2, 2, "replay"), 14, "high");
    let other_model = cell(&cell(BILL_2, 4, "m_tiny"), 14, "high");
    assert!(Table::parse(&text(&[BILL_1, BILL_2, &other_model, &replay_1, &replay_2])).is_ok());
    let other_1 = cell(&cell(GEN_1, 1, "other"), 14, "low");
    let other_2 = cell(&cell(GEN_2, 1, "other"), 14, "high");
    assert!(Table::parse(&text(&[GEN_1, &other_1, &other_2])).is_ok());
    let error = Table::parse(&text(&[BILL_1, "x"])).expect_err("refused");
    assert_eq!(error.detail(), "cell:3");
}
