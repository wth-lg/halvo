#![allow(dead_code)]

use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::{
    ActionClass, AgentConfig, Bound, BoundField, Caps, Digest, EffectId, ExecutableStep,
};

pub fn q(s: Option<&str>) -> String {
    s.map_or_else(|| "null".to_owned(), |s| format!("\"{s}\""))
}

pub fn arg(name: &str, from: &str, value: Option<&str>, key: Option<&str>) -> String {
    format!(
        r#"{{"name":"{name}","from":"{from}","value":{},"key":{}}}"#,
        q(value),
        q(key)
    )
}

pub fn konst(name: &str, value: &str) -> String {
    arg(name, "const", Some(value), None)
}

pub fn slot(name: &str, key: &str) -> String {
    arg(name, "slot", None, Some(key))
}

pub fn extract(name: &str, key: &str) -> String {
    arg(name, "extract", None, Some(key))
}

pub fn model(name: &str) -> String {
    arg(name, "model", None, Some("reply_body"))
}

pub fn web_target(credential: &str) -> String {
    format!(
        r#"{{"desc":"button Pay","frame":[],"credential":"{credential}","locators":[{{"by":"role","value":"button","name":"Pay"}}]}}"#
    )
}

pub fn bound_args(class: ActionClass) -> Vec<String> {
    class
        .bound_fields()
        .iter()
        .map(|f| match f.as_str() {
            "body" => model("body"),
            "origin" => konst("origin", "https://pay.example:443"),
            name => slot(name, name),
        })
        .collect()
}

pub fn step_json(
    n: u32,
    kind: &str,
    verb: &str,
    target: Option<String>,
    args: &[String],
    effect: &str,
    class: ActionClass,
) -> String {
    format!(
        r#"{{"v":1,"n":{n},"kind":"{kind}","verb":"{verb}","target":{},"args":[{}],"pre":[],"post":[],"effect":"{effect}","class":"{}"}}"#,
        target.unwrap_or_else(|| "null".to_owned()),
        args.join(","),
        class.as_str()
    )
}

pub fn parse_step(json: &str) -> ExecutableStep {
    match ExecutableStep::parse(json.as_bytes()) {
        Ok(step) => step,
        Err(refusal) => panic!("the test's step does not parse: {refusal}: {json}"),
    }
}

/// A web step of `verb` with no state-changing request, stored with `class`.
pub fn web(n: u32, verb: &str, class: ActionClass) -> ExecutableStep {
    let (target, args) = match verb {
        "navigate" => (None, vec![slot("url", "start")]),
        "click" => (Some(web_target("no")), vec![]),
        "fill" => (Some(web_target("no")), vec![slot("text", "payee")]),
        "select" => (Some(web_target("no")), vec![konst("option", "Card")]),
        "press" => (Some(web_target("no")), vec![konst("key", "Enter")]),
        "upload" => (Some(web_target("no")), vec![slot("file", "document")]),
        "extract" => (
            Some(web_target("no")),
            vec![konst("as", "amount_due"), konst("parse", "amount")],
        ),
        "wait" => (None, vec![konst("ms", "500")]),
        "handback" => (None, vec![konst("reason", "sign_in")]),
        other => panic!("no web verb {other}"),
    };
    parse_step(&step_json(n, "web", verb, target, &args, "none", class))
}

/// A web `click` or `press` that issues a state-changing request, with the bound args of `class`, stored with
/// `class`.
pub fn web_request(n: u32, verb: &str, class: ActionClass) -> ExecutableStep {
    let mut args = Vec::new();
    if verb == "press" {
        args.push(konst("key", "Enter"));
    }
    args.extend(bound_args(class));
    parse_step(&step_json(
        n,
        "web",
        verb,
        Some(web_target("no")),
        &args,
        "request",
        class,
    ))
}

/// A web `click` on a target that the executor reported as a credential field.
pub fn web_credential(n: u32, effect: &str, class: ActionClass) -> ExecutableStep {
    let args = if effect == "request" {
        bound_args(class)
    } else {
        Vec::new()
    };
    parse_step(&step_json(
        n,
        "web",
        "click",
        Some(web_target("yes")),
        &args,
        effect,
        class,
    ))
}

