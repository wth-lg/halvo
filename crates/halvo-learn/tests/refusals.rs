use halvo_learn::{
    admit, new_locators, value_hit, AdmitCause, AdmitFacts, Locator, RunMode, Target, EDITS_MAX,
};

fn loc(by: &str, value: &str, name: Option<&str>) -> Locator {
    Locator {
        by: by.to_owned(),
        value: value.to_owned(),
        name: name.map(str::to_owned),
    }
}

fn admitted_facts() -> AdmitFacts {
    AdmitFacts {
        mode: RunMode::Replay,
        has_base: true,
        same_but_target: true,
        kind_web: true,
        class_same: true,
        playbook_ok: true,
        new_count: 1,
        value_hit: false,
        fits: true,
        rejected: false,
        edits_for_step: 0,
    }
}

#[test]
fn refusal_value_in_label() {
    let slot_values = vec!["Acme Utility".to_owned()];
    assert_eq!(slot_values[0].len(), 12);
    let with_value = Target {
        desc: "button \"Pay\"".to_owned(),
        locators: vec![loc("role", "button", Some("Pay Acme Utility now"))],
    };
    let without_value = Target {
        desc: "button \"Pay\"".to_owned(),
        locators: vec![loc("role", "button", Some("Pay now"))],
    };
    assert!(value_hit(&with_value, &slot_values));
    assert!(!value_hit(&without_value, &slot_values));

    let facts = AdmitFacts {
        value_hit: value_hit(&with_value, &slot_values),
        ..admitted_facts()
    };
    assert_eq!(admit(&facts), Err(AdmitCause::Value));
    let control = AdmitFacts {
        value_hit: value_hit(&without_value, &slot_values),
        ..admitted_facts()
    };
    assert_eq!(admit(&control), Ok(()));
}

#[test]
fn refusal_compile_run_edit() {
    assert_eq!(admit(&admitted_facts()), Ok(()));
    for mode in [RunMode::Compile, RunMode::None] {
        let facts = AdmitFacts {
            mode,
            ..admitted_facts()
        };
        assert_eq!(
            admit(&facts),
            Err(AdmitCause::Mode),
            "mode {}",
            mode.token()
        );
    }

    let missing_step = AdmitFacts {
        has_base: false,
        ..admitted_facts()
    };
    assert_eq!(admit(&missing_step), Err(AdmitCause::Base));

    let compiled = [loc("role", "button", Some("Pay")), loc("css", "#pay", None)];
    let learned = loc("label", "Pay bill", None);
    let mut served = vec![learned.clone()];
    served.extend(compiled.iter().cloned());
    let held = vec![learned, compiled[1].clone()];
    let facts = AdmitFacts {
        new_count: new_locators(&held, &served).len(),
        ..admitted_facts()
    };
    assert_eq!(admit(&facts), Err(AdmitCause::Locator));

    let one_new = vec![loc("text", "Pay now", None), compiled[1].clone()];
    let control = AdmitFacts {
        new_count: new_locators(&one_new, &served).len(),
        ..admitted_facts()
    };
    assert_eq!(control.new_count, 1);
    assert_eq!(admit(&control), Ok(()));
}

#[test]
fn refusal_edits_full() {
    assert_eq!(EDITS_MAX, 8);
    let full = AdmitFacts {
        edits_for_step: 8,
        ..admitted_facts()
    };
    assert_eq!(admit(&full), Err(AdmitCause::Full));
    let below = AdmitFacts {
        edits_for_step: 7,
        ..admitted_facts()
    };
    assert_eq!(admit(&below), Ok(()));
}
