#[allow(dead_code)]
#[path = "../src/bin/export-names.rs"]
mod export_names;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_schema::kinds::{check_record, RetentionClass, KINDS};
use halvo_schema::request::{
    decode, decode_reply, write_reply, AnalysisRole, CoordinatorBody, CoordinatorRole,
    CoordinatorWrite, Decoded, InterfaceRole, RunnerBody, RunnerRequest, RunnerRole,
};
use halvo_schema::{
    category, ids, kinds, record, request, site, AddressFieldLayer, AgentId, AnalysisWrite, AppId,
    Category, Confidence, Decision, DismissedWorkflow, Domain, DomainSource, Episode, ErrorName,
    Evidence, EvidencePage, FloorLayer, FullWorkflow, InputClass, Instant, InterfaceDecision,
    InterfaceRead, KeyCounts, ObservedSite, ObservedStep, Ordinal, Part, PointerCounts, Position,
    Recurrence, ReplyResult, RequestId, Role, SeedCategory, SiteDomain, SiteHash, SiteToken,
    SiteValue, StoreId, Summary, SummaryId, Support, Tagged, Text, TextLayer, Trigger, Workflow,
    WorkflowId, WorkflowItem, WorkflowState, WorkflowsPage,
};

const HASH: &str = "abababababababababababababababababababababababababababababababab";

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn committed(name: &str) -> String {
    let path = golden(name);
    match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes).unwrap(),
        Err(e) => panic!("{}: {e}", path.display()),
    }
}

fn write_and_compare(name: &str, text: &str) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap()
        .join("halvo-schema/golden");
    std::fs::create_dir_all(&dir).unwrap();
    let written = dir.join(name);
    std::fs::write(&written, text).unwrap();
    let path = golden(name);
    let before = std::fs::read(&path).unwrap_or_default();
    if before != text.as_bytes() {
        let before = String::from_utf8_lossy(&before);
        let line = before
            .lines()
            .zip(text.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| before.lines().count().min(text.lines().count()));
        panic!(
            "{} differs from the file that the test wrote, {}, first at line {}",
            path.display(),
            written.display(),
            line + 1
        );
    }
}

