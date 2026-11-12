mod common;

use common::Guarded;
use halvo_agent_gate::configs::{config_line, configs, ConfigResult, Configs, StateRead};
use halvo_agent_gate::GateHost;
use halvo_schema::{AgentId, ErrorName, SeedCategory, Workflow, WorkflowId};
use serde_json::json;

const CATEGORIES: [&str; 10] = [
    "utility_bill_payment",
    "tenant_rent",
    "lease_renewal_or_change",
    "tenant_application_review",
    "email",
    "spreadsheet_fill",
    "shop_order",
    "calendar_cleanup",
    "ride_order",
    "daily_chart_check",
];

fn expected_output() -> String {
    let mut out = String::new();
    for category in CATEGORIES {
        out.push_str(&format!(
            "config category={category} stored=yes state=approved agent=set result=valid\n"
        ));
    }
    out.push_str("configs seeds=10 valid=10 store=memory\n");
    out
}

fn record(id: &WorkflowId, category: &str, state: &str, agent: Option<&AgentId>) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": id.as_str(),
        "state": state,
        "category": category,
        "site": null,
        "app": "com.example.browser",
        "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87,
        "agent": agent.map(AgentId::as_str),
    }))
    .expect("a workflow record")
}

#[test]
fn configs_prints_ten_valid_lines_in_the_order_of_the_categories() {
    let mut host = GateHost::new();
    let report = configs(&mut host);
    assert_eq!(report.render(), expected_output());
    assert_eq!(report.valid(), 10);
    assert!(report.pass());
}

#[test]
fn configs_stores_one_preset_per_category_through_the_handler() {
    let mut host = GateHost::new();
    let report = configs(&mut host);
    let stored = host.configs();
    assert_eq!(stored.len(), 10);
    for (line, category) in report.lines().iter().zip(SeedCategory::ALL) {
        let config = stored
            .iter()
            .find(|c| c.category == *category)
            .expect("the category's stored configuration");
        assert_eq!(config.rev.get(), 1);
        let workflow = host
            .workflow(&config.workflow)
            .expect("the category's workflow");
        let Workflow::Full(w) = workflow else {
            panic!("a full workflow");
        };
        assert_eq!(w.agent.as_ref(), Some(&config.agent));
        assert_eq!(line.category, *category);
    }
    let sent: Vec<&str> = host.sent().iter().map(|s| s.op).collect();
    assert_eq!(sent, vec!["create_agent"; 10]);
}

#[test]
fn a_workflow_of_another_agent_gives_a_line_that_is_not_stored_and_fails() {
    let mut host = GateHost::new();
    let workflow = WorkflowId::from_random([0x31; 16]);
    let other = AgentId::from_random([0x41; 16]);
    host.put_record(record(&workflow, "email", "approved", Some(&other)));
    let line = config_line(&mut host, &workflow, SeedCategory::Email);
    assert!(!line.stored);
    assert!(!line.agent_set);
    assert_eq!(line.state, StateRead::Approved);
    assert_eq!(line.result, ConfigResult::Refused(declared("binding")));
    assert_eq!(
        line.render(),
        "config category=email stored=no state=approved agent=none result=binding"
    );
    assert!(!line.pass());
}

#[test]
fn a_proposed_workflow_gives_its_state_and_the_refusal() {
    let mut host = GateHost::new();
    let workflow = WorkflowId::from_random([0x32; 16]);
    host.put_record(record(&workflow, "shop_order", "proposed", None));
    let line = config_line(&mut host, &workflow, SeedCategory::ShopOrder);
    assert_eq!(
        line.render(),
        "config category=shop_order stored=no state=proposed agent=none result=state"
    );
    assert!(!line.pass());
}

#[test]
fn a_workflow_that_is_not_stored_gives_no_workflow() {
    let mut host = GateHost::new();
    let workflow = WorkflowId::from_random([0x33; 16]);
    let line = config_line(&mut host, &workflow, SeedCategory::RideOrder);
    assert_eq!(
        line.render(),
        "config category=ride_order stored=no state=none agent=none result=no_workflow"
    );
    assert!(!line.pass());
}

#[test]
fn a_report_with_one_failing_line_does_not_pass() {
    let mut host = GateHost::new();
    let report = configs(&mut host);
    assert!(report.pass());
    let workflow = WorkflowId::from_random([0x34; 16]);
    let other = AgentId::from_random([0x44; 16]);
    host.put_record(record(&workflow, "email", "approved", Some(&other)));
    let mut lines = report.lines().to_vec();
    lines[4] = config_line(&mut host, &workflow, SeedCategory::Email);
    let report = Configs::from_lines(lines);
    assert_eq!(report.valid(), 9);
    assert!(!report.pass());
    assert!(report
        .render()
        .ends_with("configs seeds=10 valid=9 store=memory\n"));
}

#[test]
fn the_agents_and_workflows_of_two_categories_differ() {
    let mut host = GateHost::new();
    configs(&mut host);
    let stored = host.configs();
    for (i, a) in stored.iter().enumerate() {
        for b in &stored[i + 1..] {
            assert_ne!(a.agent, b.agent);
            assert_ne!(a.workflow, b.workflow);
        }
    }
}

#[test]
fn the_binary_prints_the_configs_lines_and_exits_0() {
    let out = Guarded::run(&["configs"]);
    assert_eq!(out.code, Some(0));
    assert_eq!(out.stdout, expected_output());
    assert_eq!(out.stderr, "");
}

#[test]
fn the_binary_refuses_other_arguments_with_usage() {
    for args in [
        &[][..],
        &["config"][..],
        &["configs", "extra"][..],
        &["--configs"][..],
        &["task"][..],
    ] {
        let out = Guarded::run(args);
        assert_eq!(out.code, Some(2), "{args:?}");
        assert_eq!(out.stdout, "", "{args:?}");
        assert!(
            out.stderr.starts_with("REFUSED usage: "),
            "{args:?}: {}",
            out.stderr
        );
        assert_eq!(out.stderr.lines().count(), 1, "{args:?}");
    }
}

fn declared(name: &str) -> ErrorName {
    ErrorName::read(name).expect("a declared error name")
}
