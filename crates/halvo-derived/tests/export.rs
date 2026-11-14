mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    domain, episode, open_host, position, store_id, summary_id, Fixed, Scratch, Seq, TestClock,
};
use halvo_derived::test_host::{export_proposals, ExportError, Stream};
use halvo_derived::{Host, Store};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::RequestId;
use halvo_schema_core::record::{Confidence, Proposal, Recurrence, Trigger, Workflow};
use halvo_schema_core::request::{InterfaceDecision, InterfaceRead, InterfaceRole, ReplyResult};
use halvo_schema_core::site::{AppId, DomainSource, SiteDomain, SiteValue};

const DAY: u32 = 20_767;

fn proposal(category: SeedCategory, site: &str, ids: &[u64]) -> Proposal {
    Proposal {
        category,
        site: Some(SiteValue::Domain {
            key: SiteDomain::parse(site).unwrap(),
            source: DomainSource::AddressField,
        }),
        app: AppId::parse("com.example.browser").unwrap(),
        trigger: Trigger::Schedule,
        recurrence: Recurrence {
            count: ids.len() as u32,
            period: None,
            first: DAY,
            last: DAY,
        },
        confidence: Confidence(90),
        summaries: ids.iter().map(|n| summary_id(*n)).collect(),
    }
}

fn stream_store(scratch: &Scratch, proposals: Vec<Proposal>, episodes: &[u64]) -> Store {
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(proposals));
    let host = Host::new(Box::new(clock), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(scratch, host).unwrap();
    let episodes: Vec<_> = episodes
        .iter()
        .map(|n| {
            let mut e = episode(*n, DAY, domain(&format!("s{n}.test")));
            if let Some(p) = fixed
                .borrow()
                .iter()
                .find(|p| p.summaries.contains(&summary_id(*n)))
            {
                e.category = p.category;
                if let Some(SiteValue::Domain { key, .. }) = &p.site {
                    e.site = domain(key.as_str());
                }
            }
            e
        })
        .collect();
    store
        .commit_batch(&store_id(1), &episodes, position(1, 0), &mut Seq(1))
        .unwrap();
    store
}

fn manifest() -> String {
    "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae".to_owned()
}

fn build() -> String {
    "c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4".to_owned()
}

#[test]
fn export_proposals_writes_the_header_and_one_line_per_proposed_workflow_in_order() {
    let a = Scratch::new("export-a");
    let b = Scratch::new("export-b");
    let mut first = stream_store(
        &a,
        vec![
            proposal(SeedCategory::ShopOrder, "s1.test", &[1]),
            proposal(SeedCategory::UtilityBillPayment, "s2.test", &[3, 2]),
            proposal(SeedCategory::UtilityBillPayment, "s4.test", &[4]),
            proposal(SeedCategory::Email, "s5.test", &[5]),
        ],
        &[1, 2, 3, 4, 5],
    );
    let second = stream_store(
        &b,
        vec![proposal(SeedCategory::TenantRent, "s9.test", &[9])],
        &[9],
    );
    let role = InterfaceRole::claim();
    let page = first
        .interface_read(&InterfaceRead::workflows(
            &role,
            RequestId::from_random([1; 16]),
            None,
        ))
        .unwrap();
    let ReplyResult::Workflows(page) = page else {
        panic!("no workflows page");
    };
    let approve = page
        .workflows
        .iter()
        .find(|i| matches!(&i.workflow, Workflow::Full(f) if f.category == SeedCategory::Email))
        .unwrap()
        .workflow
        .id()
        .clone();
    first
        .decide(&InterfaceDecision::approve(
            &role,
            RequestId::from_random([2; 16]),
            approve,
        ))
        .unwrap();
    let out = a.path().join("proposals.jsonl");
    let streams = [
        Stream {
            id: "gen-dev-s02",
            handle: second.read_handle(&role),
        },
        Stream {
            id: "gen-dev-s01",
            handle: first.read_handle(&role),
        },
    ];
    let written = export_proposals(&out, &manifest(), &build(), &streams).unwrap();
    assert_eq!(written, 4);
    let id = |n: u64| summary_id(n).as_str().to_owned();
    let expected = format!(
        "{{\"manifest\":\"{}\",\"build\":\"{}\"}}\n\
         {{\"stream\":\"gen-dev-s01\",\"category\":\"utility_bill_payment\",\"summaries\":[\"{}\",\"{}\"]}}\n\
         {{\"stream\":\"gen-dev-s01\",\"category\":\"utility_bill_payment\",\"summaries\":[\"{}\"]}}\n\
         {{\"stream\":\"gen-dev-s01\",\"category\":\"shop_order\",\"summaries\":[\"{}\"]}}\n\
         {{\"stream\":\"gen-dev-s02\",\"category\":\"tenant_rent\",\"summaries\":[\"{}\"]}}\n",
        manifest(),
        build(),
        id(2),
        id(3),
        id(4),
        id(1),
        id(9)
    );
    assert_eq!(std::fs::read_to_string(&out).unwrap(), expected);
}