fn file(lines: &[String]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

fn wid(n: u8) -> WorkflowId {
    WorkflowId::parse(&format!("100000000000000000000000000000{n:02x}")).unwrap()
}

fn sid(n: u8) -> SummaryId {
    SummaryId::parse(&format!("200000000000000000000000000000{n:02x}")).unwrap()
}

fn rid(n: u8) -> RequestId {
    RequestId::parse(&format!("400000000000000000000000000000{n:02x}")).unwrap()
}

fn app(s: &str) -> AppId {
    AppId::parse(s).unwrap()
}

fn shop() -> SiteDomain {
    SiteDomain::from(Domain::parse("shop.test").unwrap())
}

fn title_site() -> SiteValue {
    SiteValue::Title {
        key: SiteHash::parse(HASH).unwrap(),
    }
}

fn step() -> ObservedStep {
    ObservedStep {
        app: Some(Tagged {
            value: app("com.example.browser"),
            layer: FloorLayer::Floor,
        }),
        title: Some(Tagged {
            value: Text::parse("Pay bill - Example Power").unwrap(),
            layer: FloorLayer::Floor,
        }),
        input: Some(Tagged {
            value: InputClass::Click,
            layer: FloorLayer::Floor,
        }),
        site: Some(Tagged {
            value: shop(),
            layer: AddressFieldLayer::AddressField,
        }),
        text: Some(Tagged {
            value: Text::parse("Amount due").unwrap(),
            layer: TextLayer::AccessibilityText,
        }),
    }
}

fn keys() -> KeyCounts {
    KeyCounts {
        printable: 42,
        editing: 3,
        navigation: 0,
        enter: 2,
        tab: 4,
        shortcut: 0,
        other: 0,
    }
}

fn pointer() -> PointerCounts {
    PointerCounts {
        click: 7,
        scroll: 12,
        drag: 0,
        r#move: 30,
    }
}

fn segments() -> Vec<Ordinal> {
    vec![Ordinal::new(4410).unwrap(), Ordinal::new(4411).unwrap()]
}

fn proposed() -> FullWorkflow {
    FullWorkflow {
        id: wid(1),
        state: WorkflowState::Proposed,
        category: SeedCategory::UtilityBillPayment,
        site: Some(title_site()),
        app: app("com.example.browser"),
        trigger: Trigger::Schedule,
        recurrence: Recurrence {
            count: 3,
            period: Some(31),
            first: 20705,
            last: 20767,
        },
        confidence: Confidence(87),
        agent: None,
    }
}

fn evidence() -> Evidence {
    Evidence {
        summary: sid(1),
        segments: segments(),
        expires: Instant(1_795_478_400_000_000_000),
        start: Instant(1_794_339_000_000_000_000),
        end: Instant(1_794_339_135_000_000_000),
        hint: Some(SiteToken::parse("shop").unwrap()),
        steps: vec![step()],
    }
}

fn line<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

fn records() -> Vec<String> {
    let approved = FullWorkflow {
        id: wid(2),
        state: WorkflowState::Approved,
        category: SeedCategory::ShopOrder,
        site: Some(SiteValue::Domain {
            key: shop(),
            source: DomainSource::AddressField,
        }),
        app: app("browser.exe"),
        trigger: Trigger::AppContext,
        recurrence: Recurrence {
            count: 4,
            period: None,
            first: 20700,
            last: 20760,
        },
        confidence: Confidence(64),
        agent: Some(AgentId::parse("30000000000000000000000000000001").unwrap()),
    };
    let dismissed = DismissedWorkflow {
        id: wid(3),
        category: SeedCategory::Email,
        site: None,
        app: app("com.example.mail"),
    };
    let summary = Summary {
        id: sid(1),
        category: SeedCategory::UtilityBillPayment,
        site: Some(title_site()),
        app: app("com.example.browser"),
        day: 20767,
        part: Part::Evening,
        minutes: 5,
        keys: Some(keys()),
        pointer: Some(pointer()),
    };
    let bare = Summary {
        id: sid(2),
        category: SeedCategory::SpreadsheetFill,
        site: None,
        app: app("com.example.sheets"),
        day: 20768,
        part: Part::Morning,
        minutes: 20,
        keys: None,
        pointer: None,
    };
    let support = Support {
        workflow: wid(1),
        summary: sid(1),
        day: 20767,
    };
    vec![
        line(&Workflow::Full(proposed())),
        line(&Workflow::Full(approved)),
        line(&Workflow::Dismissed(dismissed)),
        line(&summary),
        line(&bare),
        line(&support),
        line(&evidence()),
    ]
}

fn back<T: serde::de::DeserializeOwned + serde::Serialize>(line: &str) -> String {
    let value: T = serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}"));
    serde_json::to_string(&value).unwrap()
}

