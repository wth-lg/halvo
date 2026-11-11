use std::{fs, num::NonZeroU64, path::PathBuf};

use halvo_wire::{
    request::{build, AccountId, Call, Credential, Refused, Request, APP_ROUTES, HEADER_NAMES},
    types::{Cap, Period},
    CAP_AMOUNT_MAX, CAP_AMOUNT_MIN,
};
use serde_json::Value;

const SEED: u64 = 20_261_111;
const DRAWN: usize = 3000;

fn vendored(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schema")
        .join(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{name} is not JSON: {error}"))
}

struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        for byte in &mut out {
            *byte = self.next() as u8;
        }
        out
    }
}

enum Atom {
    Literal(char),
    Class(Vec<(char, char)>),
}

fn compile(pattern: &str) -> Vec<(Atom, usize)> {
    let body = pattern
        .strip_prefix('^')
        .and_then(|rest| rest.strip_suffix('$'))
        .unwrap_or_else(|| panic!("the pattern {pattern:?} is not anchored at both ends"));
    let chars: Vec<char> = body.chars().collect();
    let mut atoms = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let atom = if chars[i] == '[' {
            let end = i + chars[i..]
                .iter()
                .position(|c| *c == ']')
                .expect("a class ends");
            let inner = &chars[i + 1..end];
            let mut ranges = Vec::new();
            let mut j = 0;
            while j < inner.len() {
                if j + 2 < inner.len() && inner[j + 1] == '-' {
                    ranges.push((inner[j], inner[j + 2]));
                    j += 3;
                } else {
                    ranges.push((inner[j], inner[j]));
                    j += 1;
                }
            }
            i = end + 1;
            Atom::Class(ranges)
        } else {
            assert!(
                chars[i].is_ascii_alphanumeric() || " -/".contains(chars[i]),
                "the pattern {pattern:?} holds {:?}, which this matcher does not read",
                chars[i]
            );
            i += 1;
            Atom::Literal(chars[i - 1])
        };
        let mut count = 1;
        if i < chars.len() && chars[i] == '{' {
            let end = i + chars[i..]
                .iter()
                .position(|c| *c == '}')
                .expect("a count ends");
            count = chars[i + 1..end]
                .iter()
                .collect::<String>()
                .parse()
                .expect("a count is a number");
            i = end + 1;
        }
        atoms.push((atom, count));
    }
    atoms
}

fn matches(pattern: &str, text: &str) -> bool {
    let mut chars = text.chars();
    for (atom, count) in compile(pattern) {
        for _ in 0..count {
            let Some(c) = chars.next() else { return false };
            let fits = match &atom {
                Atom::Literal(want) => c == *want,
                Atom::Class(ranges) => ranges.iter().any(|(low, high)| (*low..=*high).contains(&c)),
            };
            if !fits {
                return false;
            }
        }
    }
    chars.next().is_none()
}

fn resolve<'a>(schema: &'a Value, node: &'a Value) -> &'a Value {
    match node.get("$ref").and_then(Value::as_str) {
        Some(reference) => {
            let name = reference
                .strip_prefix("#/$defs/")
                .unwrap_or_else(|| panic!("a $ref outside $defs: {reference}"));
            resolve(schema, &schema["$defs"][name])
        }
        None => node,
    }
}

