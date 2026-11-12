use halvo_schema::executable::step::{Playbook, Target, PLAYBOOK_STEPS_MAX, STEP_JSON_MAX};
use halvo_schema::executable::{ExecutableStep, Refusal, RefusalCause};

const WT: &str = r#"{"desc":"button Pay","frame":["http://pay.test:18090"],"credential":"no","locators":[{"by":"role","value":"button","name":"Pay"},{"by":"test_id","value":"pay","name":null}]}"#;
const CRED: &str = r#"{"desc":"textbox Password","frame":[],"credential":"yes","locators":[{"by":"label","value":"Password","name":null}]}"#;
const MAIL_LIST: &str = r#"{"adapter":"graph_mail","op":"mail_list"}"#;
const DRAFT: &str = r#"{"adapter":"graph_mail","op":"mail_draft_reply"}"#;
const SEND: &str = r#"{"adapter":"graph_mail","op":"mail_send"}"#;
const CAL_LIST: &str = r#"{"adapter":"graph_calendar","op":"calendar_list"}"#;
const CAL_DELETE: &str = r#"{"adapter":"graph_calendar","op":"calendar_delete"}"#;
const READ_FILE: &str = r#"{"file":"source","sheet":null,"range":null}"#;
const WRITE_FILE: &str = r#"{"file":"workbook","sheet":"sheet","range":"range"}"#;

fn q(s: Option<&str>) -> String {
    s.map_or("null".to_owned(), |s| format!("\"{s}\""))
}

fn arg(name: &str, from: &str, value: Option<&str>, key: Option<&str>) -> String {
    format!(
        r#"{{"name":"{name}","from":"{from}","value":{},"key":{}}}"#,
        q(value),
        q(key)
    )
}

fn c(name: &str, value: &str) -> String {
    arg(name, "const", Some(value), None)
}

fn s(name: &str, key: &str) -> String {
    arg(name, "slot", None, Some(key))
}

fn x(name: &str, key: &str) -> String {
    arg(name, "extract", None, Some(key))
}

fn m(name: &str) -> String {
    arg(name, "model", None, Some("reply_body"))
}

struct Step<'a> {
    n: &'a str,
    kind: &'a str,
    verb: &'a str,
    target: &'a str,
    args: Vec<String>,
    pre: String,
    post: String,
    effect: &'a str,
    class: &'a str,
}

impl Step<'_> {
    fn json(&self) -> String {
        format!(
            r#"{{"v":1,"n":{},"kind":"{}","verb":"{}","target":{},"args":[{}],"pre":[{}],"post":[{}],"effect":"{}","class":"{}"}}"#,
            self.n,
            self.kind,
            self.verb,
            self.target,
            self.args.join(","),
            self.pre,
            self.post,
            self.effect,
            self.class
        )
    }
}

fn web<'a>(verb: &'a str, target: &'a str, args: Vec<String>, class: &'a str) -> Step<'a> {
    Step {
        n: "1",
        kind: "web",
        verb,
        target,
        args,
        pre: String::new(),
        post: String::new(),
        effect: "none",
        class,
    }
}

fn api<'a>(target: &'a str, args: Vec<String>, class: &'a str) -> Step<'a> {
    Step {
        kind: "api",
        ..web("call", target, args, class)
    }
}

fn file<'a>(verb: &'a str, target: &'a str, args: Vec<String>, class: &'a str) -> Step<'a> {
    Step {
        kind: "file",
        ..web(verb, target, args, class)
    }
}

fn pay() -> Vec<String> {
    vec![
        s("payee", "payee"),
        x("amount", "amount_due"),
        s("instrument", "instrument"),
        c("origin", "http://pay.test:18090"),
    ]
}

