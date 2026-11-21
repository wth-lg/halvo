use halvo_schema::executable::names::Cause;
use halvo_schema::executable::EffectState;

#[test]
fn not_made_has_no_stand_in_left() {
    assert!(
        EffectState::parse("not_made").is_ok(),
        "the schema has no state not_made"
    );
    for name in ["not_sent", "not_replaced", "unauthorized", "throttled"] {
        assert!(Cause::parse(name).is_ok(), "the schema has no cause {name}");
    }
    assert!(
        !include_str!("lib.rs").contains("mod not_made;"),
        "the stand-in NotMadeCause is still a module of the crate"
    );
}
