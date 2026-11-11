mod common;

use common::refused;
use halvo_schema::{Category, SeedCategory};

const IDS: [&str; 11] = [
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
    "other",
];

#[test]
fn the_categories_are_the_11_ids_in_their_order() {
    let ids: Vec<&str> = Category::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(ids, IDS);
    for (i, c) in Category::ALL.iter().enumerate() {
        assert_eq!(c.to_string(), IDS[i]);
        assert_eq!(Category::parse(IDS[i]).unwrap(), *c);
        assert_eq!(serde_json::to_string(c).unwrap(), format!("\"{}\"", IDS[i]));
        assert_eq!(
            serde_json::from_str::<Category>(&format!("\"{}\"", IDS[i])).unwrap(),
            *c
        );
    }
    let mut sorted = Category::ALL.to_vec();
    sorted.reverse();
    sorted.sort();
    assert_eq!(sorted, Category::ALL);
}

#[test]
fn the_seed_categories_are_the_first_10_and_refuse_other() {
    let ids: Vec<&str> = SeedCategory::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(ids, IDS[..10]);
    let error = SeedCategory::parse("other").unwrap_err();
    assert_eq!(error.type_name(), "SeedCategory");
    refused::<SeedCategory>("\"other\"");
    for (i, s) in SeedCategory::ALL.iter().enumerate() {
        assert_eq!(Category::from(*s), Category::ALL[i]);
        assert_eq!(Category::ALL[i].seed(), Some(*s));
        assert_eq!(s.to_string(), IDS[i]);
    }
    assert_eq!(Category::parse("other").unwrap().seed(), None);
    let mut sorted = SeedCategory::ALL.to_vec();
    sorted.reverse();
    sorted.sort();
    assert_eq!(sorted, SeedCategory::ALL);
}

#[test]
fn an_id_is_refused_in_any_other_spelling() {
    for bad in [
        "Email",
        "EMAIL",
        " email",
        "email ",
        "e-mail",
        "",
        "utility-bill-payment",
    ] {
        assert!(Category::parse(bad).is_err(), "{bad}");
        assert!(SeedCategory::parse(bad).is_err(), "{bad}");
        refused::<Category>(&serde_json::to_string(bad).unwrap());
    }
    refused::<Category>("1");
    refused::<Category>("null");
    let error = Category::parse("pay my bill").unwrap_err();
    assert_eq!(error.to_string(), "Category: unknown_name");
}

#[test]
fn categories_are_copy_and_hash() {
    let a = SeedCategory::ALL[0];
    let b = a;
    assert_eq!(a, b);
    let set: std::collections::HashSet<Category> = Category::ALL.iter().copied().collect();
    assert_eq!(set.len(), 11);
}