fn check(schema: &Value, node: &Value, value: &Value, at: &str, faults: &mut Vec<String>) {
    let node = resolve(schema, node);
    let keywords = node.as_object().expect("a schema node is an object");
    for keyword in keywords.keys() {
        assert!(
            [
                "type",
                "properties",
                "required",
                "additionalProperties",
                "enum",
                "minimum",
                "maximum",
                "maxLength"
            ]
            .contains(&keyword.as_str()),
            "the schema node at {at} holds {keyword}, which this walk does not read"
        );
    }
    match node["type"].as_str() {
        Some("object") => {
            assert_eq!(node["additionalProperties"], Value::Bool(false));
            let Some(object) = value.as_object() else {
                faults.push(format!("{at}: not an object"));
                return;
            };
            let properties = node["properties"]
                .as_object()
                .expect("an object schema has properties");
            for (key, member) in object {
                match properties.get(key) {
                    Some(property) => {
                        check(schema, property, member, &format!("{at}/{key}"), faults)
                    }
                    None => faults.push(format!("{at}: the key {key:?} is outside the schema")),
                }
            }
            for required in node["required"].as_array().into_iter().flatten() {
                let required = required.as_str().expect("a required name is a string");
                if !object.contains_key(required) {
                    faults.push(format!("{at}: the required key {required:?} is missing"));
                }
            }
        }
        Some("string") => {
            let Some(text) = value.as_str() else {
                faults.push(format!("{at}: not a string"));
                return;
            };
            if let Some(members) = node["enum"].as_array() {
                if !members.contains(value) {
                    faults.push(format!("{at}: {text:?} is no member of the enum"));
                }
            }
            if let Some(limit) = node["maxLength"].as_u64() {
                if text.chars().count() as u64 > limit {
                    faults.push(format!("{at}: longer than {limit}"));
                }
            }
        }
        Some("integer") => {
            let Some(number) = value.as_u64() else {
                faults.push(format!("{at}: not an unsigned integer"));
                return;
            };
            if node["minimum"]
                .as_u64()
                .is_some_and(|minimum| number < minimum)
                || node["maximum"]
                    .as_u64()
                    .is_some_and(|maximum| number > maximum)
            {
                faults.push(format!("{at}: {number} is outside its bounds"));
            }
        }
        other => {
            panic!("the schema node at {at} has the type {other:?}, which this walk does not read")
        }
    }
}

fn faults(schema: &Value, routes: &Value, call: &Call, request: &Request) -> Vec<String> {
    let mut faults = Vec::new();
    let route = routes["routes"]
        .as_array()
        .expect("routes is a list")
        .iter()
        .find(|route| route["name"] == call.route().name && route["kind"] == "app")
        .expect("the call names an app route of the vendored file");
    if request.method() != route["method"] || request.path() != route["path"] {
        faults.push(format!(
            "{} {} is not the route {}",
            request.method(),
            request.path(),
            route["name"]
        ));
    }
    let headers = routes["headers"].as_object().expect("headers is an object");
    let mut seen = Vec::new();
    for (name, value) in request.headers() {
        if seen.contains(name) {
            faults.push(format!("the header {name} is sent twice"));
        }
        seen.push(*name);
        match headers.get(*name) {
            Some(rule) => {
                let pattern = rule["pattern"].as_str().expect("a header has a pattern");
                if !matches(pattern, value) {
                    faults.push(format!("the value of {name} does not match {pattern}"));
                }
                if rule["length"].as_u64() != Some(value.len() as u64) {
                    faults.push(format!(
                        "the value of {name} is not {} bytes",
                        rule["length"]
                    ));
                }
            }
            None => faults.push(format!("the header {name} is outside the list")),
        }
    }
    for name in ["halvo-account", "authorization"] {
        if !seen.contains(&name) {
            faults.push(format!("the header {name} is missing"));
        }
    }
    let has_type = seen.contains(&"content-type");
    match (route["request"].as_str(), request.body()) {
        (None, None) if !has_type => {}
        (None, _) => {
            faults.push("a route with no request got a body or a content type".to_string())
        }
        (Some(_), None) => faults.push("a route with a request got no body".to_string()),
        (Some(def), Some(body)) => {
            if !has_type {
                faults.push("a body went without its content type".to_string());
            }
            let limit = route["limit"].as_u64().expect("a route has a limit");
            if body.len() as u64 > limit {
                faults.push(format!(
                    "the body has {} bytes, over the limit {limit}",
                    body.len()
                ));
            }
            match serde_json::from_slice::<Value>(body) {
                Ok(value) => {
                    if serde_json::to_vec(&value).ok().as_deref() != Some(body) {
                        faults.push(
                            "the body is not written as JSON.stringify writes it".to_string(),
                        );
                    }
                    check(schema, &schema["$defs"][def], &value, "", &mut faults);
                }
                Err(error) => faults.push(format!("the body is not JSON: {error}")),
            }
        }
    }
    faults
}

fn cap(amount: u64, period: Period) -> Cap {
    Cap {
        amount: NonZeroU64::new(amount).expect("a drawn amount is above 0"),
        period,
    }
}

