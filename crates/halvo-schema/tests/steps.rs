use std::collections::BTreeMap;

use halvo_schema::kinds::{check_record, FieldType, KindEntry, Origin, RetentionClass};
use halvo_schema::{
    Confidence, Proposal, Recurrence, RecurrenceStep, RowRead, SeedCategory, Summary, SummaryId,
    SummaryRead, Trigger, TypedRecord, WriterStep,
};

const SUMMARIES: [&str; 4] = [
    r#"{"v":1,"id":"20000000000000000000000000000001","category":"utility_bill_payment","site":{"key":"shop.test","source":"address_field"},"app":"com.example.browser","day":20705,"part":"evening","minutes":5,"keys":null,"pointer":null}"#,
    r#"{"v":1,"id":"20000000000000000000000000000002","category":"email","site":null,"app":"com.example.mail","day":20706,"part":"morning","minutes":10,"keys":null,"pointer":null}"#,
    r#"{"v":1,"id":"20000000000000000000000000000003","category":"utility_bill_payment","site":{"key":"shop.test","source":"address_field"},"app":"com.example.browser","day":20736,"part":"evening","minutes":5,"keys":null,"pointer":null}"#,
    r#"{"v":1,"id":"20000000000000000000000000000004","category":"utility_bill_payment","site":{"key":"shop.test","source":"address_field"},"app":"com.example.browser","day":20767,"part":"evening","minutes":10,"keys":null,"pointer":null}"#,
];

struct Stored(Vec<Summary>);

impl SummaryRead for Stored {
    fn for_each(&self, visit: &mut dyn FnMut(&Summary)) {
        for summary in &self.0 {
            visit(summary);
        }
    }
}

fn stored() -> Stored {
    Stored(
        SUMMARIES
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect(),
    )
}

struct Threes;

impl RecurrenceStep for Threes {
    fn propose(&self, summaries: &dyn SummaryRead) -> Vec<Proposal> {
        let mut groups: BTreeMap<String, Vec<Summary>> = BTreeMap::new();
        summaries.for_each(&mut |s| {
            let key = format!("{}|{:?}", s.category, s.site);
            groups.entry(key).or_default().push(s.clone());
        });
        groups
            .into_values()
            .filter(|group| group.len() >= 3)
            .map(|group| Proposal {
                category: group[0].category,
                site: group[0].site.clone(),
                app: group[0].app.clone(),
                trigger: Trigger::Schedule,
                recurrence: Recurrence {
                    count: u32::try_from(group.len()).unwrap(),
                    period: Some(31),
                    first: group[0].day,
                    last: group[group.len() - 1].day,
                },
                confidence: Confidence(80),
                summaries: group.iter().map(|s| s.id.clone()).collect(),
            })
            .collect()
    }
}

struct Rows(BTreeMap<&'static str, Vec<String>>);

impl RowRead for Rows {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        if let Some(rows) = self.0.get(kind) {
            for row in rows {
                visit(row);
            }
        }
    }
}

const ROUTINE: KindEntry = KindEntry {
    name: "probe_routine",
    class: RetentionClass::Bounded { days: 30 },
    origin: Origin::Capture,
    family: "probe",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("day", FieldType::Day),
        ("count", FieldType::Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","day":20767,"count":2}"#,
};

struct CountStep;

impl WriterStep for CountStep {
    fn after_commit(&self, summaries: &dyn SummaryRead, rows: &dyn RowRead) -> Vec<TypedRecord> {
        let mut count = 0u32;
        summaries.for_each(&mut |_| count += 1);
        let mut last_day = 0u32;
        rows.for_each(ROUTINE.name, &mut |row| {
            let value: serde_json::Value = serde_json::from_str(row).unwrap();
            last_day = u32::try_from(value["day"].as_u64().unwrap()).unwrap();
        });
        vec![TypedRecord {
            kind: ROUTINE.name,
            json: format!(
                r#"{{"v":1,"workflow":"10000000000000000000000000000001","day":{},"count":{count}}}"#,
                last_day + 1
            ),
        }]
    }
}

#[test]
fn a_recurrence_step_reads_the_summaries_through_the_read_trait_and_gives_the_same_proposals_each_time(
) {
    let step: Box<dyn RecurrenceStep> = Box::new(Threes);
    let first = step.propose(&stored());
    let second = step.propose(&stored());
    assert_eq!(first, second);
    assert_eq!(first.len(), 1);
    let proposal = &first[0];
    assert_eq!(proposal.category, SeedCategory::UtilityBillPayment);
    assert_eq!(
        proposal.recurrence,
        Recurrence {
            count: 3,
            period: Some(31),
            first: 20705,
            last: 20767
        }
    );
    let ids: Vec<SummaryId> = ["1", "3", "4"]
        .iter()
        .map(|n| SummaryId::parse(&format!("2000000000000000000000000000000{n}")).unwrap())
        .collect();
    assert_eq!(proposal.summaries, ids);
    assert!(format!("{proposal:?}").starts_with("Proposal {"));
}

#[test]
fn a_writer_step_reads_its_rows_and_returns_typed_records_that_its_kind_reads() {
    let rows = Rows(BTreeMap::from([(
        "probe_routine",
        vec![ROUTINE.example.to_owned()],
    )]));
    let steps: Vec<Box<dyn WriterStep>> = vec![Box::new(CountStep)];
    let records: Vec<TypedRecord> = steps
        .iter()
        .flat_map(|step| step.after_commit(&stored(), &rows))
        .collect();
    assert_eq!(
        records,
        [TypedRecord {
            kind: "probe_routine",
            json: r#"{"v":1,"workflow":"10000000000000000000000000000001","day":20768,"count":4}"#
                .to_owned()
        }]
    );
    for record in &records {
        assert_eq!(check_record(&ROUTINE, &record.json), Ok(()));
    }
    let mut other = 0;
    rows.for_each("probe_other", &mut |_| other += 1);
    assert_eq!(other, 0);
}