/// An api step of `op`, stored with `class`.
pub fn api(n: u32, op: &str, class: ActionClass) -> ExecutableStep {
    let adapter = if op.starts_with("mail") {
        "graph_mail"
    } else {
        "graph_calendar"
    };
    let args = match op {
        "mail_list" | "calendar_list" => vec![],
        "mail_draft_reply" => vec![model("body")],
        "mail_send" => bound_args(ActionClass::Send),
        "calendar_delete" => bound_args(ActionClass::Delete),
        other => panic!("no op {other}"),
    };
    let target = format!(r#"{{"adapter":"{adapter}","op":"{op}"}}"#);
    parse_step(&step_json(
        n,
        "api",
        "call",
        Some(target),
        &args,
        "none",
        class,
    ))
}

/// A file step of `verb` (`read` or `write`), stored with `class`.
pub fn file(n: u32, verb: &str, class: ActionClass) -> ExecutableStep {
    let (target, args) = match verb {
        "read" => (
            r#"{"file":"source","sheet":null,"range":null}"#.to_owned(),
            vec![konst("as", "rows")],
        ),
        "write" => (
            r#"{"file":"workbook","sheet":"sheet","range":"range"}"#.to_owned(),
            vec![extract("values", "rows")],
        ),
        other => panic!("no file verb {other}"),
    };
    parse_step(&step_json(
        n,
        "file",
        verb,
        Some(target),
        &args,
        "none",
        class,
    ))
}

pub fn config_json(
    category: &str,
    autonomy: &str,
    allow: &[&str],
    caps: (&str, &str, &str),
    runtime: &str,
) -> String {
    let allow: Vec<String> = allow.iter().map(|a| format!("\"{a}\"")).collect();
    format!(
        r#"{{"v":1,"workflow":"30000000000000000000000000000001","agent":"40000000000000000000000000000001","rev":1,"category":"{category}","tier":"on_device","reasoning":"medium","autonomy":"{autonomy}","allow":[{}],"caps":{{"untrusted":"{}","private":"{}","external":"{}"}},"limits":{{"steps":20,"wall_s":120,"model_tokens":0}},"budget":null,"permissions":[],"runtime":"{runtime}","playbook":null}}"#,
        allow.join(","),
        caps.0,
        caps.1,
        caps.2
    )
}

pub fn config(json: &str) -> AgentConfig {
    match AgentConfig::parse(json.as_bytes()) {
        Ok(config) => config,
        Err(refusal) => panic!("the test's configuration does not parse: {refusal}: {json}"),
    }
}

pub fn caps(untrusted: YesNo, private: YesNo, external: YesNo) -> Caps {
    Caps {
        untrusted,
        private,
        external,
    }
}

pub fn effect_id(last: u8) -> EffectId {
    let mut bytes = [0x60; 16];
    bytes[15] = last;
    EffectId::from_random(bytes)
}

pub fn digest_of(c: char) -> Digest {
    match Digest::parse(&c.to_string().repeat(64)) {
        Ok(d) => d,
        Err(refusal) => panic!("not a digest: {refusal}"),
    }
}

pub fn field(json: &str) -> BoundField {
    match BoundField::parse(json.as_bytes()) {
        Ok(f) => f,
        Err(refusal) => panic!("the test's field does not parse: {refusal}: {json}"),
    }
}

pub fn bound(class: ActionClass, fields: &[String]) -> Bound {
    let fields = fields.iter().map(|f| field(f)).collect();
    match Bound::new(class, fields) {
        Ok(b) => b,
        Err(refusal) => panic!("the test's bound object does not parse: {refusal}"),
    }
}

pub fn bf(name: &str, from: &str, origin: Option<&str>, value: &str) -> String {
    format!(
        r#"{{"name":"{name}","from":"{from}","origin":{},"value":["{value}"]}}"#,
        q(origin)
    )
}