#[test]
fn golden_records() {
    let lines = records();
    assert_eq!(lines.len(), 7);
    write_and_compare("records.jsonl", &file(&lines));
    let committed: Vec<String> = committed("records.jsonl")
        .lines()
        .map(str::to_owned)
        .collect();
    for (i, line) in committed.iter().enumerate() {
        let again = match i {
            0..=2 => back::<Workflow>(line),
            3 | 4 => back::<Summary>(line),
            5 => back::<Support>(line),
            _ => back::<Evidence>(line),
        };
        assert_eq!(&again, line, "line {}", i + 1);
    }
    let example = |name: &str| KINDS.iter().find(|k| k.name == name).unwrap();
    assert_eq!(example("workflow").example, committed[0]);
    assert_eq!(example("summary").example, committed[3]);
    assert_eq!(example("support").example, committed[5]);
    assert_eq!(example("evidence").example, committed[6]);
    for (i, kind) in [
        (0, "workflow"),
        (1, "workflow"),
        (3, "summary"),
        (4, "summary"),
        (5, "support"),
        (6, "evidence"),
    ] {
        assert_eq!(
            check_record(example(kind), &committed[i]),
            Ok(()),
            "line {}",
            i + 1
        );
    }
    assert!(check_record(example("workflow"), &committed[2]).is_err());
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
struct Probe {
    probe: u32,
}

impl RunnerBody for Probe {
    type Reply = ();
}

impl CoordinatorBody for Probe {
    type Reply = ();
}

fn episode() -> Episode {
    Episode {
        id: sid(1),
        category: SeedCategory::UtilityBillPayment,
        site: Some(ObservedSite::Title {
            token: SiteToken::parse("shop").unwrap(),
        }),
        app: app("com.example.browser"),
        day: 20767,
        part: Part::Evening,
        minutes: 5,
        keys: Some(keys()),
        pointer: Some(pointer()),
        segments: segments(),
        expires: Instant(1_795_478_400_000_000_000),
        start: Instant(1_794_339_000_000_000_000),
        end: Instant(1_794_339_135_000_000_000),
        steps: vec![step()],
    }
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

fn requests() -> Vec<String> {
    let analysis = AnalysisRole::claim();
    let interface = InterfaceRole::claim();
    let store = StoreId::parse("50000000000000000000000000000001").unwrap();
    let mut with_episode = AnalysisWrite::new(&analysis, rid(1), store.clone());
    with_episode.push(episode()).unwrap();
    let mut with_position = AnalysisWrite::new(&analysis, rid(2), store);
    with_position.commit(Position {
        seg: 4411,
        idx: 212,
    });
    let page = WorkflowsPage {
        workflows: vec![WorkflowItem {
            workflow: Workflow::Full(proposed()),
            summaries: vec![sid(1)],
            hint: Some(SiteToken::parse("shop").unwrap()),
        }],
        next: None,
    };
    let evidence_page = EvidencePage {
        evidence: vec![evidence()],
        next: None,
    };
    vec![
        text(with_episode.to_bytes().unwrap()),
        text(with_position.to_bytes().unwrap()),
        text(
            InterfaceRead::workflows(&interface, rid(3), None)
                .to_bytes()
                .unwrap(),
        ),
        text(
            InterfaceRead::evidence(&interface, rid(4), wid(1), Some(sid(1)))
                .to_bytes()
                .unwrap(),
        ),
        text(
            InterfaceDecision::approve(&interface, rid(5), wid(1))
                .to_bytes()
                .unwrap(),
        ),
        text(
            InterfaceDecision::dismiss(&interface, rid(6), wid(2))
                .to_bytes()
                .unwrap(),
        ),
        text(
            InterfaceDecision::delete(&interface, rid(7), wid(3))
                .to_bytes()
                .unwrap(),
        ),
        text(
            RunnerRequest::new(&RunnerRole::claim(), rid(8), Probe { probe: 1 })
                .to_bytes()
                .unwrap(),
        ),
        text(
            CoordinatorWrite::new(&CoordinatorRole::claim(), rid(9), Probe { probe: 1 })
                .to_bytes()
                .unwrap(),
        ),
        text(write_reply(Some(&rid(1)), Ok(&ReplyResult::Null)).unwrap()),
        text(write_reply(Some(&rid(3)), Ok(&ReplyResult::Workflows(page))).unwrap()),
        text(write_reply(Some(&rid(4)), Ok(&ReplyResult::Evidence(evidence_page))).unwrap()),
        text(write_reply(None, Err(ErrorName::Malformed)).unwrap()),
    ]
}

fn request_back(line: &str) -> String {
    let decoded = decode(line.as_bytes()).unwrap_or_else(|e| panic!("{line}: {e:?}"));
    let bytes = match decoded {
        Decoded::AnalysisWrite(write) => write.to_bytes(),
        Decoded::InterfaceRead(read) => read.to_bytes(),
        Decoded::InterfaceDecision(decision) => decision.to_bytes(),
        Decoded::Raw(raw) => {
            let body: Probe = serde_json::from_str(raw.body().get()).unwrap();
            match raw.role() {
                Some(Role::Runner) => {
                    RunnerRequest::new(&RunnerRole::claim(), raw.id().clone(), body).to_bytes()
                }
                Some(Role::Coordinator) => {
                    CoordinatorWrite::new(&CoordinatorRole::claim(), raw.id().clone(), body)
                        .to_bytes()
                }
                other => panic!("{line}: role {other:?}"),
            }
        }
    };
    text(bytes.unwrap())
}

fn reply_back(line: &str) -> String {
    let reply = decode_reply(line.as_bytes()).unwrap_or_else(|e| panic!("{line}: {e:?}"));
    let outcome = match &reply.outcome {
        Ok(result) => Ok(result),
        Err(error) => Err(*error),
    };
    text(write_reply(reply.id.as_ref(), outcome).unwrap())
}

#[test]
fn golden_requests() {
    let lines = requests();
    assert_eq!(lines.len(), 13);
    write_and_compare("requests.jsonl", &file(&lines));
    let committed = committed("requests.jsonl");
    let committed: Vec<&str> = committed.lines().collect();
    for (i, line) in committed.iter().enumerate() {
        let again = if i < 9 {
            request_back(line)
        } else {
            reply_back(line)
        };
        assert_eq!(&again, line, "line {}", i + 1);
    }
    let roles: Vec<Option<Role>> = committed[..9]
        .iter()
        .map(|line| decode(line.as_bytes()).unwrap().role())
        .collect();
    assert_eq!(
        roles,
        [
            Some(Role::Analysis),
            Some(Role::Analysis),
            Some(Role::Interface),
            Some(Role::Interface),
            Some(Role::Interface),
            Some(Role::Interface),
            Some(Role::Interface),
            Some(Role::Runner),
            Some(Role::Coordinator),
        ]
    );
}

fn parse_out(kind: &str, input: &str) -> Option<String> {
    match kind {
        "app" => AppId::parse(input).ok().map(|v| v.as_str().to_owned()),
        "app_capture" => AppId::from_capture(input)
            .ok()
            .map(|v| v.as_str().to_owned()),
        "domain" => Domain::parse(input).ok().map(|v| v.as_str().to_owned()),
        "registrable" => Domain::registrable(input).map(|v| v.as_str().to_owned()),
        "site_token" => SiteToken::parse(input).ok().map(|v| v.as_str().to_owned()),
        other => panic!("no parser for {other}"),
    }
}

fn parse_cases() -> Vec<(&'static str, String, Option<String>)> {
    let listed: [(&str, &str, Option<&str>); 27] = [
        ("app", "com.apple.Safari", Some("com.apple.Safari")),
        ("app", "chrome.exe", Some("chrome.exe")),
        ("app", "/usr/bin/vim", None),
        ("app", "..", None),
        ("app_capture", "/usr/bin/vim", Some("vim")),
        (
            "app_capture",
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            Some("chrome.exe"),
        ),
        ("domain", "shop.test", Some("shop.test")),
        ("domain", "example.co.uk", Some("example.co.uk")),
        ("domain", "www.ck", Some("www.ck")),
        ("domain", "test", None),
        ("domain", "co.uk", None),
        ("domain", "www.shop.test", None),
        ("domain", "Shop.test", None),
        ("domain", "shop.test.", None),
        ("domain", "192.0.2.1", None),
        ("domain", "0x7f.0x1", None),
        ("domain", "[2001:db8::1]", None),
        ("domain", "pay your bill here", None),
        ("registrable", "www.shop.test", Some("shop.test")),
        ("registrable", "WWW.Shop.Test.", Some("shop.test")),
        ("registrable", "a.b.example.co.uk", Some("example.co.uk")),
        ("registrable", "www.www.ck", Some("www.ck")),
        ("registrable", "a.b.test.ck", Some("b.test.ck")),
        ("registrable", "localhost", None),
        ("registrable", "192.0.2.1", None),
        ("site_token", "northwind-power", Some("northwind-power")),
        ("site_token", "Northwind Power", None),
    ];
    let a63 = "a".repeat(63);
    let b63 = "b".repeat(63);
    let c63 = "c".repeat(63);
    let host_253 = format!("{a63}.{b63}.{c63}.{}.test", "d".repeat(56));
    let host_254 = format!("{a63}.{b63}.{c63}.{}.test", "d".repeat(57));
    let added: Vec<(&str, String, Option<String>)> = vec![
        ("app", String::new(), None),
        ("app", "a".repeat(256), None),
        ("app", "a".repeat(255), Some("a".repeat(255))),
        ("app", "chrome\\x.exe".to_owned(), None),
        ("app", "a\x07b".to_owned(), None),
        ("app", "a\tb".to_owned(), None),
        ("app", "a\x7fb".to_owned(), None),
        ("app", ".".to_owned(), None),
        ("app_capture", "/usr/bin/".to_owned(), None),
        ("app_capture", "C:\\dir\\..".to_owned(), None),
        ("app_capture", "vim".to_owned(), Some("vim".to_owned())),
        ("domain", String::new(), None),
        ("domain", "b\u{fc}cher.test".to_owned(), None),
        (
            "domain",
            format!("{a63}.{b63}.{c63}.{}.test", "d".repeat(57)),
            None,
        ),
        ("domain", "shop..test".to_owned(), None),
        ("domain", ".shop.test".to_owned(), None),
        ("domain", "-shop.test".to_owned(), None),
        ("domain", "shop-.test".to_owned(), None),
        ("domain", format!("{}.test", "a".repeat(64)), None),
        ("domain", format!("{a63}.test"), Some(format!("{a63}.test"))),
        ("domain", "shop_x.test".to_owned(), None),
        ("domain", "shop.123".to_owned(), None),
        ("domain", "shop.0x".to_owned(), None),
        ("domain", "shop.0xff".to_owned(), None),
        (
            "registrable",
            host_253,
            Some(format!("{}.test", "d".repeat(56))),
        ),
        ("registrable", host_254, None),
        ("registrable", "http://shop.test".to_owned(), None),
        ("registrable", "shop.test..".to_owned(), None),
        ("registrable", "b\u{fc}cher.test".to_owned(), None),
        (
            "registrable",
            "SHOP.TEST".to_owned(),
            Some("shop.test".to_owned()),
        ),
        ("site_token", String::new(), None),
        ("site_token", "a".repeat(65), None),
        ("site_token", "a".repeat(64), Some("a".repeat(64))),
        ("site_token", "shop_x".to_owned(), None),
    ];
    listed
        .iter()
        .map(|(k, i, o)| (*k, (*i).to_owned(), o.map(str::to_owned)))
        .chain(added)
        .collect()
}

#[derive(serde::Serialize)]
struct ParseCase<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    #[serde(rename = "in")]
    input: &'a str,
    out: Option<String>,
}

#[test]
fn golden_parse() {
    let mut lines = Vec::new();
    for (kind, input, expected) in parse_cases() {
        let out = parse_out(kind, &input);
        assert_eq!(out, expected, "{kind} {input:?}");
        lines.push(line(&ParseCase {
            kind,
            input: &input,
            out,
        }));
    }
    let host_254 = parse_cases()
        .into_iter()
        .filter(|(k, i, o)| *k == "registrable" && i.len() == 254 && o.is_none())
        .count();
    assert_eq!(host_254, 1);
    write_and_compare("parse.jsonl", &file(&lines));
}

const NAME_KEYS: [&str; 9] = [
    "state", "category", "trigger", "part", "request", "decision", "read", "source", "layer",
];

fn names_in(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, inner) in map {
                if key != "probe" {
                    out.insert(key.clone());
                }
                if let (true, Some(s)) = (NAME_KEYS.contains(&key.as_str()), inner.as_str()) {
                    out.insert(s.to_owned());
                }
                if key == "input" {
                    if let Some(s) = inner.get("value").and_then(|v| v.as_str()) {
                        out.insert(s.to_owned());
                    }
                }
                names_in(inner, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| names_in(item, out)),
        _ => {}
    }
}

