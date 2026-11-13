use serde_json::Value;

/// The most steps of one playbook, and the most bytes of one step's compact JSON (A3.4).
pub const STEPS_MAX: usize = 64;
pub const STEP_BYTES_MAX: usize = 2048;

/// How a locator finds a control (A3.1). The trial writes `role` for a control and `text` for
/// a text line of the snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    Role,
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locator {
    pub by: By,
    pub value: String,
}

/// A web target of A3.1: `desc` is the role and the name of the snapshot line, `frame` the
/// origins (with no port) of the frames that hold it from the top document inward. `place` is its
/// place among the lines of the same role, name and frame when the compile saw more than one; A3's
/// form has no key for it, so the step's JSON does not hold it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub role: String,
    pub name: String,
    pub frame: Vec<String>,
    pub credential: bool,
    pub place: Option<usize>,
}

impl Target {
    pub fn desc(&self) -> String {
        format!("{} \"{}\"", self.role, self.name)
    }

    pub fn locator(&self) -> Locator {
        if self.role == "text" {
            Locator {
                by: By::Text,
                value: self.name.clone(),
            }
        } else {
            Locator {
                by: By::Role,
                value: self.role.clone(),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Const(String),
    Slot(String),
    Extract(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arg {
    pub name: String,
    pub from: Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Navigate,
    Click,
    Fill,
    Select,
    Press,
    Extract,
    Handback,
}

impl Verb {
    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Navigate => "navigate",
            Verb::Click => "click",
            Verb::Fill => "fill",
            Verb::Select => "select",
            Verb::Press => "press",
            Verb::Extract => "extract",
            Verb::Handback => "handback",
        }
    }
}

/// One compiled web step of A3.1, as the trial writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub n: usize,
    pub verb: Verb,
    pub target: Option<Target>,
    pub args: Vec<Arg>,
    pub request: bool,
}

/// The category of each task of the trial (A1).
pub fn category(task: &str) -> &'static str {
    match task {
        "bill" => "utility_bill_payment",
        "order" => "shop_order",
        _ => "other",
    }
}

/// The bound fields of an effect step of a task, with their sources (A9.4); the origin's const
/// is the origin of the step's state-changing request.
pub fn bound_args(task: &str, origin: &str) -> Vec<Arg> {
    let arg = |name: &str, from: Source| Arg {
        name: name.to_string(),
        from,
    };
    match task {
        "bill" => vec![
            arg("payee", Source::Slot("payee".into())),
            arg("amount", Source::Extract("amount_due".into())),
            arg("instrument", Source::Slot("instrument".into())),
            arg("origin", Source::Const(origin.to_string())),
        ],
        "order" => vec![
            arg("seller", Source::Slot("seller".into())),
            arg("items", Source::Extract("items".into())),
            arg("amount", Source::Extract("total".into())),
            arg("instrument", Source::Slot("instrument".into())),
            arg("origin", Source::Const(origin.to_string())),
        ],
        _ => Vec::new(),
    }
}

fn text(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

fn arg_json(a: &Arg) -> String {
    let (from, value, key) = match &a.from {
        Source::Const(v) => ("const", text(v), "null".to_string()),
        Source::Slot(k) => ("slot", "null".to_string(), text(k)),
        Source::Extract(k) => ("extract", "null".to_string(), text(k)),
    };
    format!(
        "{{\"name\":{},\"from\":\"{from}\",\"value\":{value},\"key\":{key}}}",
        text(&a.name)
    )
}

fn target_json(t: &Target) -> String {
    let frame: Vec<String> = t.frame.iter().map(|f| text(f)).collect();
    let l = t.locator();
    let locator = match l.by {
        By::Role => format!(
            "{{\"by\":\"role\",\"value\":{},\"name\":{}}}",
            text(&l.value),
            text(&t.name)
        ),
        By::Text => format!(
            "{{\"by\":\"text\",\"value\":{},\"name\":null}}",
            text(&l.value)
        ),
    };
    format!(
        "{{\"desc\":{},\"frame\":[{}],\"credential\":\"{}\",\"locators\":[{locator}]}}",
        text(&t.desc()),
        frame.join(","),
        if t.credential { "yes" } else { "no" }
    )
}

impl Step {
    /// The compact JSON of the step, keys in the order of A3.1, with its class by A4.3.
    pub fn to_json(&self, task: &str) -> String {
        let target = self.target.as_ref().map_or("null".to_string(), target_json);
        let args: Vec<String> = self.args.iter().map(arg_json).collect();
        let line = format!(
            "{{\"v\":1,\"n\":{},\"kind\":\"web\",\"verb\":\"{}\",\"target\":{target},\"args\":[{}],\"pre\":[],\"post\":[],\"effect\":\"{}\",\"class\":\"\"}}",
            self.n,
            self.verb.as_str(),
            args.join(","),
            if self.request { "request" } else { "none" },
        );
        let class = serde_json::from_str::<Value>(&line)
            .map(|v| class_of(category(task), &v))
            .unwrap_or("credential");
        line.replace("\"class\":\"\"", &format!("\"class\":\"{class}\""))
    }
}

/// `class_of(category, step)` of A4.3, the trial's copy of its table, over a step's JSON.
pub fn class_of(category: &str, step: &Value) -> &'static str {
    let kind = step["kind"].as_str().unwrap_or("");
    let verb = step["verb"].as_str().unwrap_or("");
    let effect = step["effect"].as_str().unwrap_or("");
    match kind {
        "web" => {
            if verb == "handback" || step["target"]["credential"].as_str() == Some("yes") {
                "credential"
            } else if effect == "request" {
                match category {
                    "utility_bill_payment" | "tenant_rent" => "payment",
                    "shop_order" | "ride_order" => "purchase",
                    "lease_renewal_or_change" => "sign",
                    _ => "submit",
                }
            } else if matches!(verb, "navigate" | "extract" | "wait") {
                "read"
            } else {
                "prepare"
            }
        }
        "api" => match step["target"]["op"].as_str().unwrap_or("") {
            "mail_list" | "calendar_list" => "read",
            "mail_draft_reply" => "prepare",
            "mail_send" => "send",
            "calendar_delete" => "delete",
            _ => "credential",
        },
        "file" => match verb {
            "read" => "read",
            "write" => "local_edit",
            _ => "credential",
        },
        _ => "credential",
    }
}

/// The bound fields of each class of A9.4, the trial's copy (the names of halvo-schema's
/// `bound_fields`).
fn bound_fields(class: &str) -> &'static [&'static str] {
    match class {
        "payment" => &["payee", "amount", "instrument", "origin"],
        "purchase" => &["seller", "items", "amount", "instrument", "origin"],
        "submit" => &["origin", "fields"],
        "sign" => &["document", "party"],
        _ => &[],
    }
}

