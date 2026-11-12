mod common;

use common::{bf, bound};
use halvo_policy::{admit, Facts, Provenance};
use halvo_schema::executable::names::{Cause, FieldName};
use halvo_schema::executable::{ActionClass as C, WebOrigin};

const BILL: &str = "https://bill.example:443";
const PAY: &str = "https://pay.example:443";
const SHOP: &str = "https://shop.example:443";
const FRAME: &str = "https://frame.example:443";
const SENDER: &str = "ann@mail.example";

fn origin(s: &str) -> WebOrigin {
    match WebOrigin::parse(s) {
        Ok(o) => o,
        Err(r) => panic!("{r}"),
    }
}

fn facts() -> Facts {
    Facts {
        origins: vec![origin(BILL), origin(PAY), origin(SHOP)],
        request_origin: Some(origin(PAY)),
        new_origin: None,
        senders: vec![SENDER.to_owned(), "bob@mail.example".to_owned()],
        ids: vec!["evt-1".to_owned(), "evt-2".to_owned()],
    }
}

/// A field of the class with a source, an origin and a value that its row admits under `facts()`, and the
/// sources that its row admits.
fn good(class: C) -> Vec<(String, &'static [&'static str])> {
    const SLOT: &[&str] = &["slot"];
    const EXTRACT: &[&str] = &["extract"];
    const CONST: &[&str] = &["const"];
    match class {
        C::Payment => vec![
            (bf("payee", "slot", None, "Northwind Water"), SLOT),
            (bf("amount", "extract", Some(BILL), "42.17"), EXTRACT),
            (bf("instrument", "slot", None, "card ending 4242"), SLOT),
            (bf("origin", "const", None, PAY), CONST),
        ],
        C::Purchase => vec![
            (bf("seller", "slot", None, "Tea House"), SLOT),
            (bf("items", "extract", Some(SHOP), "2 x tea"), EXTRACT),
            (bf("amount", "extract", Some(SHOP), "9.80"), EXTRACT),
            (bf("instrument", "slot", None, "card ending 4242"), SLOT),
            (bf("origin", "const", None, PAY), CONST),
        ],
        C::Send => vec![
            (bf("to", "extract", None, SENDER), EXTRACT),
            (bf("subject", "extract", None, "Re: the rent"), EXTRACT),
            (bf("body", "model", None, "Thank you."), &["model", "const"]),
        ],
        C::Delete => vec![(bf("items", "extract", None, "evt-1"), EXTRACT)],
        C::Submit => vec![
            (bf("origin", "const", None, PAY), CONST),
            (bf("fields", "slot", None, "the form"), &["const", "slot"]),
        ],
        C::Sign => vec![
            (bf("document", "slot", None, "lease.pdf"), SLOT),
            (bf("party", "slot", None, "A. Tenant"), SLOT),
        ],
        C::LocalEdit => vec![
            (bf("file", "slot", None, "ledger.xlsx"), SLOT),
            (bf("range", "slot", None, "B2:D6"), SLOT),
        ],
        C::Read | C::Prepare | C::Credential => panic!("no bound object for {class:?}"),
    }
}

const CLASSES: [C; 7] = [
    C::Payment,
    C::Purchase,
    C::Send,
    C::Delete,
    C::Submit,
    C::Sign,
    C::LocalEdit,
];

fn fields(class: C) -> Vec<String> {
    good(class).into_iter().map(|(f, _)| f).collect()
}

fn name(class: C, i: usize) -> FieldName {
    class.bound_fields()[i]
}

fn with(class: C, i: usize, replacement: String) -> Vec<String> {
    let mut all = fields(class);
    all[i] = replacement;
    all
}

#[test]
fn admit_takes_every_field_from_where_its_row_admits_it() {
    for class in CLASSES {
        assert_eq!(
            admit(class, &bound(class, &fields(class)), &facts()),
            Ok(()),
            "{class:?}"
        );
    }
}