fn names_of_lines(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        names_in(&serde_json::from_str(line).unwrap(), &mut out);
    }
    out
}

fn derived_names() -> BTreeSet<String> {
    let mut names = names_of_lines(&committed("records.jsonl"));
    names.extend(names_of_lines(&committed("requests.jsonl")));
    let observed = ObservedSite::AddressField {
        domain: Domain::parse("shop.test").unwrap(),
    };
    let sign_in = SiteValue::Domain {
        key: SiteDomain::from(Domain::parse("shop.test").unwrap()),
        source: DomainSource::SignIn,
    };
    names.extend(names_of_lines(&format!(
        "{}\n{}\n",
        line(&observed),
        line(&sign_in)
    )));
    let enums: Vec<&str> = [
        Category::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
        SeedCategory::ALL.iter().map(|v| v.as_str()).collect(),
        WorkflowState::ALL.iter().map(|v| v.as_str()).collect(),
        Trigger::ALL.iter().map(|v| v.as_str()).collect(),
        Part::ALL.iter().map(|v| v.as_str()).collect(),
        InputClass::ALL.iter().map(|v| v.as_str()).collect(),
        FloorLayer::ALL.iter().map(|v| v.as_str()).collect(),
        AddressFieldLayer::ALL.iter().map(|v| v.as_str()).collect(),
        TextLayer::ALL.iter().map(|v| v.as_str()).collect(),
        DomainSource::ALL.iter().map(|v| v.as_str()).collect(),
        Decision::ALL.iter().map(|v| v.as_str()).collect(),
        Role::ALL.iter().map(|v| v.as_str()).collect(),
        vec![
            "dismissed",
            "title",
            "analysis_write",
            "interface_read",
            "interface_decision",
            "runner",
            "coordinator_write",
            "workflows",
            "evidence",
        ],
        KINDS.iter().flat_map(|k| [k.name, k.family]).collect(),
        [
            RetentionClass::Window,
            RetentionClass::Bounded { days: 93 },
            RetentionClass::Kept,
        ]
        .map(RetentionClass::name)
        .to_vec(),
    ]
    .concat();
    names.extend(enums.into_iter().map(str::to_owned));
    names
}