const KEYS: [&str; 10] = [
    "v", "n", "kind", "verb", "target", "args", "pre", "post", "effect", "class",
];

fn keys_are(v: &Value, keys: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
}

fn own_text(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.len() <= 1024)
}

fn origin_form(v: &Value) -> bool {
    let Some(s) = v.as_str() else { return false };
    let Some(host) = s
        .strip_prefix("http://")
        .or_else(|| s.strip_prefix("https://"))
    else {
        return false;
    };
    let (name, port) = match host.split_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (host, None),
    };
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        && port.is_none_or(|p| !p.is_empty() && !p.starts_with('0') && p.parse::<u16>().is_ok())
}

fn web_target(t: &Value) -> bool {
    if !keys_are(t, &["desc", "frame", "credential", "locators"]) || !own_text(&t["desc"]) {
        return false;
    }
    let frame_ok = t["frame"]
        .as_array()
        .is_some_and(|f| f.iter().all(origin_form));
    let credential_ok = matches!(t["credential"].as_str(), Some("yes" | "no"));
    let locators_ok = t["locators"].as_array().is_some_and(|ls| {
        !ls.is_empty()
            && ls.iter().all(|l| {
                keys_are(l, &["by", "value", "name"])
                    && own_text(&l["value"])
                    && match l["by"].as_str() {
                        Some("role") => own_text(&l["name"]),
                        Some("label" | "text" | "test_id" | "css") => l["name"].is_null(),
                        _ => false,
                    }
            })
    });
    frame_ok && credential_ok && locators_ok
}

