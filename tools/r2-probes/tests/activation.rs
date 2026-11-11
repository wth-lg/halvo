use r2_probes::sys::{activation_of, attribute_read, Activation, AttributeRead};

const ATTRIBUTE_UNSUPPORTED: i32 = -25205;
const NO_VALUE: i32 = -25212;
const CANNOT_COMPLETE: i32 = -25204;
const API_DISABLED: i32 = -25211;

#[test]
fn an_attribute_read_tells_false_and_absent_from_a_failed_read() {
    assert_eq!(attribute_read(0, Some(true)), AttributeRead::True);
    assert_eq!(attribute_read(0, Some(false)), AttributeRead::False);
    assert_eq!(attribute_read(0, None), AttributeRead::Absent);
    assert_eq!(
        attribute_read(ATTRIBUTE_UNSUPPORTED, None),
        AttributeRead::Absent
    );
    assert_eq!(attribute_read(NO_VALUE, None), AttributeRead::Absent);
    assert_eq!(
        attribute_read(CANNOT_COMPLETE, None),
        AttributeRead::Failed(CANNOT_COMPLETE)
    );
    assert_eq!(
        attribute_read(API_DISABLED, None),
        AttributeRead::Failed(API_DISABLED)
    );
}

#[test]
fn a_failed_read_gets_a_value_of_its_own() {
    use AttributeRead::{Absent, Failed, False, True};
    assert_eq!(activation_of(&[False, Absent]), Activation::Unset);
    assert_eq!(activation_of(&[Absent, Absent]), Activation::Unset);
    assert_eq!(activation_of(&[True, Absent]), Activation::Set);
    assert_eq!(
        activation_of(&[Failed(CANNOT_COMPLETE), True]),
        Activation::Set
    );
    assert_eq!(
        activation_of(&[False, Failed(CANNOT_COMPLETE)]),
        Activation::ReadFailed
    );
    assert_eq!(activation_of(&[]), Activation::ReadFailed);
}

#[test]
fn each_activation_has_a_word_of_its_own_and_only_unset_states_count_a_read() {
    let all = [
        Activation::Set,
        Activation::Unset,
        Activation::ReadFailed,
        Activation::NoAttribute,
    ];
    let words: Vec<&str> = all.iter().map(Activation::as_str).collect();
    assert_eq!(words, ["set", "unset", "read-failed", "no-attribute"]);
    for word in &words {
        assert!(
            !word.contains("readable"),
            "{word} reads against readable_runs"
        );
    }
    let counting: Vec<Activation> = all
        .into_iter()
        .filter(Activation::allows_a_readable_run)
        .collect();
    assert_eq!(counting, [Activation::Unset, Activation::NoAttribute]);
}