#[test]
fn admit_refuses_every_source_that_the_row_of_a_field_does_not_admit() {
    let mut refused = 0;
    for class in CLASSES {
        for (i, (field, sources)) in good(class).into_iter().enumerate() {
            for source in ["const", "slot", "extract", "model"] {
                let from = field
                    .split("\"from\":\"")
                    .nth(1)
                    .and_then(|s| s.split('"').next());
                let variant = field.replacen(
                    &format!("\"from\":\"{}\"", from.unwrap_or("")),
                    &format!("\"from\":\"{source}\""),
                    1,
                );
                let got = admit(class, &bound(class, &with(class, i, variant)), &facts());
                if sources.contains(&source) {
                    assert_eq!(got, Ok(()), "{class:?} field {i} from {source}");
                } else {
                    assert_eq!(
                        got,
                        Err(Provenance::Field(name(class, i))),
                        "{class:?} field {i} from {source}"
                    );
                    refused += 1;
                }
            }
        }
    }
    assert!(refused > 40, "{refused}");
}

#[test]
fn a_model_value_is_admitted_for_send_body_alone() {
    for class in CLASSES {
        for (i, (field, _)) in good(class).into_iter().enumerate() {
            if class == C::Send && i == 2 {
                continue;
            }
            let from = field
                .split("\"from\":\"")
                .nth(1)
                .and_then(|s| s.split('"').next());
            let variant = field.replacen(
                &format!("\"from\":\"{}\"", from.unwrap_or("")),
                "\"from\":\"model\"",
                1,
            );
            assert_eq!(
                admit(class, &bound(class, &with(class, i, variant)), &facts()),
                Err(Provenance::Field(name(class, i))),
                "{class:?} field {i}"
            );
        }
    }
}

#[test]
fn an_extract_needs_an_origin_of_the_bindings_where_its_row_says_so() {
    for (class, i) in [(C::Payment, 1), (C::Purchase, 1), (C::Purchase, 2)] {
        let n = name(class, i).as_str();
        for o in [None, Some(FRAME)] {
            let f = bf(n, "extract", o, "9.80");
            assert_eq!(
                admit(class, &bound(class, &with(class, i, f)), &facts()),
                Err(Provenance::Field(name(class, i))),
                "{class:?} {n} from {o:?}"
            );
        }
        let mut f = facts();
        f.new_origin = Some(origin(FRAME));
        let field = bf(n, "extract", Some(FRAME), "9.80");
        assert_eq!(
            admit(class, &bound(class, &with(class, i, field)), &f),
            Err(Provenance::Field(name(class, i))),
            "the new origin is no origin of the bindings: {class:?} {n}"
        );
    }
}

#[test]
fn an_amount_over_32_bytes_is_refused() {
    for (class, i, o) in [(C::Payment, 1, BILL), (C::Purchase, 2, SHOP)] {
        let at = "1".repeat(32);
        let over = "1".repeat(33);
        let ok = bf("amount", "extract", Some(o), &at);
        assert_eq!(
            admit(class, &bound(class, &with(class, i, ok)), &facts()),
            Ok(())
        );
        let f = bf("amount", "extract", Some(o), &over);
        assert_eq!(
            admit(class, &bound(class, &with(class, i, f)), &facts()),
            Err(Provenance::Field(FieldName::Amount))
        );
    }
}

#[test]
fn the_origin_field_is_the_paused_request_origin_of_the_bindings_or_the_new_origin() {
    for (class, i) in [(C::Payment, 3), (C::Purchase, 4), (C::Submit, 0)] {
        let other = bf("origin", "const", None, BILL);
        assert_eq!(
            admit(class, &bound(class, &with(class, i, other)), &facts()),
            Err(Provenance::Field(FieldName::Origin)),
            "an origin of the bindings that is not the request's: {class:?}"
        );
        let mut none = facts();
        none.request_origin = None;
        assert_eq!(
            admit(class, &bound(class, &fields(class)), &none),
            Err(Provenance::Field(FieldName::Origin)),
            "no paused request: {class:?}"
        );
        let mut outside = facts();
        outside.request_origin = Some(origin(FRAME));
        let frame = bf("origin", "const", None, FRAME);
        assert_eq!(
            admit(
                class,
                &bound(class, &with(class, i, frame.clone())),
                &outside
            ),
            Err(Provenance::Field(FieldName::Origin)),
            "a request origin outside the bindings: {class:?}"
        );
        outside.new_origin = Some(origin(FRAME));
        assert_eq!(
            admit(class, &bound(class, &with(class, i, frame)), &outside),
            Ok(()),
            "the new origin of the confirmation: {class:?}"
        );
    }
}

