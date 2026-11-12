use halvo_policy::digest;
use halvo_schema::executable::Bound;

const BILL: &str = r#"{"class":"payment","fields":[{"name":"payee","from":"slot","origin":null,"value":["Northwind Water"]},{"name":"amount","from":"extract","origin":"https://bill.example:443","value":["42.17"]},{"name":"instrument","from":"slot","origin":null,"value":["card ending 4242"]},{"name":"origin","from":"const","origin":null,"value":["https://pay.example:443"]}]}"#;
const BILL_CHANGED: &str = r#"{"class":"payment","fields":[{"name":"payee","from":"slot","origin":null,"value":["Northwind Water"]},{"name":"amount","from":"extract","origin":"https://bill.example:443","value":["42.18"]},{"name":"instrument","from":"slot","origin":null,"value":["card ending 4242"]},{"name":"origin","from":"const","origin":null,"value":["https://pay.example:443"]}]}"#;
const FILL: &str = r#"{"class":"local_edit","fields":[{"name":"file","from":"slot","origin":null,"value":["ledger.xlsx"]},{"name":"range","from":"slot","origin":null,"value":["B2:D6"]}]}"#;

fn bound(json: &str) -> Bound {
    match Bound::parse(json.as_bytes()) {
        Ok(b) => b,
        Err(r) => panic!("{r}"),
    }
}

fn hex(json: &str) -> String {
    match digest(&bound(json)) {
        Ok(d) => d.as_str().to_owned(),
        Err(r) => panic!("{r}"),
    }
}

#[test]
fn the_digest_is_the_lower_case_hex_sha256_of_the_compact_json_of_the_bound_object() {
    assert_eq!(
        hex(BILL),
        "eddee197c1f97b2e3b10214eba9087a6d18c814ac617b7515c5ef67a450fe7b1"
    );
    assert_eq!(
        hex(BILL_CHANGED),
        "1d6097a1a1a3367fbb3317c52b2864ebcf7057a1739d64ab64aa0a822cab2221"
    );
    assert_eq!(
        hex(FILL),
        "8701a2c57f0745500752c22f99daa6c83fabfd4744f9a147e1bdf78ad7670925"
    );
}

#[test]
fn the_digest_covers_the_compact_form_whatever_the_spacing_of_the_input() {
    let spaced = BILL.replace("\":", "\": ").replace(",\"", ", \"");
    assert_ne!(spaced, BILL);
    assert_eq!(hex(&spaced), hex(BILL));
}

#[test]
fn one_changed_value_changes_the_digest() {
    assert_ne!(hex(BILL), hex(BILL_CHANGED));
    assert_eq!(hex(BILL), hex(BILL));
}
