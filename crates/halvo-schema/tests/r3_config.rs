use halvo_schema::executable::names::{ActionClass, AllowedClass, Autonomy, ModelTier, Period};
use halvo_schema::executable::{AgentConfig, Refusal, RefusalCause};

const GOOD: &str = r#"{"v":1,"workflow":"30000000000000000000000000000001","agent":"40000000000000000000000000000001","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{"untrusted":"yes","private":"yes","external":"yes"},"limits":{"steps":20,"wall_s":120,"model_tokens":0},"budget":null,"permissions":[],"runtime":"web","playbook":null}"#;

fn with(from: &str, to: &str) -> String {
    assert_eq!(GOOD.matches(from).count(), 1, "{from}");
    GOOD.replacen(from, to, 1)
}

fn refused(text: &str) -> Refusal {
    match AgentConfig::parse(text.as_bytes()) {
        Ok(_) => panic!("taken: {text}"),
        Err(refusal) => refusal,
    }
}

fn rule(text: &str, type_name: &str, rule: &str) {
    let refusal = refused(text);
    assert_eq!(refusal.cause(), RefusalCause::Malformed, "{text}");
    assert_eq!(
        (refusal.type_name(), refusal.rule()),
        (type_name, rule),
        "{text}"
    );
}

#[test]
fn refusal_allow_money() {
    for class in ["payment", "purchase", "sign", "credential"] {
        let text = with(r#""allow":[]"#, &format!(r#""allow":["{class}"]"#));
        let refusal = refused(&text);
        assert_eq!(refusal.cause(), RefusalCause::Malformed);
        assert_eq!(refusal.type_name(), "AllowedClass");
        assert_eq!(refusal.rule(), "unknown_name");
        assert_eq!(refusal.key(), Some("allow"));
        let mixed = with(r#""allow":[]"#, &format!(r#""allow":["send","{class}"]"#));
        assert_eq!(refused(&mixed).rule(), "unknown_name");
    }
    let names: Vec<&str> = AllowedClass::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(names, ["send", "delete", "submit"]);
    for class in ActionClass::ALL {
        let allowed = AllowedClass::parse(class.as_str());
        assert_eq!(allowed.is_ok(), names.contains(&class.as_str()), "{class}");
        if allowed.is_ok() {
            assert!(
                !class.is_money()
                    && *class != ActionClass::Sign
                    && *class != ActionClass::Credential
            );
        }
    }
}

#[test]
fn a_configuration_is_read_and_written_back_to_the_same_bytes() {
    let config = AgentConfig::parse(GOOD.as_bytes()).unwrap();
    assert_eq!(config.to_json().unwrap(), GOOD);
    assert_eq!(serde_json::to_string(&config).unwrap(), GOOD);
    assert_eq!(config.autonomy, Autonomy::DraftAndConfirm);
    assert_eq!(config.tier, ModelTier::OnDevice);
    assert_eq!(config.rev.get(), 1);
    assert!(config.budget.is_none() && config.playbook.is_none());
}

#[test]
fn the_parser_takes_every_shape_that_validate_answers_with_a_cause() {
    for text in [
        with(r#""tier":"on_device""#, r#""tier":"frontier""#),
        with(r#""tier":"on_device""#, r#""tier":"low_cost_hosted""#),
        with(
            r#""budget":null"#,
            r#""budget":{"amount":1,"period":"week"}"#,
        ),
        with(
            r#""budget":null"#,
            r#""budget":{"amount":100000000,"period":"month"}"#,
        ),
        with(r#""allow":[]"#, r#""allow":["send","delete","submit"]"#),
        with(r#""allow":[]"#, r#""allow":["submit","send"]"#),
        with(
            r#""autonomy":"draft_and_confirm""#,
            r#""autonomy":"unattended""#,
        ),
        with(
            r#""autonomy":"draft_and_confirm""#,
            r#""autonomy":"observe""#,
        ),
        with(
            r#""limits":{"steps":20,"wall_s":120,"model_tokens":0}"#,
            r#""limits":{"steps":0,"wall_s":0,"model_tokens":4294967295}"#,
        ),
        with(r#""runtime":"web""#, r#""runtime":"none""#),
        with(r#""permissions":[]"#, r#""permissions":["user_files"]"#),
        with(r#""playbook":null"#, r#""playbook":4294967295"#),
        with(r#""rev":1"#, r#""rev":4294967295"#),
    ] {
        let config = AgentConfig::parse(text.as_bytes()).expect(&text);
        assert_eq!(config.to_json().unwrap(), text);
    }
    let budget = AgentConfig::parse(
        with(
            r#""budget":null"#,
            r#""budget":{"amount":5,"period":"month"}"#,
        )
        .as_bytes(),
    )
    .unwrap()
    .budget
    .unwrap();
    assert_eq!((budget.amount, budget.period), (5, Period::Month));
}

#[test]
fn keys_in_another_order_and_white_space_are_read_and_written_in_the_order_of_the_contract() {
    let reordered = r#"{ "playbook":null, "runtime":"web","permissions":[],"budget":null,"limits":{"model_tokens":0,"wall_s":120,"steps":20},"caps":{"external":"yes","private":"yes","untrusted":"yes"},"allow":[],"autonomy":"draft_and_confirm","reasoning":"medium","tier":"on_device","category":"utility_bill_payment","rev":1,"agent":"40000000000000000000000000000001","workflow":"30000000000000000000000000000001","v":1 }"#;
    assert_eq!(
        AgentConfig::parse(reordered.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        GOOD
    );
}

#[test]
fn the_hand_made_inputs_are_refused_each_with_its_rule() {
    let array = r#"[1,"30000000000000000000000000000001","40000000000000000000000000000001",1,"utility_bill_payment","on_device","medium","draft_and_confirm",[],{"untrusted":"yes","private":"yes","external":"yes"},{"steps":20,"wall_s":120,"model_tokens":0},null,[],"web",null]"#;
    rule(array, "AgentConfig", "not_an_object");
    rule(
        &with(
            r#""caps":{"untrusted":"yes","private":"yes","external":"yes"}"#,
            r#""caps":["yes","yes","yes"]"#,
        ),
        "Caps",
        "not_an_object",
    );
    rule(
        &with(r#""budget":null"#, r#""budget":[5,"month"]"#),
        "Budget",
        "not_an_object",
    );
    rule(
        &with(r#""rev":1,"#, r#""rev":1,"rev":1,"#),
        "AgentConfig",
        "repeated_key",
    );
    rule(
        &with(
            r#""untrusted":"yes","#,
            r#""untrusted":"yes","untrusted":"no","#,
        ),
        "Caps",
        "repeated_key",
    );
    rule(
        &with(r#""v":1,"#, r#""v":1,"amount":null,"#),
        "AgentConfig",
        "unknown_key",
    );
    rule(
        &with(r#""v":1,"#, r#""v":1,"note":null,"#),
        "AgentConfig",
        "unknown_key",
    );
    rule(
        &with(r#""external":"yes""#, r#""external":"yes","internal":null"#),
        "Caps",
        "unknown_key",
    );
    rule(&with(r#""rev":1"#, r#""rev":"1""#), "number", "not_a_count");
    rule(
        &with(r#""steps":20"#, r#""steps":"20""#),
        "count",
        "not_a_count",
    );
    rule(&with(r#""rev":1"#, r#""rev":-0"#), "number", "not_a_count");
    rule(
        &with(r#""steps":20"#, r#""steps":-0"#),
        "count",
        "not_a_count",
    );
    rule(&with(r#""rev":1"#, r#""rev":1e0"#), "number", "not_a_count");
    rule(
        &with(r#""steps":20"#, r#""steps":2e1"#),
        "count",
        "not_a_count",
    );
    rule(
        &with(r#""steps":20"#, r#""steps":20.0"#),
        "count",
        "not_a_count",
    );
    rule(
        &with(r#""steps":20"#, r#""steps":-1"#),
        "count",
        "not_a_count",
    );
    rule(
        &with(r#""v":1,"#, r#""v":1,"op":"load","#),
        "AgentConfig",
        "unknown_key",
    );
    rule(
        &with(r#""v":1,"#, r#""v":1,"period":"week","#),
        "AgentConfig",
        "unknown_key",
    );
    rule(
        &with(r#""steps":20,"#, r#""steps":20,"amount":1,"#),
        "Limits",
        "unknown_key",
    );
    rule(
        &with(
            r#""autonomy":"draft_and_confirm""#,
            r#""autonomy":"Draft_and_confirm""#,
        ),
        "Autonomy",
        "unknown_name",
    );
    rule(
        &with(
            r#""category":"utility_bill_payment""#,
            r#""category":"Utility_bill_payment""#,
        ),
        "SeedCategory",
        "unknown_name",
    );
    rule(
        &with(r#""runtime":"web""#, r#""runtime":"Web""#),
        "Runtime",
        "unknown_name",
    );
}

#[test]
fn every_other_refused_input_names_its_rule_and_its_key() {
    let cases: &[(&str, &str, &str, &str, &str)] = &[
        (r#""v":1"#, r#""v":2"#, "V1", "version", "v"),
        (r#""v":1"#, r#""v":"1""#, "V1", "version", "v"),
        (r#""v":1"#, r#""v":1.0"#, "V1", "version", "v"),
        (r#""v":1"#, r#""v":null"#, "V1", "version", "v"),
        (r#""rev":1"#, r#""rev":0"#, "number", "zero", "rev"),
        (
            r#""rev":1"#,
            r#""rev":4294967296"#,
            "number",
            "range",
            "rev",
        ),
        (
            r#""rev":1"#,
            r#""rev":null"#,
            "number",
            "not_a_count",
            "rev",
        ),
        (
            r#""workflow":"30000000000000000000000000000001""#,
            r#""workflow":"3000000000000000000000000000000A""#,
            "WorkflowId",
            "hex_digit",
            "workflow",
        ),
        (
            r#""agent":"40000000000000000000000000000001""#,
            r#""agent":"4000000000000000000000000000001""#,
            "AgentId",
            "length",
            "agent",
        ),
        (
            r#""agent":"40000000000000000000000000000001""#,
            r#""agent":null"#,
            "AgentId",
            "not_a_string",
            "agent",
        ),
        (
            r#""category":"utility_bill_payment""#,
            r#""category":"other""#,
            "SeedCategory",
            "unknown_name",
            "category",
        ),
        (
            r#""tier":"on_device""#,
            r#""tier":"gpt""#,
            "ModelTier",
            "unknown_name",
            "tier",
        ),
        (
            r#""tier":"on_device""#,
            r#""tier":null"#,
            "ModelTier",
            "not_a_string",
            "tier",
        ),
        (
            r#""reasoning":"medium""#,
            r#""reasoning":"max""#,
            "Reasoning",
            "unknown_name",
            "reasoning",
        ),
        (
            r#""autonomy":"draft_and_confirm""#,
            r#""autonomy":"confirm""#,
            "Autonomy",
            "unknown_name",
            "autonomy",
        ),
        (
            r#""allow":[]"#,
            r#""allow":["send","send"]"#,
            "AgentConfig",
            "repeated_name",
            "allow",
        ),
        (
            r#""allow":[]"#,
            r#""allow":"send""#,
            "list",
            "not_a_list",
            "allow",
        ),
        (
            r#""allow":[]"#,
            r#""allow":null"#,
            "list",
            "not_a_list",
            "allow",
        ),
        (
            r#""allow":[]"#,
            r#""allow":{}"#,
            "list",
            "not_a_list",
            "allow",
        ),
        (
            r#""allow":[]"#,
            r#""allow":["read"]"#,
            "AllowedClass",
            "unknown_name",
            "allow",
        ),
        (
            r#""allow":[]"#,
            r#""allow":["prepare"]"#,
            "AllowedClass",
            "unknown_name",
            "allow",
        ),
        (
            r#""allow":[]"#,
            r#""allow":["local_edit"]"#,
            "AllowedClass",
            "unknown_name",
            "allow",
        ),
        (
            r#""allow":[]"#,
            r#""allow":[1]"#,
            "AllowedClass",
            "not_a_string",
            "allow",
        ),
        (
            r#""allow":[]"#,
            r#""allow":[null]"#,
            "AllowedClass",
            "not_a_string",
            "allow",
        ),
        (
            r#""untrusted":"yes""#,
            r#""untrusted":true"#,
            "YesNo",
            "not_a_string",
            "untrusted",
        ),
        (
            r#""untrusted":"yes""#,
            r#""untrusted":"y""#,
            "YesNo",
            "unknown_name",
            "untrusted",
        ),
        (
            r#","external":"yes""#,
            "",
            "Caps",
            "missing_key",
            "external",
        ),
        (
            r#""steps":20"#,
            r#""steps":4294967296"#,
            "count",
            "range",
            "steps",
        ),
        (
            r#""steps":20"#,
            r#""steps":null"#,
            "count",
            "not_a_count",
            "steps",
        ),
        (
            r#""budget":null"#,
            r#""budget":{"amount":0,"period":"week"}"#,
            "Budget",
            "amount_range",
            "amount",
        ),
        (
            r#""budget":null"#,
            r#""budget":{"amount":100000001,"period":"week"}"#,
            "Budget",
            "amount_range",
            "amount",
        ),
        (
            r#""budget":null"#,
            r#""budget":{"amount":5,"period":"day"}"#,
            "Period",
            "unknown_name",
            "period",
        ),
        (
            r#""budget":null"#,
            r#""budget":{"amount":5}"#,
            "Budget",
            "missing_key",
            "period",
        ),
        (
            r#""budget":null"#,
            r#""budget":{"amount":5,"period":"week","currency":"usd"}"#,
            "Budget",
            "unknown_key",
            "budget",
        ),
        (
            r#""budget":null"#,
            r#""budget":{}"#,
            "Budget",
            "missing_key",
            "amount",
        ),
        (
            r#""permissions":[]"#,
            r#""permissions":["user_files","user_files"]"#,
            "AgentConfig",
            "repeated_name",
            "permissions",
        ),
        (
            r#""permissions":[]"#,
            r#""permissions":["network"]"#,
            "Permission",
            "unknown_name",
            "permissions",
        ),
        (
            r#""runtime":"web""#,
            r#""runtime":"browser""#,
            "Runtime",
            "unknown_name",
            "runtime",
        ),
        (
            r#""playbook":null"#,
            r#""playbook":0"#,
            "number",
            "zero",
            "playbook",
        ),
        (
            r#""playbook":null"#,
            r#""playbook":"1""#,
            "number",
            "not_a_count",
            "playbook",
        ),
        (
            r#","playbook":null"#,
            r#""#,
            "AgentConfig",
            "missing_key",
            "playbook",
        ),
    ];
    for (from, to, type_name, rule, key) in cases {
        let text = with(from, to);
        let refusal = refused(&text);
        assert_eq!(refusal.cause(), RefusalCause::Malformed, "{text}");
        assert_eq!(
            (
                refusal.type_name(),
                refusal.rule(),
                refusal.key().unwrap_or("-")
            ),
            (*type_name, *rule, *key),
            "{text}"
        );
    }
}

#[test]
fn input_that_is_no_single_json_object_is_refused() {
    for text in [
        "",
        " ",
        "null",
        "1",
        "\"config\"",
        "{}",
        &format!("{GOOD}{GOOD}"),
        &format!("{GOOD} x"),
        &GOOD[..GOOD.len() - 1],
    ] {
        let refusal = refused(text);
        assert_eq!(refusal.cause(), RefusalCause::Malformed);
        assert!(
            ["json", "not_an_object", "missing_key"].contains(&refusal.rule()),
            "{text}: {refusal}"
        );
    }
}

#[test]
fn no_refusal_holds_a_value_of_the_input() {
    for (from, to) in [
        (
            r#""autonomy":"draft_and_confirm""#,
            r#""autonomy":"zq_secret_4711""#,
        ),
        (r#""allow":[]"#, r#""allow":["zq_secret_4711"]"#),
        (r#""v":1,"#, r#""v":1,"zq_secret_4711":1,"#),
        (r#""rev":1"#, r#""rev":"zq_secret_4711""#),
        (
            r#""category":"utility_bill_payment""#,
            r#""category":"zq_secret_4711""#,
        ),
    ] {
        let refusal = refused(&with(from, to));
        let shown = format!("{refusal} {refusal:?}");
        assert!(!shown.contains("4711") && !shown.contains("zq_"), "{shown}");
    }
}

#[test]
fn a_configuration_that_breaks_a_rule_is_not_written() {
    let mut config = AgentConfig::parse(GOOD.as_bytes()).unwrap();
    config.allow = vec![AllowedClass::Send, AllowedClass::Send];
    assert_eq!(config.to_json().unwrap_err().rule(), "repeated_name");
    assert!(serde_json::to_string(&config).is_err());
    config.allow = vec![AllowedClass::Send];
    config.budget = Some(halvo_schema::executable::Budget {
        amount: 0,
        period: Period::Week,
    });
    assert_eq!(config.to_json().unwrap_err().rule(), "amount_range");
}
