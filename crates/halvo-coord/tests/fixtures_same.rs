use halvo_coord::types::commands::LIST;
use halvo_coord::types::fixtures::{self, State};
use halvo_coord::types::{Category, ConfigView};
use halvo_coord::words;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const TRACKED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/tests/fixtures/coord");

fn fresh_dir(name: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    for n in 0.. {
        let dir = base.join(format!("{name}-{}-{n}", std::process::id()));
        if std::fs::create_dir_all(base).is_ok() && std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
    unreachable!()
}

fn names(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n != ".gitattributes")
        .collect()
}

#[test]
fn fixtures_same() {
    let dir = fresh_dir("fixtures_same");
    assert_eq!(fixtures::write(&dir).unwrap(), 38);
    let written = names(&dir);
    assert_eq!(written.len(), 38);
    assert_eq!(
        names(Path::new(TRACKED)),
        written,
        "ui/tests/fixtures/coord/ holds other files than fixtures::write writes"
    );
    let mut differ = Vec::new();
    for name in &written {
        let new = std::fs::read(dir.join(name)).unwrap();
        let old = std::fs::read(Path::new(TRACKED).join(name)).unwrap();
        if new != old {
            differ.push(name.clone());
        }
    }
    assert!(
        differ.is_empty(),
        "these fixtures differ from the types: {differ:?}; write them again with `cargo run -p halvo-coord --bin coord-words -- fixtures ui/tests/fixtures/coord`"
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(TRACKED).join(".gitattributes")).unwrap(),
        "* text eol=lf\n"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_second_write_changes_no_file() {
    let dir = fresh_dir("fixtures_twice");
    fixtures::write(&dir).unwrap();
    let first: Vec<Vec<u8>> = names(&dir)
        .iter()
        .map(|n| std::fs::read(dir.join(n)).unwrap())
        .collect();
    assert_eq!(fixtures::write(&dir).unwrap(), 38);
    let second: Vec<Vec<u8>> = names(&dir)
        .iter()
        .map(|n| std::fs::read(dir.join(n)).unwrap())
        .collect();
    assert_eq!(first, second);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn json(name: &str) -> Value {
    let text = std::fs::read_to_string(Path::new(TRACKED).join(name)).unwrap();
    assert!(
        text.ends_with('\n') && text.matches('\n').count() == 1,
        "{name}"
    );
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_files_are_those_of_i8_3() {
    let mut want: BTreeSet<String> = ["commands.json", "words.json", "reply-ok.json"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    want.insert("reply-closed.json".into());
    want.insert("reply-next-same.json".into());
    for s in State::ALL {
        want.insert(format!("state-{}.json", s.name()));
    }
    for c in [
        "stale",
        "presenter",
        "early",
        "hold",
        "state",
        "budget",
        "queue_full",
        "busy",
        "no_service",
        "unknown_item",
        "unknown_agent",
        "malformed",
    ] {
        want.insert(format!("reply-error-{c}.json"));
    }
    assert_eq!(State::ALL.len(), 21);
    assert_eq!(want.len(), 38);
    assert_eq!(names(Path::new(TRACKED)), want);
    for command in LIST {
        for stem in command.replies {
            assert!(want.contains(&format!("{stem}.json")), "{stem}");
        }
    }
    assert_eq!(json("reply-ok.json"), serde_json::json!({"ok": "yes"}));
    assert_eq!(
        json("reply-closed.json"),
        serde_json::json!({"closed": "yes"})
    );
    assert_eq!(
        json("reply-error-queue_full.json"),
        serde_json::json!({"error": "queue_full"})
    );
    assert_eq!(
        json("reply-next-same.json"),
        serde_json::json!({"seq": 1, "rows": [], "items": [], "note": null})
    );
}

fn objects(text: &str) -> Vec<Vec<String>> {
    let b = text.as_bytes();
    let mut stack: Vec<(bool, usize, bool)> = Vec::new();
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' => {
                out.push(Vec::new());
                stack.push((true, out.len() - 1, true));
            }
            b'[' => stack.push((false, 0, false)),
            b'}' | b']' => {
                stack.pop();
            }
            b',' => {
                if let Some(top) = stack.last_mut() {
                    top.2 = top.0;
                }
            }
            b'"' => {
                let start = i + 1;
                i += 1;
                while b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                if let Some(top) = stack.last_mut() {
                    if top.0 && top.2 {
                        out[top.1].push(text[start..i].to_owned());
                        top.2 = false;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn hex32(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 32
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

#[test]
fn every_state_file_has_the_shapes_of_i8_1_and_its_facts() {
    for state in State::ALL {
        let file = json(&format!("state-{}.json", state.name()));
        let o = file.as_object().unwrap();
        let text = std::fs::read_to_string(
            Path::new(TRACKED).join(format!("state-{}.json", state.name())),
        )
        .unwrap();
        for keys in objects(&text) {
            let want: &[&str] = match keys.first().map(String::as_str) {
                Some("snapshot") => &["snapshot", "card"],
                Some("seq") => &["seq", "rows", "items", "note"],
                Some("agent") if keys.get(1).map(String::as_str) == Some("workflow") => &[
                    "agent", "workflow", "name", "status", "word", "line", "say", "level", "next",
                    "controls",
                ],
                Some("agent") => &["agent", "name", "lines", "runs"],
                Some("control") => &["control", "text"],
                Some("item") => &[
                    "item",
                    "agent",
                    "kind",
                    "presenter",
                    "armed",
                    "title",
                    "text",
                    "fields",
                    "params",
                    "choices",
                ],
                Some("label") => &["label", "value", "from", "from_text"],
                Some("name") => &["name", "label", "candidates", "typed"],
                Some("n") => &["n", "value", "from"],
                Some("choice") => &["choice", "text", "safe"],
                Some("key") if keys.get(2).map(String::as_str) == Some("fields") => {
                    &["key", "text", "fields"]
                }
                Some("key") => &["key", "text"],
                Some("day") => &["day", "text", "effects"],
                _ => panic!("{state}: an object with the keys {keys:?}"),
            };
            assert_eq!(keys, want, "{state}");
        }
        assert_eq!(o["card"].is_null(), *state != State::Card, "{state}");
        for line in o["card"]["lines"].as_array().into_iter().flatten() {
            let keys: Vec<&String> = line.as_object().unwrap().keys().collect();
            assert!(
                keys.len() == 2 && line["key"].is_string() && line["text"].is_string(),
                "{state}: a card line holds {keys:?}"
            );
        }
        let snap = o["snapshot"].as_object().unwrap();
        assert_eq!(snap["seq"], 1);
        let rows = snap["rows"].as_array().unwrap();
        let facts = fixtures::facts(*state);
        assert_eq!(rows.len(), facts.len(), "{state}");
        for row in rows {
            let r = row.as_object().unwrap();
            assert!(hex32(&r["agent"]) && hex32(&r["workflow"]));
            let f = facts
                .iter()
                .find(|f| f.agent.as_str() == r["agent"])
                .expect("a row has its facts");
            assert_eq!(f.workflow.id.as_str(), r["workflow"]);
            let n = Category::ALL
                .iter()
                .position(|c| *c == f.workflow.category)
                .unwrap()
                + 1;
            assert_eq!(
                f.workflow.site.as_deref(),
                Some(format!("site{n}.example").as_str())
            );
            let name = words::text(
                "name.with_site",
                &[
                    (
                        "category",
                        &words::text(&format!("category.{}", f.workflow.category.name()), &[])
                            .unwrap(),
                    ),
                    ("site", &format!("Site{n}")),
                ],
            )
            .unwrap();
            assert_eq!(r["name"], name.as_str());
            assert_eq!(
                r["level"],
                words::text(&format!("line.level.{}", f.config.reasoning().name()), &[])
                    .unwrap()
                    .as_str()
            );
            assert_eq!(f.today, 20_760);
        }
        for item in snap["items"].as_array().unwrap() {
            let i = item.as_object().unwrap();
            let choices = i["choices"].as_array().unwrap();
            assert_eq!(choices[0]["safe"], "yes");
            assert!(choices[1..].iter().all(|c| c["safe"] == "no"));
            assert_eq!(i["presenter"].is_null(), i["kind"] != "confirm");
            let f = facts
                .iter()
                .find(|f| f.agent.as_str() == i["agent"])
                .expect("an item has its agent");
            let open = f.doing.item().expect("the facts hold the open item");
            assert_eq!(open.item.as_str(), i["item"]);
            assert_eq!(open.kind.name(), i["kind"]);
        }
    }
}

#[test]
fn the_ids_count_up_from_one_in_each_state() {
    for state in [State::Ten, State::Queue, State::Problem] {
        let facts = fixtures::facts(state);
        let mut cats: Vec<_> = facts.iter().collect();
        cats.sort_by_key(|f| f.workflow.category);
        for (k, f) in cats.iter().enumerate() {
            assert_eq!(f.workflow.id.as_str(), format!("{:032x}", 2 * k + 1));
            assert_eq!(f.agent.as_str(), format!("{:032x}", 2 * k + 2));
        }
    }
    let ten = fixtures::facts(State::Ten);
    let not_yet: Vec<Category> = ten
        .iter()
        .filter(|f| f.config.runtime().name() == "none")
        .map(|f| f.workflow.category)
        .collect();
    assert_eq!(
        not_yet,
        vec![
            Category::TenantRent,
            Category::LeaseRenewalOrChange,
            Category::TenantApplicationReview,
            Category::RideOrder,
            Category::DailyChartCheck
        ]
    );
}

#[test]
fn each_kind_has_the_choices_of_i3_1_with_the_safe_one_first() {
    let column: [(&str, [&str; 2]); 8] = [
        ("confirm", ["reject", "allow"]),
        ("sign_in", ["decline", "done"]),
        ("link_account", ["decline", "done"]),
        ("stuck", ["decline", "done"]),
        ("check", ["try_again", "went_through"]),
        ("setup", ["none", "choose"]),
        ("ready", ["skip", "start"]),
        ("show", ["decline", "done"]),
    ];
    let mut kinds = BTreeSet::new();
    for state in State::ALL {
        let file = json(&format!("state-{}.json", state.name()));
        for item in file["snapshot"]["items"].as_array().unwrap() {
            let kind = item["kind"].as_str().unwrap();
            let got: Vec<(&str, &str)> = item["choices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| (c["choice"].as_str().unwrap(), c["safe"].as_str().unwrap()))
                .collect();
            if item["presenter"] == "native" {
                assert_eq!(got, [("present", "yes")], "{state}: the one choice of I3.4");
            } else {
                let want = if *state == State::NeedsSignInBind {
                    ["decline", "allow"]
                } else {
                    column.iter().find(|(k, _)| *k == kind).unwrap().1
                };
                assert_eq!(got, [(want[0], "yes"), (want[1], "no")], "{state}: {kind}");
            }
            kinds.insert(kind.to_owned());
        }
    }
    assert_eq!(kinds.len(), 8, "the fixtures hold an item of each kind");
}

fn snapshot(state: &str) -> Value {
    json(&format!("state-{state}.json"))["snapshot"].clone()
}

fn statuses(state: &str) -> Vec<String> {
    snapshot(state)["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["status"].as_str().unwrap().to_owned())
        .collect()
}

fn count(list: &[String], name: &str) -> usize {
    list.iter().filter(|s| *s == name).count()
}

fn labels(class: &str, fields: &[&str], effects: usize) -> Vec<String> {
    (0..effects)
        .flat_map(|_| fields)
        .map(|f| words::text(&format!("field.{class}.{f}"), &[]).unwrap())
        .collect()
}

#[test]
fn the_states_hold_the_counts_of_i9_4() {
    assert_eq!(State::ALL.len(), 21, "That is 21 states.");

    let empty = snapshot("empty");
    assert_eq!(empty["rows"].as_array().unwrap().len(), 0);
    assert_eq!(empty["items"].as_array().unwrap().len(), 0);

    let ten = statuses("ten");
    assert_eq!(ten.len(), 10, "the ten seed agents");
    assert_eq!(count(&ten, "not_yet"), 5, "five not_yet");

    for (state, kind) in [
        ("needs_confirm_page", "confirm"),
        ("needs_confirm_native", "confirm"),
        ("needs_sign_in", "sign_in"),
        ("needs_link_account", "link_account"),
        ("needs_stuck", "stuck"),
        ("needs_check", "check"),
        ("needs_setup", "setup"),
        ("needs_ready", "ready"),
        ("needs_show", "show"),
    ] {
        let items = snapshot(state)["items"].as_array().unwrap().clone();
        assert_eq!(items.len(), 1, "{state}");
        assert_eq!(items[0]["kind"], kind, "{state}");
    }

    let page = &snapshot("needs_confirm_page")["items"][0];
    let got: Vec<&str> = page["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        got,
        labels("send", &["to", "subject", "body"], 3),
        "three effects of the class send"
    );
    assert_eq!(
        page["title"],
        words::text("confirm.send.title", &[]).unwrap().as_str()
    );

    let native = &snapshot("needs_confirm_native")["items"][0];
    let got: Vec<&str> = native["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        got,
        labels("payment", &["payee", "amount", "instrument", "origin"], 1),
        "one effect of the class payment"
    );
    assert_eq!(
        native["title"],
        words::text("confirm.payment.title", &[]).unwrap().as_str()
    );

    let three = snapshot("setup_three");
    let params = three["items"][0]["params"].as_array().unwrap().clone();
    assert_eq!(params.len(), 3, "three parameters");
    assert_eq!(
        params.iter().filter(|p| p["typed"] == "yes").count(),
        1,
        "one typed"
    );

    let card = json("state-card.json");
    assert_eq!(statuses("card"), ten, "the roster of ten");
    assert_eq!(
        card["card"]["runs"].as_array().unwrap().len(),
        2,
        "two runs"
    );

    let queue = statuses("queue");
    assert_eq!(queue.len(), 3);
    assert_eq!(count(&queue, "working"), 1, "one agent working");
    assert_eq!(count(&queue, "waiting_turn"), 2, "two waiting");

    let none = snapshot("no_service");
    assert_eq!(none["rows"].as_array().unwrap().len(), 0, "empty lists");
    assert_eq!(none["items"].as_array().unwrap().len(), 0, "empty lists");
    assert_eq!(
        none["note"],
        words::text("note.no_service", &[]).unwrap().as_str(),
        "and the note"
    );
}