fn calls(draw: &mut Draw, amount: u64) -> [Call; 6] {
    let period = if draw.below(2) == 0 {
        Period::Week
    } else {
        Period::Month
    };
    [
        Call::Status,
        Call::CheckoutStart(cap(amount, period)),
        Call::CapGet,
        Call::CapSet(cap(amount, period)),
        Call::Manage,
        Call::AccountDelete,
    ]
}

fn value_faults(
    call: &Call,
    account: &AccountId,
    credential: &Credential,
    request: &Request,
) -> Vec<String> {
    let mut faults = Vec::new();
    let header = |name: &str| {
        request
            .headers()
            .iter()
            .find(|(each, _)| *each == name)
            .map(|(_, value)| value.as_str())
    };
    if header("halvo-account") != Some(account.as_str()) {
        faults.push("halvo-account is not the call's account id".to_string());
    }
    let bearer = format!("Bearer {}", credential.as_str());
    if header("authorization") != Some(bearer.as_str()) {
        faults.push("authorization is not Bearer and the call's credential".to_string());
    }
    match call {
        Call::CheckoutStart(cap) | Call::CapSet(cap) => {
            let body: Value = request
                .body()
                .and_then(|bytes| serde_json::from_slice(bytes).ok())
                .unwrap_or(Value::Null);
            let want =
                serde_json::json!({ "cap": { "amount": cap.amount.get(), "period": cap.period } });
            if body != want {
                faults.push(format!("the body {body} is not the call's cap {want}"));
            }
        }
        Call::Status | Call::CapGet | Call::Manage | Call::AccountDelete => {
            if request.body().is_some() {
                faults.push("a call without a cap got a body".to_string());
            }
        }
    }
    faults
}

#[test]
fn every_built_request_carries_the_values_of_its_call() {
    let mut draw = Draw(SEED + 5);
    let mut amounts = vec![CAP_AMOUNT_MIN, CAP_AMOUNT_MAX];
    for _ in 0..DRAWN {
        amounts.push(CAP_AMOUNT_MIN + draw.below(CAP_AMOUNT_MAX - CAP_AMOUNT_MIN + 1));
    }
    let mut built = 0;
    let mut wrong = Vec::new();
    for amount in amounts {
        let account = AccountId::from_random(draw.bytes());
        let credential = Credential::from_random(draw.bytes());
        for call in calls(&mut draw, amount) {
            let request =
                build(&call, &account, &credential).expect("an amount within the bounds is built");
            built += 1;
            for fault in value_faults(&call, &account, &credential, &request) {
                wrong.push(format!(
                    "{} with amount {amount}: {fault}",
                    call.route().name
                ));
            }
        }
    }
    assert_eq!(built, (DRAWN + 2) * 6);
    assert!(
        wrong.is_empty(),
        "{} faults, the first: {:#?}",
        wrong.len(),
        &wrong[..wrong.len().min(5)]
    );
}

#[test]
fn every_built_request_holds_only_what_the_vendored_files_admit() {
    let mut draw = Draw(SEED);
    let mut amounts = vec![
        CAP_AMOUNT_MIN,
        CAP_AMOUNT_MIN + 1,
        9,
        10,
        99_999_999,
        CAP_AMOUNT_MAX - 1,
        CAP_AMOUNT_MAX,
    ];
    for _ in 0..DRAWN {
        amounts.push(CAP_AMOUNT_MIN + draw.below(CAP_AMOUNT_MAX - CAP_AMOUNT_MIN + 1));
    }
    let schema = vendored("schema.v1.json");
    let routes = vendored("routes.v1.json");
    let mut built = 0;
    let mut wrong = Vec::new();
    for amount in amounts {
        let account = AccountId::from_random(draw.bytes());
        let credential = Credential::from_random(draw.bytes());
        for call in calls(&mut draw, amount) {
            let request =
                build(&call, &account, &credential).expect("an amount within the bounds is built");
            built += 1;
            for fault in faults(&schema, &routes, &call, &request) {
                wrong.push(format!(
                    "{} with amount {amount}: {fault}",
                    call.route().name
                ));
            }
        }
    }
    assert_eq!(built, (DRAWN + 7) * 6);
    assert!(
        wrong.is_empty(),
        "{} faults, the first: {:#?}",
        wrong.len(),
        &wrong[..wrong.len().min(5)]
    );
}

