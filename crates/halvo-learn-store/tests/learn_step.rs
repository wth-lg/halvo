use halvo_learn_store::{Clock, LearnStep};
use halvo_schema::{RowRead, Summary, SummaryRead, WriterStep};

const ROW: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":3,"day":20767,"category":"utility_bill_payment","site":{"key":"northwind-power.test","source":"address_field"},"app":"com.google.Chrome","period":30,"seen":3,"first":20705,"last":20765,"next":20795,"missed":0,"mark":"none","since":null}"#;

struct Today(u32);

impl Clock for Today {
    fn today(&self) -> u32 {
        self.0
    }
}

struct Rows;

impl RowRead for Rows {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        if kind == "learn_routine" {
            visit(ROW);
        }
    }
}

struct NoSummaries;

impl SummaryRead for NoSummaries {
    fn for_each(&self, _: &mut dyn FnMut(&Summary)) {}
}

#[test]
fn a_host_boxes_the_step_by_its_public_path_and_gets_the_row_of_the_thirtieth_day() {
    let steps: Vec<Box<dyn WriterStep>> = vec![
        Box::new(LearnStep::new(Today(20_796))),
        Box::new(LearnStep::new(Today(20_797))),
    ];
    assert!(steps[0].after_commit(&NoSummaries, &Rows).is_empty());
    let records = steps[1].after_commit(&NoSummaries, &Rows);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, "learn_routine");
    assert_eq!(
        records[0].json,
        ROW.replace("\"rev\":3,\"day\":20767", "\"rev\":4,\"day\":20797")
            .replace("\"seen\":3", "\"seen\":0")
    );
}