fn name_form(v: &Value) -> bool {
    v.as_str().is_some_and(crate::action::is_name)
}

/// The args of a verb's row of A3.2: each name with the sources it may take and, for a const,
/// the words it may hold (none: any own text).
type Row = Vec<(
    &'static str,
    &'static [&'static str],
    Option<&'static [&'static str]>,
)>;

fn row(verb: &str, effect: &str, class: &str) -> Option<Row> {
    const SLOT_CONST: &[&str] = &["slot", "const"];
    const BOUND: &[&str] = &["const", "slot", "extract"];
    let bound: Row = if effect == "request" {
        bound_fields(class)
            .iter()
            .map(|f| (*f, BOUND, None))
            .collect()
    } else {
        Vec::new()
    };
    Some(match verb {
        "navigate" => vec![("url", SLOT_CONST, None)],
        "click" => bound,
        "fill" => vec![("text", SLOT_CONST, None)],
        "select" => vec![("option", SLOT_CONST, None)],
        "press" => {
            let mut r: Row = vec![("key", &["const"], Some(&["Enter", "Tab", "Escape"]))];
            r.extend(bound);
            r
        }
        "extract" => vec![
            ("as", &["const"], None),
            ("parse", &["const"], Some(&["text", "amount"])),
        ],
        "handback" => vec![("reason", &["const"], Some(&["sign_in"]))],
        _ => return None,
    })
}

/// Why a step line is not of A3: the first rule it breaks.
pub fn check_line(line: &str, category: &str) -> Result<(), &'static str> {
    if line.len() > STEP_BYTES_MAX {
        return Err("length");
    }
    let v: Value = serde_json::from_str(line).map_err(|_| "json")?;
    if !keys_are(&v, &KEYS) {
        return Err("keys");
    }
    if v["v"].as_u64() != Some(1) {
        return Err("v");
    }
    let n = v["n"].as_u64().unwrap_or(0);
    if n == 0 || n > STEPS_MAX as u64 {
        return Err("n");
    }
    if v["kind"].as_str() != Some("web") {
        return Err("kind");
    }
    let verb = v["verb"].as_str().unwrap_or("");
    let effect = v["effect"].as_str().unwrap_or("");
    if !matches!(effect, "none" | "request") {
        return Err("effect");
    }
    if effect == "request" && !matches!(verb, "click" | "press") {
        return Err("effect_of_verb");
    }
    let wants_target = !matches!(verb, "navigate" | "handback");
    if wants_target != !v["target"].is_null() || (wants_target && !web_target(&v["target"])) {
        return Err("target");
    }
    if v["pre"].as_array().is_none() || v["post"].as_array().is_none() {
        return Err("conditions");
    }
    let class = v["class"].as_str().unwrap_or("");
    if class != class_of(category, &v) {
        return Err("class");
    }
    let Some(row) = row(verb, effect, class) else {
        return Err("verb");
    };
    let args = v["args"].as_array().ok_or("args")?;
    if args.len() != row.len() {
        return Err("args");
    }
    for (a, (name, sources, words)) in args.iter().zip(row) {
        if !keys_are(a, &["name", "from", "value", "key"]) || a["name"].as_str() != Some(name) {
            return Err("args");
        }
        let from = a["from"].as_str().unwrap_or("");
        if !sources.contains(&from) {
            return Err("source");
        }
        let holds = (a["value"].is_null(), a["key"].is_null());
        if from == "const" {
            if holds != (false, true) || !own_text(&a["value"]) {
                return Err("value_or_key");
            }
            let value = a["value"].as_str().unwrap_or("");
            if words.is_some_and(|w| !w.contains(&value))
                || (name == "as" && !name_form(&a["value"]))
            {
                return Err("value");
            }
        } else if holds != (true, false) || !name_form(&a["key"]) {
            return Err("value_or_key");
        }
    }
    Ok(())
}