#[test]
fn an_amount_outside_the_bounds_is_refused_and_nothing_is_built() {
    let mut draw = Draw(SEED + 1);
    let account = AccountId::from_random(draw.bytes());
    let credential = Credential::from_random(draw.bytes());
    let mut outside = vec![CAP_AMOUNT_MAX + 1, CAP_AMOUNT_MAX + 2, u64::MAX];
    for _ in 0..200 {
        outside.push(CAP_AMOUNT_MAX + 1 + draw.below(u64::MAX - CAP_AMOUNT_MAX - 1));
    }
    for amount in outside {
        for period in [Period::Week, Period::Month] {
            for call in [
                Call::CheckoutStart(cap(amount, period)),
                Call::CapSet(cap(amount, period)),
            ] {
                assert_eq!(
                    build(&call, &account, &credential).err(),
                    Some(Refused::AmountOutOfRange(amount))
                );
            }
        }
    }
    assert_eq!(
        CAP_AMOUNT_MIN, 1,
        "NonZeroU64 refuses every amount below the minimum only while it is 1"
    );
}

#[test]
fn the_compiled_routes_and_header_names_are_those_of_the_vendored_file() {
    let routes = vendored("routes.v1.json");
    let app: Vec<(String, String, String, Option<String>, String, u64)> = routes["routes"]
        .as_array()
        .expect("routes is a list")
        .iter()
        .filter(|route| route["kind"] == "app")
        .map(|route| {
            (
                route["name"].as_str().expect("a name").to_string(),
                route["method"].as_str().expect("a method").to_string(),
                route["path"].as_str().expect("a path").to_string(),
                route["request"].as_str().map(str::to_string),
                route["answer"].as_str().expect("an answer").to_string(),
                route["limit"].as_u64().expect("a limit"),
            )
        })
        .collect();
    let compiled: Vec<(String, String, String, Option<String>, String, u64)> = APP_ROUTES
        .iter()
        .map(|route| {
            (
                route.name.to_string(),
                route.method.to_string(),
                route.path.to_string(),
                route.request.map(str::to_string),
                route.answer.to_string(),
                route.limit as u64,
            )
        })
        .collect();
    assert_eq!(compiled, app);
    let mut names: Vec<&str> = routes["headers"]
        .as_object()
        .expect("headers is an object")
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    let mut compiled_names = HEADER_NAMES.to_vec();
    compiled_names.sort_unstable();
    assert_eq!(compiled_names, names);
}

fn edited(draw: &mut Draw, made: &str, alphabet: &[char]) -> String {
    let mut text: Vec<char> = made.chars().collect();
    for _ in 0..draw.below(3) {
        let pick = alphabet[draw.below(alphabet.len() as u64) as usize];
        let at = draw.below(text.len() as u64 + 1) as usize;
        match draw.below(3) {
            0 if at < text.len() => text[at] = pick,
            1 => text.insert(at, pick),
            _ if at < text.len() => {
                text.remove(at);
            }
            _ => text.push(pick),
        }
    }
    text.into_iter().collect()
}

#[test]
fn ids_and_credentials_parse_exactly_what_the_vendored_patterns_match() {
    let routes = vendored("routes.v1.json");
    let account_pattern = routes["headers"]["halvo-account"]["pattern"]
        .as_str()
        .expect("a pattern");
    let credential_pattern = routes["headers"]["authorization"]["pattern"]
        .as_str()
        .expect("a pattern");
    let mut draw = Draw(SEED + 2);
    let alphabet: Vec<char> = "0123456789abcdefABCDEF-gz4 89".chars().collect();
    let account = AccountId::from_random(draw.bytes());
    let credential = Credential::from_random(draw.bytes());
    let mut ids = vec![
        String::new(),
        account.as_str()[1..].to_string(),
        account.as_str()[..35].to_string(),
        format!("{}0", account.as_str()),
    ];
    let mut credentials = vec![
        String::new(),
        credential.as_str()[1..].to_string(),
        format!("{}0", credential.as_str()),
        format!("0{}", credential.as_str()),
    ];
    for _ in 0..20_000 {
        let made = AccountId::from_random(draw.bytes());
        assert!(matches(account_pattern, made.as_str()));
        ids.push(edited(&mut draw, made.as_str(), &alphabet));
        let made = Credential::from_random(draw.bytes());
        assert!(matches(
            credential_pattern,
            &format!("Bearer {}", made.as_str())
        ));
        credentials.push(edited(&mut draw, made.as_str(), &alphabet));
    }
    let lengths: std::collections::BTreeSet<usize> =
        ids.iter().chain(&credentials).map(String::len).collect();
    for length in [0, 35, 36, 37, 63, 64, 65] {
        assert!(lengths.contains(&length), "no case of {length} characters");
    }
    for text in &ids {
        assert_eq!(
            AccountId::parse(text).is_some(),
            matches(account_pattern, text),
            "{text:?}"
        );
    }
    for text in &credentials {
        assert_eq!(
            Credential::parse(text).is_some(),
            matches(credential_pattern, &format!("Bearer {text}")),
            "{text:?}"
        );
    }
    assert_eq!(ids.len() + credentials.len(), 40_008);
}