fn cond(check: &str, target: &str, arg: &str) -> String {
    format!(r#"{{"check":"{check}","target":{target},"arg":{arg}}}"#)
}

fn good_steps() -> Vec<String> {
    let request = |verb, args, class| Step {
        effect: "request",
        ..web(verb, WT, args, class)
    };
    let mut purchase = vec![
        s("seller", "seller"),
        x("items", "items"),
        x("amount", "total"),
        s("instrument", "instrument"),
        c("origin", "http://shop.test:18090"),
    ];
    let mut press_submit = vec![c("key", "Enter"), c("origin", "http://form.test:18090")];
    press_submit.push(s("fields", "answers"));
    let steps = vec![
        Step {
            post: cond(
                "url_is",
                "null",
                &c("url", "http://utility.test:18090/bill"),
            ),
            ..web("navigate", "null", vec![s("url", "start")], "read")
        },
        web(
            "navigate",
            "null",
            vec![c("url", "http://utility.test:18090/")],
            "read",
        ),
        Step {
            pre: cond("enabled", WT, "null"),
            post: cond("present", WT, "null"),
            ..web("click", WT, vec![], "prepare")
        },
        request("click", pay(), "payment"),
        request("click", std::mem::take(&mut purchase), "purchase"),
        request(
            "click",
            vec![s("document", "lease"), s("party", "landlord")],
            "sign",
        ),
        request("click", vec![], "credential"),
        request("click", vec![], "read"),
        request("press", press_submit, "submit"),
        web("press", WT, vec![c("key", "Enter")], "prepare"),
        web("press", WT, vec![c("key", "Tab")], "prepare"),
        web("press", WT, vec![c("key", "Escape")], "prepare"),
        Step {
            post: cond("value_is", WT, &s("text", "payee")),
            ..web("fill", WT, vec![s("text", "payee")], "prepare")
        },
        web("fill", CRED, vec![c("text", "")], "credential"),
        web("select", WT, vec![c("option", "Monthly")], "prepare"),
        web("upload", WT, vec![s("file", "statement")], "prepare"),
        Step {
            pre: cond("text_is", WT, &c("text", "Amount due")),
            post: cond("count_is", WT, &c("count", "1")),
            ..web(
                "extract",
                WT,
                vec![c("as", "amount_due"), c("parse", "amount")],
                "read",
            )
        },
        web(
            "extract",
            WT,
            vec![c("as", "items"), c("parse", "text")],
            "read",
        ),
        web("wait", "null", vec![c("ms", "0")], "read"),
        web("wait", "null", vec![c("ms", "10000")], "read"),
        web("wait", "null", vec![c("ms", "250")], "read"),
        web(
            "handback",
            "null",
            vec![c("reason", "sign_in")],
            "credential",
        ),
        api(MAIL_LIST, vec![], "read"),
        api(DRAFT, vec![m("body")], "prepare"),
        api(DRAFT, vec![c("body", "Thank you, noted.")], "prepare"),
        api(
            SEND,
            vec![x("to", "to"), x("subject", "subject"), m("body")],
            "send",
        ),
        api(CAL_LIST, vec![], "read"),
        api(CAL_DELETE, vec![x("items", "events")], "delete"),
        file("read", READ_FILE, vec![c("as", "rows")], "read"),
        file("write", WRITE_FILE, vec![x("values", "rows")], "local_edit"),
    ];
    steps.iter().map(Step::json).collect()
}

fn refused(text: &str) -> Refusal {
    match ExecutableStep::parse(text.as_bytes()) {
        Ok(_) => panic!("taken: {text}"),
        Err(refusal) => refusal,
    }
}

fn expect(text: &str, type_name: &str, rule: &str, key: &str) {
    let refusal = refused(text);
    assert_eq!(refusal.cause(), RefusalCause::Malformed, "{text}");
    assert_eq!(
        (
            refusal.type_name(),
            refusal.rule(),
            refusal.key().unwrap_or("-")
        ),
        (type_name, rule, key),
        "{text}"
    );
}

#[test]
fn every_verb_of_the_table_reads_and_writes_back_to_the_same_bytes() {
    for text in good_steps() {
        let step = ExecutableStep::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(step.to_json().unwrap(), text);
    }
    let call = ExecutableStep::parse(
        api(
            SEND,
            vec![x("to", "to"), x("subject", "subject"), m("body")],
            "send",
        )
        .json()
        .as_bytes(),
    )
    .unwrap();
    assert!(matches!(call.target, Some(Target::Api(_))));
}

#[test]
fn the_step_of_a_verb_has_the_kind_target_and_effect_of_its_row() {
    let cases: Vec<(Step, &str, &str)> = vec![
        (
            Step {
                kind: "api",
                ..web("click", WT, vec![], "prepare")
            },
            "verb_of_kind",
            "verb",
        ),
        (
            Step {
                kind: "web",
                ..api(MAIL_LIST, vec![], "read")
            },
            "verb_of_kind",
            "verb",
        ),
        (
            Step {
                kind: "file",
                ..web("navigate", "null", vec![s("url", "start")], "read")
            },
            "verb_of_kind",
            "verb",
        ),
        (
            web("navigate", WT, vec![s("url", "start")], "read"),
            "target_of_verb",
            "target",
        ),
        (
            web("wait", WT, vec![c("ms", "1")], "read"),
            "target_of_verb",
            "target",
        ),
        (
            web("handback", WT, vec![c("reason", "sign_in")], "credential"),
            "target_of_verb",
            "target",
        ),
        (
            web("click", "null", vec![], "prepare"),
            "target_of_verb",
            "target",
        ),
        (
            web("click", MAIL_LIST, vec![], "prepare"),
            "target_of_verb",
            "target",
        ),
        (
            web("fill", READ_FILE, vec![s("text", "payee")], "prepare"),
            "target_of_verb",
            "target",
        ),
        (api(WT, vec![], "read"), "target_of_verb", "target"),
        (api("null", vec![], "read"), "target_of_verb", "target"),
        (
            file("read", WRITE_FILE, vec![c("as", "rows")], "read"),
            "target_of_verb",
            "target",
        ),
        (
            file(
                "read",
                r#"{"file":"source","sheet":"sheet","range":null}"#,
                vec![c("as", "rows")],
                "read",
            ),
            "target_of_verb",
            "target",
        ),
        (
            file(
                "write",
                r#"{"file":"workbook","sheet":"sheet","range":null}"#,
                vec![x("values", "rows")],
                "local_edit",
            ),
            "target_of_verb",
            "target",
        ),
        (
            file(
                "read",
                r#"{"file":"source","sheet":null,"range":"range"}"#,
                vec![c("as", "rows")],
                "read",
            ),
            "target_of_verb",
            "target",
        ),
        (
            file(
                "write",
                r#"{"file":"workbook","sheet":null,"range":"range"}"#,
                vec![x("values", "rows")],
                "local_edit",
            ),
            "target_of_verb",
            "target",
        ),
        (
            file("write", READ_FILE, vec![x("values", "rows")], "local_edit"),
            "target_of_verb",
            "target",
        ),
        (
            file("write", WT, vec![x("values", "rows")], "local_edit"),
            "target_of_verb",
            "target",
        ),
        (
            Step {
                effect: "request",
                ..web("fill", WT, vec![s("text", "payee")], "prepare")
            },
            "effect_of_verb",
            "effect",
        ),
        (
            Step {
                effect: "request",
                ..web("navigate", "null", vec![s("url", "start")], "read")
            },
            "effect_of_verb",
            "effect",
        ),
        (
            Step {
                effect: "request",
                ..web("select", WT, vec![s("option", "plan")], "prepare")
            },
            "effect_of_verb",
            "effect",
        ),
        (
            Step {
                effect: "request",
                ..web("upload", WT, vec![s("file", "f")], "prepare")
            },
            "effect_of_verb",
            "effect",
        ),
        (
            Step {
                effect: "request",
                ..api(
                    SEND,
                    vec![x("to", "to"), x("subject", "subject"), m("body")],
                    "send",
                )
            },
            "effect_of_verb",
            "effect",
        ),
        (
            Step {
                effect: "request",
                ..file("write", WRITE_FILE, vec![x("values", "rows")], "local_edit")
            },
            "effect_of_verb",
            "effect",
        ),
    ];
    for (step, rule, key) in cases {
        expect(&step.json(), "ExecutableStep", rule, key);
    }
}

#[test]
fn the_args_of_a_step_are_those_of_its_verb_by_name_order_and_source() {
    let request = |args, class| Step {
        effect: "request",
        ..web("click", WT, args, class)
    };
    let mut swapped = pay();
    swapped.swap(0, 1);
    let mut short = pay();
    short.pop();
    let mut model_amount = pay();
    model_amount[1] = m("amount");
    let mut const_amount = pay();
    const_amount[1] = c("amount", "100.00");
    let cases: Vec<(Step, &str)> = vec![
        (web("navigate", "null", vec![], "read"), "args_of_verb"),
        (
            web(
                "navigate",
                "null",
                vec![s("url", "a"), s("url", "b")],
                "read",
            ),
            "args_of_verb",
        ),
        (
            web("navigate", "null", vec![s("address", "start")], "read"),
            "args_of_verb",
        ),
        (web("click", WT, pay(), "payment"), "args_of_verb"),
        (request(short, "payment"), "args_of_verb"),
        (request(swapped, "payment"), "args_of_verb"),
        (request(vec![], "payment"), "args_of_verb"),
        (request(pay(), "purchase"), "args_of_verb"),
        (request(pay(), "submit"), "args_of_verb"),
        (web("press", WT, vec![], "prepare"), "args_of_verb"),
        (
            Step {
                effect: "request",
                ..web("press", WT, vec![c("key", "Enter")], "payment")
            },
            "args_of_verb",
        ),
        (web("fill", WT, vec![], "prepare"), "args_of_verb"),
        (
            web(
                "extract",
                WT,
                vec![c("parse", "amount"), c("as", "amount_due")],
                "read",
            ),
            "args_of_verb",
        ),
        (
            api(MAIL_LIST, vec![s("reply_senders", "reply_senders")], "read"),
            "args_of_verb",
        ),
        (api(DRAFT, vec![], "prepare"), "args_of_verb"),
        (
            api(SEND, vec![x("to", "to"), m("body")], "send"),
            "args_of_verb",
        ),
        (api(CAL_DELETE, vec![], "delete"), "args_of_verb"),
        (file("read", READ_FILE, vec![], "read"), "args_of_verb"),
        (
            web("navigate", "null", vec![x("url", "link")], "read"),
            "source_of_arg",
        ),
        (
            web("navigate", "null", vec![m("url")], "read"),
            "source_of_arg",
        ),
        (
            web("upload", WT, vec![c("file", "a.pdf")], "prepare"),
            "source_of_arg",
        ),
        (web("fill", WT, vec![m("text")], "prepare"), "source_of_arg"),
        (
            web("fill", WT, vec![x("text", "amount_due")], "prepare"),
            "source_of_arg",
        ),
        (
            web("select", WT, vec![m("option")], "prepare"),
            "source_of_arg",
        ),
        (
            web("press", WT, vec![s("key", "key")], "prepare"),
            "source_of_arg",
        ),
        (
            web(
                "extract",
                WT,
                vec![s("as", "a"), c("parse", "text")],
                "read",
            ),
            "source_of_arg",
        ),
        (
            web("wait", "null", vec![s("ms", "ms")], "read"),
            "source_of_arg",
        ),
        (
            web(
                "handback",
                "null",
                vec![s("reason", "reason")],
                "credential",
            ),
            "source_of_arg",
        ),
        (request(model_amount, "payment"), "source_of_arg"),
        (
            api(
                SEND,
                vec![m("to"), x("subject", "subject"), m("body")],
                "send",
            ),
            "source_of_arg",
        ),
        (
            api(SEND, vec![x("to", "to"), m("subject"), m("body")], "send"),
            "source_of_arg",
        ),
        (
            api(
                SEND,
                vec![x("to", "to"), x("subject", "subject"), s("body", "body")],
                "send",
            ),
            "source_of_arg",
        ),
        (
            api(DRAFT, vec![s("body", "draft")], "prepare"),
            "source_of_arg",
        ),
        (api(CAL_DELETE, vec![m("items")], "delete"), "source_of_arg"),
        (
            file("write", WRITE_FILE, vec![s("values", "rows")], "local_edit"),
            "source_of_arg",
        ),
        (
            file(
                "write",
                WRITE_FILE,
                vec![c("values", "1,2,3")],
                "local_edit",
            ),
            "source_of_arg",
        ),
        (
            file("read", READ_FILE, vec![s("as", "rows")], "read"),
            "source_of_arg",
        ),
        (
            web("press", WT, vec![c("key", "enter")], "prepare"),
            "value_of_arg",
        ),
        (
            web("press", WT, vec![c("key", "Space")], "prepare"),
            "value_of_arg",
        ),
        (
            web("press", WT, vec![c("key", "")], "prepare"),
            "value_of_arg",
        ),
        (
            web("wait", "null", vec![c("ms", "10001")], "read"),
            "value_of_arg",
        ),
        (
            web("wait", "null", vec![c("ms", "010")], "read"),
            "value_of_arg",
        ),
        (
            web("wait", "null", vec![c("ms", "1e3")], "read"),
            "value_of_arg",
        ),
        (
            web("wait", "null", vec![c("ms", "-1")], "read"),
            "value_of_arg",
        ),
        (
            web("wait", "null", vec![c("ms", "")], "read"),
            "value_of_arg",
        ),
        (
            web("wait", "null", vec![c("ms", "500 ")], "read"),
            "value_of_arg",
        ),
        (
            web("wait", "null", vec![c("ms", "4294967296")], "read"),
            "value_of_arg",
        ),
        (
            web(
                "handback",
                "null",
                vec![c("reason", "captcha")],
                "credential",
            ),
            "value_of_arg",
        ),
        (
            web(
                "handback",
                "null",
                vec![c("reason", "Sign_in")],
                "credential",
            ),
            "value_of_arg",
        ),
        (
            web(
                "extract",
                WT,
                vec![c("as", "amount_due"), c("parse", "number")],
                "read",
            ),
            "value_of_arg",
        ),
        (
            web(
                "extract",
                WT,
                vec![c("as", "Amount"), c("parse", "amount")],
                "read",
            ),
            "value_of_arg",
        ),
        (
            web(
                "extract",
                WT,
                vec![c("as", ""), c("parse", "amount")],
                "read",
            ),
            "value_of_arg",
        ),
        (
            file("read", READ_FILE, vec![c("as", "rows-1")], "read"),
            "value_of_arg",
        ),
    ];
    for (step, rule) in cases {
        expect(&step.json(), "ExecutableStep", rule, "args");
    }
}

#[test]
fn an_arg_holds_value_or_key_by_its_source_and_a_model_key_is_reply_body() {
    let nav = |a: String| web("navigate", "null", vec![a], "read").json();
    expect(
        &nav(arg("url", "const", Some("x"), Some("start"))),
        "Arg",
        "value_or_key",
        "args",
    );
    expect(
        &nav(arg("url", "const", None, None)),
        "Arg",
        "value_or_key",
        "args",
    );
    expect(
        &nav(arg("url", "slot", Some("x"), Some("start"))),
        "Arg",
        "value_or_key",
        "args",
    );
    expect(
        &nav(arg("url", "slot", None, None)),
        "Arg",
        "value_or_key",
        "args",
    );
    expect(
        &nav(arg("url", "slot", Some("x"), None)),
        "Arg",
        "value_or_key",
        "args",
    );
    let draft = |a: String| api(DRAFT, vec![a], "prepare").json();
    expect(
        &draft(arg("body", "model", None, Some("amount"))),
        "ModelOutput",
        "unknown_name",
        "key",
    );
    expect(
        &draft(arg("body", "model", None, Some("reply"))),
        "ModelOutput",
        "unknown_name",
        "key",
    );
    expect(
        &draft(arg("body", "model", Some("x"), None)),
        "Arg",
        "value_or_key",
        "args",
    );
    expect(
        &nav(arg("Url", "slot", None, Some("start"))),
        "Name",
        "name_form",
        "name",
    );
    expect(
        &nav(arg("url", "slot", None, Some("Start"))),
        "Name",
        "name_form",
        "key",
    );
    expect(
        &nav(arg("url", "page", None, Some("start"))),
        "Source",
        "unknown_name",
        "from",
    );
    expect(
        &nav(c("url", &"a".repeat(1025))),
        "OwnText",
        "own_text_length",
        "value",
    );
}

#[test]
fn a_target_and_a_condition_have_the_keys_of_their_form() {
    let click = |target: &str| web("click", target, vec![], "prepare").json();
    expect(
        &click(
            r#"{"desc":"button Pay","frame":[],"credential":"no","locators":[{"by":"role","value":"button","name":null}]}"#,
        ),
        "Locator",
        "name_of_by",
        "name",
    );
    expect(
        &click(
            r#"{"desc":"button Pay","frame":[],"credential":"no","locators":[{"by":"label","value":"Pay","name":"Pay"}]}"#,
        ),
        "Locator",
        "name_of_by",
        "name",
    );
    expect(
        &click(
            r#"{"desc":"button Pay","frame":[],"credential":"no","locators":[{"by":"xpath","value":"//a","name":null}]}"#,
        ),
        "LocatorBy",
        "unknown_name",
        "by",
    );
    expect(
        &click(r#"{"desc":"button Pay","frame":[],"credential":"no","locators":[]}"#),
        "WebTarget",
        "no_locator",
        "locators",
    );
    expect(
        &click(
            r#"{"desc":"button Pay","frame":["http://Pay.test:1"],"credential":"no","locators":[{"by":"css","value":".pay","name":null}]}"#,
        ),
        "WebOrigin",
        "origin_form",
        "frame",
    );
    expect(
        &click(
            r#"{"desc":"button Pay","frame":null,"credential":"no","locators":[{"by":"css","value":".pay","name":null}]}"#,
        ),
        "list",
        "not_a_list",
        "frame",
    );
    expect(
        &click(
            r#"{"desc":"button Pay","frame":[],"credential":"maybe","locators":[{"by":"css","value":".pay","name":null}]}"#,
        ),
        "YesNo",
        "unknown_name",
        "credential",
    );
    expect(
        &click(
            r#"{"desc":"button Pay","frame":[],"credential":"no","locators":[{"by":"css","value":".pay","name":null}],"op":null}"#,
        ),
        "WebTarget",
        "unknown_key",
        "target",
    );
    expect(
        &click(
            r#"{"desc":"button Pay","file":"workbook","frame":[],"credential":"no","locators":[{"by":"css","value":".pay","name":null}]}"#,
        ),
        "WebTarget",
        "unknown_key",
        "target",
    );
    expect(
        &click(r#"{"button":"Pay"}"#),
        "Target",
        "target_form",
        "target",
    );
    expect(&click(r#"[]"#), "Target", "not_an_object", "target");
    let call = |target: &str| api(target, vec![], "read").json();
    expect(
        &call(r#"{"adapter":"graph_mail","op":"calendar_list"}"#),
        "ApiTarget",
        "op_of_adapter",
        "op",
    );
    expect(
        &call(r#"{"adapter":"graph_calendar","op":"mail_list"}"#),
        "ApiTarget",
        "op_of_adapter",
        "op",
    );
    expect(
        &call(r#"{"adapter":"graph","op":"mail_list"}"#),
        "Adapter",
        "unknown_name",
        "adapter",
    );
    expect(
        &call(r#"{"adapter":"graph_mail","op":"mail_list","base":null}"#),
        "ApiTarget",
        "unknown_key",
        "target",
    );
    let with_post = |post: String| {
        Step {
            post,
            ..web("click", WT, vec![], "prepare")
        }
        .json()
    };
    expect(
        &with_post(cond("url_is", WT, &c("url", "http://a.test:1/"))),
        "Cond",
        "keys_of_check",
        "post",
    );
    expect(
        &with_post(cond("url_is", "null", "null")),
        "Cond",
        "keys_of_check",
        "post",
    );
    expect(
        &with_post(cond("present", WT, &c("t", "x"))),
        "Cond",
        "keys_of_check",
        "post",
    );
    expect(
        &with_post(cond("enabled", "null", "null")),
        "Cond",
        "keys_of_check",
        "post",
    );
    expect(
        &with_post(cond("text_is", WT, "null")),
        "Cond",
        "keys_of_check",
        "post",
    );
    expect(
        &with_post(cond("count_is", "null", &c("count", "1"))),
        "Cond",
        "keys_of_check",
        "post",
    );
    expect(
        &with_post(cond("url_is", "null", &s("url", "start"))),
        "Cond",
        "url_is_const",
        "arg",
    );
    expect(
        &with_post(cond("text_is", WT, &m("text"))),
        "Cond",
        "model_in_condition",
        "arg",
    );
    expect(
        &with_post(cond("present", MAIL_LIST, "null")),
        "WebTarget",
        "unknown_key",
        "target",
    );
    expect(
        &with_post(cond("visible", WT, "null")),
        "Check",
        "unknown_name",
        "check",
    );
}

#[test]
fn the_hand_made_inputs_are_refused_each_with_its_rule() {
    let good = web("click", WT, vec![], "prepare").json();
    let swap = |from: &str, to: &str| {
        assert_eq!(good.matches(from).count(), 1, "{from}");
        good.replacen(from, to, 1)
    };
    let array = format!(r#"[1,1,"web","click",{WT},[],[],[],"none","prepare"]"#);
    expect(&array, "ExecutableStep", "not_an_object", "-");
    expect(
        &swap(r#""verb":"click","#, r#""verb":"click","verb":"click","#),
        "ExecutableStep",
        "repeated_key",
        "-",
    );
    expect(
        &swap(r#""v":1,"#, r#""v":1,"note":null,"#),
        "ExecutableStep",
        "unknown_key",
        "-",
    );
    expect(
        &swap(r#""v":1,"#, r#""v":1,"op":"load","#),
        "ExecutableStep",
        "unknown_key",
        "-",
    );
    expect(
        &swap(r#""v":1,"#, r#""v":1,"number":1,"#),
        "ExecutableStep",
        "unknown_key",
        "-",
    );
    expect(
        &swap(r#""n":1"#, r#""n":"1""#),
        "number",
        "not_a_count",
        "n",
    );
    expect(&swap(r#""n":1"#, r#""n":-0"#), "number", "not_a_count", "n");
    expect(
        &swap(r#""n":1"#, r#""n":1e0"#),
        "number",
        "not_a_count",
        "n",
    );
    expect(
        &swap(r#""n":1"#, r#""n":1.0"#),
        "number",
        "not_a_count",
        "n",
    );
    expect(&swap(r#""n":1"#, r#""n":0"#), "number", "zero", "n");
    expect(
        &swap(r#""verb":"click""#, r#""verb":"Click""#),
        "Verb",
        "unknown_name",
        "verb",
    );
    expect(
        &swap(r#""kind":"web""#, r#""kind":"WEB""#),
        "StepKind",
        "unknown_name",
        "kind",
    );
    expect(
        &swap(r#""class":"prepare""#, r#""class":"pay""#),
        "ActionClass",
        "unknown_name",
        "class",
    );
    expect(
        &swap(r#""class":"prepare""#, r#""class":"ended""#),
        "ActionClass",
        "unknown_name",
        "class",
    );
    expect(
        &swap(r#""effect":"none""#, r#""effect":"no""#),
        "StepEffect",
        "unknown_name",
        "effect",
    );
    expect(
        &swap(r#""effect":"none""#, r#""effect":null"#),
        "StepEffect",
        "not_a_string",
        "effect",
    );
    expect(&swap(r#""v":1"#, r#""v":2"#), "V1", "version", "v");
    expect(
        &swap(r#","pre":[]"#, ""),
        "ExecutableStep",
        "missing_key",
        "pre",
    );
    expect(
        &swap(r#""args":[]"#, r#""args":null"#),
        "list",
        "not_a_list",
        "args",
    );
    expect(
        &swap(r#""pre":[]"#, r#""pre":{}"#),
        "list",
        "not_a_list",
        "pre",
    );
    let desc = swap(
        r#""desc":"button Pay""#,
        &format!(r#""desc":"{}""#, "d".repeat(1025)),
    );
    expect(&desc, "OwnText", "own_text_length", "desc");
}

fn sized(value_len: usize) -> String {
    let target = format!(
        r#"{{"desc":"{}","frame":[],"credential":"no","locators":[{{"by":"css","value":"{}","name":null}}]}}"#,
        "d".repeat(1024),
        "v".repeat(value_len)
    );
    web("click", &target, vec![], "prepare").json()
}

#[test]
fn a_step_of_more_than_2048_bytes_of_compact_json_is_refused_with_length() {
    let base = sized(0).len();
    assert_eq!(STEP_JSON_MAX, 2048);
    assert!(base < 2048);
    let exact = sized(2048 - base);
    assert_eq!(exact.len(), 2048);
    assert_eq!(
        ExecutableStep::parse(exact.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        exact
    );
    let over = sized(2048 - base + 1);
    let refusal = refused(&over);
    assert_eq!(refusal.cause(), RefusalCause::Length);
    assert_eq!(
        (refusal.type_name(), refusal.rule()),
        ("ExecutableStep", "json_length")
    );
    let spaced = exact.replace(r#","kind""#, r#", "kind""#);
    assert!(ExecutableStep::parse(spaced.as_bytes()).is_ok());
    let mut step = ExecutableStep::parse(exact.as_bytes()).unwrap();
    step.n = std::num::NonZeroU32::new(10).unwrap();
    assert_eq!(step.to_json().unwrap_err().cause(), RefusalCause::Length);
    assert!(serde_json::to_string(&step).is_err());
}

fn wait(n: usize) -> String {
    Step {
        n: &n.to_string(),
        ..web("wait", "null", vec![c("ms", "1")], "read")
    }
    .json()
}

fn lines(range: std::ops::RangeInclusive<usize>) -> String {
    range.map(|n| format!("{}\n", wait(n))).collect()
}

fn playbook_refused(text: &str, cause: RefusalCause, rule: &str) {
    let refusal = Playbook::parse_file(text.as_bytes()).expect_err(text);
    assert_eq!((refusal.cause(), refusal.rule()), (cause, rule), "{text:?}");
}

#[test]
fn a_playbook_file_holds_1_to_64_steps_one_per_line_numbered_from_1() {
    let three = lines(1..=3);
    let playbook = Playbook::parse_file(three.as_bytes()).unwrap();
    assert_eq!(playbook.steps().len(), 3);
    assert_eq!(playbook.to_file().unwrap(), three);
    let full = lines(1..=PLAYBOOK_STEPS_MAX);
    assert_eq!(
        Playbook::parse_file(full.as_bytes())
            .unwrap()
            .to_file()
            .unwrap(),
        full
    );
    playbook_refused(&lines(1..=65), RefusalCause::Length, "steps");
    playbook_refused("", RefusalCause::Malformed, "last_line");
    playbook_refused("\n", RefusalCause::Malformed, "empty_line");
    playbook_refused(
        &format!("{}\n\n{}\n", wait(1), wait(2)),
        RefusalCause::Malformed,
        "empty_line",
    );
    playbook_refused(
        &format!("{}\n{}\n", wait(1), wait(3)),
        RefusalCause::Malformed,
        "number_order",
    );
    playbook_refused(
        &format!("{}\n{}\n", wait(2), wait(1)),
        RefusalCause::Malformed,
        "number_order",
    );
    playbook_refused(
        &format!("{}\n{}\n", wait(1), wait(1)),
        RefusalCause::Malformed,
        "number_order",
    );
    playbook_refused(
        &format!("{}\n", wait(2)),
        RefusalCause::Malformed,
        "number_order",
    );
    playbook_refused(
        &format!("{}\r\n", wait(1)),
        RefusalCause::Malformed,
        "text_form",
    );
    playbook_refused(
        &format!("\u{feff}{}\n", wait(1)),
        RefusalCause::Malformed,
        "text_form",
    );
    playbook_refused(&wait(1), RefusalCause::Malformed, "last_line");
    playbook_refused(
        &format!("{}\n[]\n", wait(1)),
        RefusalCause::Malformed,
        "not_an_object",
    );
    let refusal = Playbook::parse_file(b"\xff\n").unwrap_err();
    assert_eq!(refusal.rule(), "utf8");
}

#[test]
fn a_step_numbered_above_64_is_refused_with_length() {
    assert!(ExecutableStep::parse(wait(64).as_bytes()).is_ok());
    for n in [65, 1000, 4_294_967_295] {
        let refusal = refused(&wait(n));
        assert_eq!(refusal.cause(), RefusalCause::Length, "{n}");
        assert_eq!(
            (refusal.type_name(), refusal.rule(), refusal.key()),
            ("ExecutableStep", "step_number", Some("n"))
        );
    }
}

#[test]
fn a_playbook_value_holds_the_same_rules() {
    let step = |n: u32| {
        let mut step = ExecutableStep::parse(wait(1).as_bytes()).unwrap();
        step.n = std::num::NonZeroU32::new(n).unwrap();
        step
    };
    assert_eq!(Playbook::new(Vec::new()).unwrap_err().rule(), "no_step");
    let many: Vec<ExecutableStep> = (1..=65).map(step).collect();
    assert_eq!(
        Playbook::new(many).unwrap_err().cause(),
        RefusalCause::Length
    );
    assert_eq!(
        Playbook::new(vec![step(2)]).unwrap_err().rule(),
        "number_order"
    );
    assert!(Playbook::new((1..=64).map(step).collect()).is_ok());
}