#[test]
fn send_to_is_a_sender_of_the_facts_of_at_most_320_bytes() {
    let stranger = bf("to", "extract", None, "eve@mail.example");
    assert_eq!(
        admit(
            C::Send,
            &bound(C::Send, &with(C::Send, 0, stranger)),
            &facts()
        ),
        Err(Provenance::Field(FieldName::To))
    );
    let at = format!("{}@mail.example", "a".repeat(320 - "@mail.example".len()));
    let over = format!("{}@mail.example", "a".repeat(321 - "@mail.example".len()));
    let mut f = facts();
    f.senders = vec![at.clone(), over.clone()];
    let ok = bf("to", "extract", None, &at);
    assert_eq!(
        admit(C::Send, &bound(C::Send, &with(C::Send, 0, ok)), &f),
        Ok(())
    );
    let long = bf("to", "extract", None, &over);
    assert_eq!(
        admit(C::Send, &bound(C::Send, &with(C::Send, 0, long)), &f),
        Err(Provenance::Field(FieldName::To))
    );
}

#[test]
fn send_body_is_at_most_4096_bytes() {
    for source in ["model", "const"] {
        let at = bf("body", source, None, &"b".repeat(1024));
        let mut fields = with(C::Send, 2, at);
        let four = r#"{"name":"body","from":"SOURCE","origin":null,"value":["P","P","P","P"]}"#
            .replace("SOURCE", source)
            .replace('P', &"b".repeat(1024));
        fields[2] = four;
        assert_eq!(
            admit(C::Send, &bound(C::Send, &fields), &facts()),
            Ok(()),
            "4,096 bytes from {source}"
        );
        let five = r#"{"name":"body","from":"SOURCE","origin":null,"value":["P","P","P","P","b"]}"#
            .replace("SOURCE", source)
            .replace('P', &"b".repeat(1024));
        fields[2] = five;
        assert_eq!(
            admit(C::Send, &bound(C::Send, &fields), &facts()),
            Err(Provenance::Field(FieldName::Body)),
            "4,097 bytes from {source}"
        );
    }
}

#[test]
fn delete_items_is_one_id_of_the_facts() {
    let unknown = bf("items", "extract", None, "evt-9");
    assert_eq!(
        admit(
            C::Delete,
            &bound(C::Delete, &with(C::Delete, 0, unknown)),
            &facts()
        ),
        Err(Provenance::Field(FieldName::Items))
    );
    let two = bf("items", "extract", None, "evt-1,evt-2");
    assert_eq!(
        admit(
            C::Delete,
            &bound(C::Delete, &with(C::Delete, 0, two)),
            &facts()
        ),
        Err(Provenance::Field(FieldName::Items))
    );
    let second = bf("items", "extract", None, "evt-2");
    assert_eq!(
        admit(
            C::Delete,
            &bound(C::Delete, &with(C::Delete, 0, second)),
            &facts()
        ),
        Ok(())
    );
}

#[test]
fn admit_names_the_first_field_that_it_refuses() {
    let mut all = fields(C::Payment);
    all[0] = bf("payee", "extract", Some(BILL), "Mallory");
    all[1] = bf("amount", "model", None, "1.00");
    assert_eq!(
        admit(C::Payment, &bound(C::Payment, &all), &facts()),
        Err(Provenance::Field(FieldName::Payee))
    );
}

#[test]
fn admit_refuses_a_bound_object_of_another_class() {
    let b = bound(C::Payment, &fields(C::Payment));
    for class in [C::Purchase, C::Submit, C::Send, C::Read, C::Credential] {
        assert_eq!(
            admit(class, &b, &facts()),
            Err(Provenance::Class),
            "{class:?}"
        );
    }
    assert_eq!(Provenance::Class.cause(), Cause::Provenance);
    assert_eq!(
        Provenance::Field(FieldName::Amount).cause(),
        Cause::Provenance
    );
}