fn locked_lists() -> BTreeSet<String> {
    [
        category::NAMES,
        ids::NAMES,
        kinds::NAMES,
        record::NAMES,
        request::NAMES,
        site::NAMES,
    ]
    .concat()
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[test]
fn golden_names() {
    let text = export_names::names_file();
    write_and_compare("names.txt", &text);

    let file: BTreeSet<String> = committed("names.txt").lines().map(str::to_owned).collect();
    let in_golden_lines = names_of_lines(&committed("records.jsonl"))
        .into_iter()
        .chain(names_of_lines(&committed("requests.jsonl")))
        .collect::<BTreeSet<String>>();
    let missing: Vec<&String> = in_golden_lines.difference(&file).collect();
    assert!(
        missing.is_empty(),
        "names of the golden lines missing from names.txt: {missing:?}"
    );

    let locked = locked_lists();
    let derived = derived_names();
    let extra: Vec<&String> = locked.difference(&derived).collect();
    let lacking: Vec<&String> = derived.difference(&locked).collect();
    assert!(
        extra.is_empty() && lacking.is_empty(),
        "lines that the rules do not give: {extra:?}; names that the lists lack: {lacking:?}"
    );
    assert!(locked.is_subset(&file));

    for error in ErrorName::ALL {
        let name = error.as_str();
        assert_eq!(
            locked.contains(name),
            name == "store" || name == "state",
            "{name}"
        );
    }
    for absent in ["probe", "instant", "ordinal", "own_text", "hash", "version"] {
        assert!(!locked.contains(absent), "{absent}");
    }
    for absent in ["role", "days", "capture", "own"] {
        assert!(!file.contains(absent), "{absent}");
    }
}

#[test]
fn export_names_writes_the_committed_names_file_on_two_runs() {
    let held = leaf("golden-export-names");
    let dir = held.path();
    let expected = committed("names.txt");
    for run in ["first.txt", "second.txt"] {
        let out = Command::new(env!("CARGO_BIN_EXE_export-names"))
            .arg("--out")
            .arg(dir.join(run))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("names {}\n", expected.lines().count())
        );
        assert_eq!(std::fs::read_to_string(dir.join(run)).unwrap(), expected);
    }
}