#[test]
fn export_proposals_writes_nothing_when_the_file_exists_or_an_argument_breaks_its_form() {
    let a = Scratch::new("export-exists");
    let store = stream_store(
        &a,
        vec![proposal(SeedCategory::ShopOrder, "s1.test", &[1])],
        &[1],
    );
    let role = InterfaceRole::claim();
    let out = a.path().join("proposals.jsonl");
    std::fs::write(&out, "kept\n").unwrap();
    let streams = [Stream {
        id: "gen-dev-s01",
        handle: store.read_handle(&role),
    }];
    assert!(matches!(
        export_proposals(&out, &manifest(), &build(), &streams),
        Err(ExportError::Exists)
    ));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "kept\n");
    let fresh = a.path().join("fresh.jsonl");
    for (m, b, stream) in [
        ("2C26", build(), "gen-dev-s01"),
        (manifest().as_str(), "c3d2".to_owned(), "gen-dev-s01"),
        (manifest().as_str(), build(), "Gen dev"),
        (manifest().as_str(), build(), ""),
    ] {
        let streams = [Stream {
            id: stream,
            handle: store.read_handle(&role),
        }];
        assert!(matches!(
            export_proposals(&fresh, m, &b, &streams),
            Err(ExportError::Usage(_))
        ));
        assert!(!fresh.exists());
    }
}

/// DS-4, H5 (m): `export_proposals` reads through the interface read alone, so it writes no id of a support row
/// whose summary is past its bound, though no expiry has removed it.
#[test]
fn export_proposals_writes_no_id_of_a_support_row_past_its_bound() {
    let a = Scratch::new("export-past-bound");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(vec![proposal(
        SeedCategory::ShopOrder,
        "s1.test",
        &[1, 2],
    )]));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&a, host).unwrap();
    let episodes: Vec<_> = [(1, DAY - 92), (2, DAY)]
        .iter()
        .map(|(n, day)| {
            let mut e = episode(*n, *day, domain("s1.test"));
            e.category = SeedCategory::ShopOrder;
            e
        })
        .collect();
    store
        .commit_batch(&store_id(1), &episodes, position(1, 0), &mut Seq(1))
        .unwrap();
    clock.set_day(DAY + 1);
    let role = InterfaceRole::claim();
    let out = a.path().join("proposals.jsonl");
    let streams = [Stream {
        id: "gen-dev-s01",
        handle: store.read_handle(&role),
    }];
    assert_eq!(
        export_proposals(&out, &manifest(), &build(), &streams).unwrap(),
        1
    );
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(
        !text.contains(summary_id(1).as_str()),
        "summary 1 is past its bound"
    );
    assert!(text.contains(summary_id(2).as_str()));
}
