use halvo_coord::items::{presenter, request_presenter};
use halvo_coord::types::{
    AgentId, AskedEffect, Class, ConfirmAsk, Digest, EffectId, Presenter, RunId,
};

#[test]
fn presenter_table() {
    let table = [
        (Class::Read, Presenter::Native),
        (Class::Prepare, Presenter::Native),
        (Class::LocalEdit, Presenter::Native),
        (Class::Send, Presenter::Page),
        (Class::Delete, Presenter::Page),
        (Class::Submit, Presenter::Page),
        (Class::Purchase, Presenter::Native),
        (Class::Payment, Presenter::Native),
        (Class::Sign, Presenter::Native),
        (Class::Credential, Presenter::Native),
    ];
    let classes: Vec<Class> = table.iter().map(|(c, _)| *c).collect();
    assert_eq!(
        classes,
        Class::ALL,
        "the ten classes of A-A4.1 in their order"
    );
    for (class, expected) in table {
        assert_eq!(presenter(class), expected, "presenter({class})");
    }
}

fn effect(n: u8, class: Class) -> AskedEffect {
    AskedEffect {
        effect: EffectId::parse(&format!("{:032x}", n)).unwrap(),
        class,
        digest: Digest::parse(&format!("{:064x}", n)).unwrap(),
        digest_ok: true,
        fields: vec![],
        new_origin: None,
    }
}

fn ask(classes: &[Class]) -> ConfirmAsk {
    ConfirmAsk::new(
        RunId::parse(&format!("{:032x}", 4)).unwrap(),
        AgentId::parse(&format!("{:032x}", 2)).unwrap(),
        classes
            .iter()
            .enumerate()
            .map(|(i, c)| effect(i as u8 + 1, *c))
            .collect(),
    )
    .unwrap()
}

#[test]
fn a_request_is_native_when_any_of_its_effects_is() {
    let page = [Class::Send, Class::Delete, Class::Submit];
    assert_eq!(request_presenter(&ask(&page)), Presenter::Page);
    for one in [Class::Purchase, Class::Payment, Class::Sign] {
        assert_eq!(
            request_presenter(&ask(&[one])),
            Presenter::Native,
            "{one} alone"
        );
        for at in 0..=page.len() {
            let mut classes = page.to_vec();
            classes.insert(at, one);
            assert_eq!(
                request_presenter(&ask(&classes)),
                Presenter::Native,
                "{one} at {at} among the page classes"
            );
        }
    }
    for one in page {
        assert_eq!(
            request_presenter(&ask(&[one])),
            Presenter::Page,
            "{one} alone"
        );
    }
}