#[test]
fn debug_output_holds_no_id_and_no_credential() {
    let mut draw = Draw(SEED + 4);
    let account = AccountId::from_random(draw.bytes());
    let credential = Credential::from_random(draw.bytes());
    let request = build(&Call::CapSet(cap(1, Period::Week)), &account, &credential)
        .expect("a call within the bounds is built");
    assert_eq!(format!("{account:?}"), "AccountId(..)");
    assert_eq!(format!("{credential:?}"), "Credential(..)");
    let printed = format!("{request:?} {:?}", Call::Status);
    assert!(!printed.contains(account.as_str()) && !printed.contains(credential.as_str()));
    assert!(printed.contains("header_names"));
}

#[test]
fn each_call_goes_to_the_route_that_contract_8_names() {
    let mut draw = Draw(SEED + 3);
    let account = AccountId::from_random(draw.bytes());
    let credential = Credential::from_random(draw.bytes());
    let cases = [
        (Call::Status, "status", "GET", "/v1/subscription"),
        (
            Call::CheckoutStart(cap(1, Period::Week)),
            "checkout-start",
            "POST",
            "/v1/checkout",
        ),
        (
            Call::CapSet(cap(1, Period::Month)),
            "cap-set",
            "PUT",
            "/v1/cap",
        ),
        (Call::CapGet, "cap-get", "GET", "/v1/cap"),
        (Call::Manage, "manage", "POST", "/v1/manage"),
        (
            Call::AccountDelete,
            "account-delete",
            "DELETE",
            "/v1/account",
        ),
    ];
    for (call, name, method, path) in cases {
        assert_eq!(call.route().name, name);
        let request =
            build(&call, &account, &credential).expect("a call within the bounds is built");
        assert_eq!((request.method(), request.path()), (method, path), "{name}");
    }
}

fn with_wide_character(text: &str, at: usize, wide: char) -> String {
    let width = wide.len_utf8();
    format!("{}{wide}{}", &text[..at], &text[at + width..])
}

#[test]
fn ids_and_credentials_with_a_character_outside_ascii_are_refused() {
    let mut draw = Draw(SEED + 6);
    let account = AccountId::from_random(draw.bytes());
    let credential = Credential::from_random(draw.bytes());
    let mut cases = 0;
    for wide in ['\u{80}', 'é', 'ÿ', 'ā', '€', '\u{ffff}'] {
        for at in 0..=36 - wide.len_utf8() {
            let text = with_wide_character(account.as_str(), at, wide);
            assert_eq!(text.len(), 36);
            assert!(AccountId::parse(&text).is_none(), "{text:?} was read");
            cases += 1;
        }
        for at in 0..=64 - wide.len_utf8() {
            let text = with_wide_character(credential.as_str(), at, wide);
            assert_eq!(text.len(), 64);
            assert!(Credential::parse(&text).is_none(), "{text:?} was read");
            cases += 1;
        }
    }
    assert_eq!(cases, 4 * (35 + 63) + 2 * (34 + 62));
}

#[test]
fn the_pattern_matcher_reads_the_three_vendored_patterns() {
    assert!(matches("^[0-9a-f]{2}-x$", "a9-x"));
    assert!(!matches("^[0-9a-f]{2}-x$", "a9-xx"));
    assert!(!matches("^[0-9a-f]{2}-x$", "A9-x"));
    assert!(matches("^application/json$", "application/json"));
    assert!(!matches("^application/json$", "application/jsonx"));
}