struct Leaf(PathBuf);

impl Leaf {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Leaf {
    fn drop(&mut self) {
        if let Some(line) = remove(&self.0) {
            eprintln!("{line}");
        }
    }
}

fn remove(dir: &Path) -> Option<String> {
    let error = std::fs::remove_dir_all(dir).err()?;
    Some(format!("leaf not removed: {}: {error}", dir.display()))
}

fn leaf_name(word: &str, pid: u32, clock: u128, count: u64) -> String {
    format!("{word}-{pid}-{clock}-{count}")
}

fn make_leaf(dir: PathBuf) -> Result<Leaf, String> {
    let parent = dir
        .parent()
        .ok_or_else(|| format!("no parent: {}", dir.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Leaf(dir))
}

fn leaf(word: &str) -> Leaf {
    static CLOCK: OnceLock<u128> = OnceLock::new();
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let clock = *CLOCK.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    });
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let name = leaf_name(word, std::process::id(), clock, count);
    make_leaf(Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn leaf_two_calls_give_two_paths() {
    let first = leaf("golden-leaf");
    let second = leaf("golden-leaf");
    assert_ne!(first.path(), second.path());
    assert!(first.path().is_dir() && second.path().is_dir());
    let stem = |held: &Leaf| {
        let name = held
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        name[..name.rfind('-').unwrap()].to_owned()
    };
    assert_eq!(stem(&first), stem(&second));
}

#[test]
fn leaf_name_differs_by_the_clock_reading_the_counter_and_the_process_id() {
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 7, 2, 0));
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 7, 1, 1));
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 8, 1, 0));
    assert!(leaf_name("w", 7, 1, 0).starts_with("w-7-"));
}

