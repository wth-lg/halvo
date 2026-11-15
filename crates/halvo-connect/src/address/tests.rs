use std::any::{type_name_of_val, TypeId};

use halvo_schema::executable as schema;

#[test]
fn the_names_that_the_crate_gives_on_are_the_items_of_the_schema() {
    let names = [
        (
            type_name_of_val(&crate::same_address),
            type_name_of_val(&schema::same_address),
        ),
        (
            type_name_of_val(&crate::reply_senders),
            type_name_of_val(&schema::reply_senders),
        ),
    ];
    for (given_on, of_the_schema) in names {
        assert!(
            of_the_schema.starts_with("halvo_schema::"),
            "{of_the_schema}"
        );
        assert_eq!(given_on, of_the_schema);
    }
    assert_eq!(
        TypeId::of::<crate::SendersError>(),
        TypeId::of::<schema::SendersError>()
    );
    assert_eq!(
        TypeId::of::<super::Senders>(),
        TypeId::of::<schema::Senders>()
    );
}
