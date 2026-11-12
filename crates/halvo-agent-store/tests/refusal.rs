mod common;

use common::*;
use halvo_agent_store::{Host, MemoryStore};
use halvo_policy::preset;
use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::{AllowedClass, Autonomy};
use halvo_schema::{AgentId, Category, TypedRecord, WorkflowId};

fn unattended_sender() -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "approved", Some(&agent(1))));
    let mut config = preset(
        Category::Email,
        WorkflowId::parse(&wf(1)).expect("an id"),
        AgentId::parse(&agent(1)).expect("an id"),
    )
    .expect("a preset");
    config.autonomy = Autonomy::Unattended;
    config.allow = vec![AllowedClass::Send];
    config.caps.untrusted = YesNo::No;
    store
        .commit(vec![TypedRecord {
            kind: "agent_config",
            json: config.to_json().expect("the config's JSON"),
        }])
        .expect("the config is stored");
    store
}

#[test]
fn refusal_confirm_as_run() {
    let cases = [
        ("email", "send", "cleared", false),
        ("email", "send", "refused", false),
        ("email", "send", "held", true),
        ("utility_bill_payment", "payment", "cleared", false),
        ("utility_bill_payment", "payment", "held", true),
        ("shop_order", "purchase", "cleared", false),
        ("shop_order", "purchase", "held", true),
        ("calendar_cleanup", "delete", "cleared", false),
        ("calendar_cleanup", "delete", "held", true),
        ("tenant_rent", "payment", "held", false),
        ("tenant_rent", "payment", "cleared", false),
        ("tenant_rent", "payment", "refused", true),
        ("lease_renewal_or_change", "sign", "held", false),
        ("lease_renewal_or_change", "sign", "refused", true),
        ("spreadsheet_fill", "local_edit", "cleared", true),
        ("spreadsheet_fill", "local_edit", "refused", true),
        ("spreadsheet_fill", "local_edit", "held", false),
        ("daily_chart_check", "local_edit", "cleared", false),
        ("daily_chart_check", "submit", "refused", true),
    ];
    for (category, class, state, taken) in cases {
        let mut store = store_with(category);
        let answer = put(&mut store, row(1, 1, 1, state, class, 1, None, None));
        if taken {
            assert_eq!(answer, Ok(stored(1)), "{category} {class} {state}");
            assert_eq!(effects(&store).len(), 1);
        } else {
            assert_eq!(
                answer,
                Err(declared("effect")),
                "{category} {class} {state}"
            );
            assert!(effects(&store).is_empty());
        }
    }

    let mut store = unattended_sender();
    assert_eq!(
        put(&mut store, row(1, 1, 1, "held", "send", 1, None, None)),
        Err(declared("effect"))
    );
    assert_eq!(
        put(&mut store, row(1, 1, 1, "cleared", "send", 1, None, None)),
        Ok(stored(1))
    );
    assert_eq!(
        put(&mut store, row(1, 2, 1, "cleared", "delete", 1, None, None)),
        Err(declared("effect"))
    );
    assert_eq!(
        put(&mut store, row(1, 2, 1, "held", "delete", 1, None, None)),
        Ok(stored(1))
    );
}