#[test]
fn leaf_that_exists_fails_with_its_path() {
    let first = leaf("golden-leaf");
    let error = make_leaf(first.path().to_path_buf())
        .err()
        .expect("a second making of one leaf fails");
    assert!(
        error.contains(&first.path().display().to_string()),
        "{error}"
    );
}

#[test]
fn leaf_is_gone_after_its_drop() {
    let held = leaf("golden-leaf");
    let path = held.path().to_path_buf();
    std::fs::write(path.join("file"), "x").unwrap();
    drop(held);
    assert!(!path.exists(), "{}", path.display());
}

#[test]
fn leaf_removal_that_fails_gives_one_line_with_the_path() {
    let held = leaf("golden-leaf");
    let path = held.path().to_path_buf();
    drop(held);
    let line = remove(&path).expect("a line for a removal that fails");
    assert!(
        line.contains(&path.display().to_string()) && !line.contains('\n'),
        "{line}"
    );
}

#[test]
fn leaf_lies_under_the_target_directory() {
    let held = leaf("golden-leaf");
    assert!(
        held.path().starts_with(env!("CARGO_TARGET_TMPDIR")),
        "{}",
        held.path().display()
    );
}

const LEAF_CHILD: &str = "HALVO_LEAF_CHILD";

fn leaf_child(test: &str, role: &str, nocapture: bool) -> std::process::Output {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([test, "--exact", "--test-threads=1"])
        .env(LEAF_CHILD, role);
    if nocapture {
        command.arg("--nocapture");
    }
    let out = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("test result: ok. 1 passed;"),
        "{role}: {}\n{stdout}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn leaf_parts(path: &Path) -> (String, u32, u128, u64) {
    let name = path.file_name().unwrap().to_str().unwrap();
    let mut parts = name.rsplitn(4, '-');
    let count = parts.next().unwrap_or_default().parse();
    let clock = parts.next().unwrap_or_default().parse();
    let pid = parts.next().unwrap_or_default().parse();
    let word = parts.next().unwrap_or_default().to_owned();
    match (pid, clock, count) {
        (Ok(pid), Ok(clock), Ok(count)) => (word, pid, clock, count),
        _ => panic!("not word-pid-clock-count: {name}"),
    }
}

fn leaf_clock_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn leaf_from_leaf_holds_the_word_the_process_id_and_the_clock() {
    if std::env::var(LEAF_CHILD).as_deref() != Ok("clock") {
        leaf_child(
            "leaf_from_leaf_holds_the_word_the_process_id_and_the_clock",
            "clock",
            false,
        );
        return;
    }
    let before = leaf_clock_now();
    let held = leaf("golden-leaf-clock");
    let after = leaf_clock_now();
    let (word, pid, clock, _) = leaf_parts(held.path());
    assert_eq!(word, "golden-leaf-clock");
    assert_eq!(pid, std::process::id());
    assert!(
        before <= clock && clock <= after,
        "{before} {clock} {after}"
    );
}

#[test]
fn leaf_refuses_the_directory_that_the_next_leaf_names() {
    if std::env::var(LEAF_CHILD).as_deref() != Ok("next") {
        leaf_child(
            "leaf_refuses_the_directory_that_the_next_leaf_names",
            "next",
            false,
        );
        return;
    }
    let first = leaf("golden-leaf-next");
    let (word, pid, clock, count) = leaf_parts(first.path());
    let next = first
        .path()
        .with_file_name(format!("{word}-{pid}-{clock}-{}", count + 1));
    std::fs::create_dir(&next).unwrap();
    let _next = Leaf(next.clone());
    std::fs::write(next.join("kept"), "kept\n").unwrap();
    let caught = std::panic::catch_unwind(|| leaf("golden-leaf-next"));
    let message = caught
        .err()
        .and_then(|payload| payload.downcast::<String>().ok())
        .expect("leaf() fails on the directory that it names");
    assert!(message.contains(&next.display().to_string()), "{message}");
    assert_eq!(
        std::fs::read_to_string(next.join("kept")).unwrap(),
        "kept\n"
    );
    assert_eq!(std::fs::read_dir(&next).unwrap().count(), 1);
}

#[test]
fn leaf_whose_path_is_gone_drops_without_a_panic() {
    let held = leaf("golden-leaf-gone");
    std::fs::remove_dir_all(held.path()).unwrap();
    let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(held)));
    assert!(dropped.is_ok());
}