/// Whether the lines are a playbook of A3.4 for the category: 1 to 64 steps, `n` from 1 in
/// order, each step of A3.
pub fn playbook_parses(lines: &[String], category: &str) -> bool {
    !lines.is_empty()
        && lines.len() <= STEPS_MAX
        && lines.iter().enumerate().all(|(i, line)| {
            check_line(line, category).is_ok()
                && serde_json::from_str::<Value>(line)
                    .ok()
                    .and_then(|v| v["n"].as_u64())
                    == Some(i as u64 + 1)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pay_button() -> Target {
        Target {
            role: "button".into(),
            name: "Pay now".into(),
            frame: vec!["http://pay.agent.test".into()],
            credential: false,
            place: None,
        }
    }

    fn steps() -> Vec<Step> {
        vec![
            Step {
                n: 1,
                verb: Verb::Navigate,
                target: None,
                args: vec![Arg {
                    name: "url".into(),
                    from: Source::Slot("start".into()),
                }],
                request: false,
            },
            Step {
                n: 2,
                verb: Verb::Handback,
                target: None,
                args: vec![Arg {
                    name: "reason".into(),
                    from: Source::Const("sign_in".into()),
                }],
                request: false,
            },
            Step {
                n: 3,
                verb: Verb::Extract,
                target: Some(Target {
                    role: "text".into(),
                    name: "84.37".into(),
                    frame: vec![],
                    credential: false,
                    place: None,
                }),
                args: vec![
                    Arg {
                        name: "as".into(),
                        from: Source::Const("amount_due".into()),
                    },
                    Arg {
                        name: "parse".into(),
                        from: Source::Const("amount".into()),
                    },
                ],
                request: false,
            },
            Step {
                n: 4,
                verb: Verb::Select,
                target: Some(Target {
                    role: "combobox".into(),
                    name: "Payment method".into(),
                    ..pay_button()
                }),
                args: vec![Arg {
                    name: "option".into(),
                    from: Source::Slot("instrument".into()),
                }],
                request: false,
            },
            Step {
                n: 5,
                verb: Verb::Click,
                target: Some(pay_button()),
                args: bound_args("bill", "http://pay.agent.test"),
                request: true,
            },
        ]
    }

    #[test]
    fn a_compiled_bill_is_a_playbook_of_a3() {
        let lines: Vec<String> = steps().iter().map(|s| s.to_json("bill")).collect();
        for line in &lines {
            assert_eq!(check_line(line, "utility_bill_payment"), Ok(()), "{line}");
        }
        assert!(playbook_parses(&lines, "utility_bill_payment"));
        assert_eq!(
            lines[4],
            "{\"v\":1,\"n\":5,\"kind\":\"web\",\"verb\":\"click\",\"target\":{\"desc\":\"button \\\"Pay now\\\"\",\"frame\":[\"http://pay.agent.test\"],\"credential\":\"no\",\"locators\":[{\"by\":\"role\",\"value\":\"button\",\"name\":\"Pay now\"}]},\"args\":[{\"name\":\"payee\",\"from\":\"slot\",\"value\":null,\"key\":\"payee\"},{\"name\":\"amount\",\"from\":\"extract\",\"value\":null,\"key\":\"amount_due\"},{\"name\":\"instrument\",\"from\":\"slot\",\"value\":null,\"key\":\"instrument\"},{\"name\":\"origin\",\"from\":\"const\",\"value\":\"http://pay.agent.test\",\"key\":null}],\"pre\":[],\"post\":[],\"effect\":\"request\",\"class\":\"payment\"}"
        );
        assert!(lines[2].contains("{\"by\":\"text\",\"value\":\"84.37\",\"name\":null}"));
        assert!(lines[2].ends_with("\"class\":\"read\"}"));
        assert!(lines[3].ends_with("\"class\":\"prepare\"}"));
        assert!(lines[1].ends_with("\"class\":\"credential\"}"));
    }

    #[test]
    fn the_class_follows_the_table_of_a4_3() {
        let mut order_steps = steps();
        order_steps[4].args = bound_args("order", "http://pay.agent.test");
        let order: Vec<String> = order_steps.iter().map(|s| s.to_json("order")).collect();
        assert!(order[4].ends_with("\"class\":\"purchase\"}"));
        assert!(playbook_parses(&order, "shop_order"));
        assert_eq!(check_line(&order[4], "utility_bill_payment"), Err("class"));
        let bill_args_on_order = steps()[4].to_json("order");
        assert_eq!(check_line(&bill_args_on_order, "shop_order"), Err("args"));
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        let effect = v("{\"kind\":\"web\",\"verb\":\"click\",\"effect\":\"request\",\"target\":{\"credential\":\"no\"}}");
        assert_eq!(class_of("utility_bill_payment", &effect), "payment");
        assert_eq!(class_of("tenant_rent", &effect), "payment");
        assert_eq!(class_of("shop_order", &effect), "purchase");
        assert_eq!(class_of("ride_order", &effect), "purchase");
        assert_eq!(class_of("lease_renewal_or_change", &effect), "sign");
        assert_eq!(class_of("email", &effect), "submit");
        let marked = v("{\"kind\":\"web\",\"verb\":\"click\",\"effect\":\"request\",\"target\":{\"credential\":\"yes\"}}");
        assert_eq!(class_of("utility_bill_payment", &marked), "credential");
        let wait = v("{\"kind\":\"web\",\"verb\":\"wait\",\"effect\":\"none\",\"target\":null}");
        assert_eq!(class_of("email", &wait), "read");
        let fill = v("{\"kind\":\"web\",\"verb\":\"fill\",\"effect\":\"none\",\"target\":{\"credential\":\"no\"}}");
        assert_eq!(class_of("email", &fill), "prepare");
        let send = v("{\"kind\":\"api\",\"verb\":\"call\",\"effect\":\"none\",\"target\":{\"op\":\"mail_send\"}}");
        assert_eq!(class_of("email", &send), "send");
        let delete = v("{\"kind\":\"api\",\"verb\":\"call\",\"effect\":\"none\",\"target\":{\"op\":\"calendar_delete\"}}");
        assert_eq!(class_of("email", &delete), "delete");
        let draft = v("{\"kind\":\"api\",\"verb\":\"call\",\"effect\":\"none\",\"target\":{\"op\":\"mail_draft_reply\"}}");
        assert_eq!(class_of("email", &draft), "prepare");
        let list = v("{\"kind\":\"api\",\"verb\":\"call\",\"effect\":\"none\",\"target\":{\"op\":\"mail_list\"}}");
        assert_eq!(class_of("email", &list), "read");
        let write = v("{\"kind\":\"file\",\"verb\":\"write\",\"effect\":\"none\",\"target\":{}}");
        assert_eq!(class_of("fill", &write), "local_edit");
        let read = v("{\"kind\":\"file\",\"verb\":\"read\",\"effect\":\"none\",\"target\":{}}");
        assert_eq!(class_of("fill", &read), "read");
    }

    #[test]
    fn the_shape_check_refuses_what_a3_refuses() {
        let good = steps()[4].to_json("bill");
        let cases = [
            (good.replace("\"class\":\"payment\"", "\"class\":\"purchase\""), "class"),
            (good.replace("\"effect\":\"request\"", "\"effect\":\"none\""), "class"),
            (good.replace("\"v\":1", "\"v\":2"), "v"),
            (good.replace("\"n\":5", "\"n\":65"), "n"),
            (good.replace("\"n\":5", "\"n\":0"), "n"),
            (good.replace("\"kind\":\"web\"", "\"kind\":\"api\""), "kind"),
            (good.replace("\"locators\":[{\"by\":\"role\",\"value\":\"button\",\"name\":\"Pay now\"}]", "\"locators\":[]"), "target"),
            (good.replace("\"name\":\"Pay now\"}", "\"name\":null}"), "target"),
            (good.replace("\"frame\":[\"http://pay.agent.test\"]", "\"frame\":[\"pay.agent.test\"]"), "target"),
            (good.replace("\"credential\":\"no\"", "\"credential\":\"maybe\""), "target"),
            (good.replace("{\"name\":\"origin\",\"from\":\"const\",\"value\":\"http://pay.agent.test\",\"key\":null}", "{\"name\":\"origin\",\"from\":\"const\",\"value\":null,\"key\":\"origin\"}"), "value_or_key"),
            (good.replace(",{\"name\":\"origin\",\"from\":\"const\",\"value\":\"http://pay.agent.test\",\"key\":null}", ""), "args"),
            (good.replace("\"from\":\"extract\"", "\"from\":\"model\""), "source"),
            (good.replace("\"verb\":\"click\"", "\"verb\":\"fill\""), "effect_of_verb"),
            (good.replace("\"pre\":[]", "\"pre\":null"), "conditions"),
            (good.replace("\"v\":1,", ""), "keys"),
            ("x".to_string(), "json"),
            (format!("{}{}", &good[..good.len() - 1], &format!(",\"pad\":\"{}\"}}", "x".repeat(2048))), "length"),
        ];
        for (line, want) in cases {
            assert_eq!(
                check_line(&line, "utility_bill_payment"),
                Err(want),
                "{line}"
            );
        }
        let extract = steps()[2].to_json("bill");
        assert_eq!(
            check_line(
                &extract.replace("\"amount\"", "\"number\""),
                "utility_bill_payment"
            ),
            Err("value")
        );
        assert_eq!(
            check_line(
                &extract.replace("\"amount_due\"", "\"Amount\""),
                "utility_bill_payment"
            ),
            Err("value")
        );
        let handback = steps()[1].to_json("bill");
        assert_eq!(
            check_line(
                &handback.replace("sign_in", "captcha"),
                "utility_bill_payment"
            ),
            Err("value")
        );
        assert_eq!(
            check_line(
                &handback.replace("\"target\":null", "\"target\":{}"),
                "utility_bill_payment"
            ),
            Err("target")
        );
    }

    #[test]
    fn a_playbook_is_numbered_from_one_and_holds_at_most_64_steps() {
        let lines: Vec<String> = steps().iter().map(|s| s.to_json("bill")).collect();
        let cat = "utility_bill_payment";
        assert!(!playbook_parses(&[], cat));
        assert!(!playbook_parses(&lines[1..], cat));
        let mut swapped = lines.clone();
        swapped.swap(0, 1);
        assert!(!playbook_parses(&swapped, cat));
        let nav = |n: usize| {
            Step {
                n,
                ..steps()[0].clone()
            }
            .to_json("bill")
        };
        let long: Vec<String> = (1..=64).map(nav).collect();
        assert!(playbook_parses(&long, cat));
        let longer: Vec<String> = (1..=65).map(nav).collect();
        assert!(!playbook_parses(&longer, cat));
    }

    #[test]
    fn an_origin_has_a_scheme_a_host_and_at_most_a_port() {
        for good in [
            "http://pay.agent.test",
            "https://a.b",
            "http://pay.agent.test:18090",
        ] {
            assert!(origin_form(&Value::String(good.into())), "{good}");
        }
        for bad in [
            "pay.agent.test",
            "http://",
            "http://Pay.test",
            "http://a.b:0",
            "http://a.b:",
            "http://a.b:99999",
            "ftp://a.b",
        ] {
            assert!(!origin_form(&Value::String(bad.into())), "{bad}");
        }
    }
}
