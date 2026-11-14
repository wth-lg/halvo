mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{files_under, refused, run, s, scratch, write};
use halvo_sample::compose::Rates;
use halvo_sample::manifest::TierManifest;
use halvo_sample::script::Sites;
use halvo_sample::site_data::{Flow, Page, Role, Service};
use halvo_schema::{Category, SeedCategory};

fn page(id: &str, title: &str, password: bool, card: bool) -> Page {
    Page {
        page: id.to_owned(),
        title: title.to_owned(),
        text: format!("Text of {id}."),
        password_input: password,
        card_title: card,
    }
}

/// Made-up services: for each seed category two services, and four of the category `other`; the category
/// spreadsheet_fill has two native services of one app. Every flow holds a page whose title holds a word of
/// its own service.
fn services(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut n = 0;
    let put = |s: Service, out: &mut Vec<PathBuf>| {
        let p = dir.join(format!("{}.json", s.service));
        write(&p, &s.write());
        out.push(p);
    };
    for c in SeedCategory::ALL {
        for k in 0..2 {
            n += 1;
            let name = format!("zz-{n}");
            let word = format!("quill{n}");
            let native = *c == SeedCategory::SpreadsheetFill;
            let mut pages = vec![
                page("main", &format!("Do {word} {{thing}}"), false, false),
                page("look", &format!("Look {word} {k}"), false, false),
            ];
            let mut task = vec!["main".to_owned()];
            if !native {
                pages.insert(0, page("sign-in", &format!("Sign in {word}"), true, false));
                task.insert(0, "sign-in".to_owned());
            }
            if *c == SeedCategory::ShopOrder {
                pages.push(page(
                    "pay",
                    &format!("Pay 4242 4242 4242 4242 {word}"),
                    false,
                    true,
                ));
                task.push("pay".to_owned());
            }
            put(
                Service {
                    service: name.clone(),
                    category: Category::from(*c),
                    web: !native,
                    host: (!native).then(|| format!("{name}.test")),
                    app: native.then(|| "org.example.sheets".to_owned()),
                    parameters: vec!["thing".to_owned()],
                    values: vec![(
                        "thing".to_owned(),
                        vec!["one".to_owned(), "two & three".to_owned()],
                    )],
                    pages,
                    flows: vec![
                        Flow {
                            flow: "do".to_owned(),
                            role: Role::Task,
                            pages: task,
                        },
                        Flow {
                            flow: "look".to_owned(),
                            role: Role::NearMiss,
                            pages: vec!["look".to_owned()],
                        },
                    ],
                },
                &mut out,
            );
        }
    }
    for k in 0..4 {
        n += 1;
        let name = format!("zz-{n}");
        let native = k >= 2;
        put(
            Service {
                service: name.clone(),
                category: Category::Other,
                web: !native,
                host: (!native).then(|| format!("{name}.test")),
                app: native.then(|| "org.example.sheets".to_owned()),
                parameters: Vec::new(),
                values: Vec::new(),
                pages: vec![
                    page("home", &format!("Home thrush{n}"), false, false),
                    page("more", &format!("More thrush{n}"), false, false),
                ],
                flows: vec![Flow {
                    flow: "read".to_owned(),
                    role: Role::Other,
                    pages: vec!["home".to_owned(), "more".to_owned()],
                }],
            },
            &mut out,
        );
    }
    out
}

fn rates(streams: u64, planted: u64) -> Rates {
    Rates {
        sides: (Sites::Seen, Sites::Unseen),
        seed: 5,
        browser: "org.chromium.Chromium".to_owned(),
        streams,
        planted,
        interleave: 3,
        noise: 3,
        near_miss: 1,
        day1: 1_767_225_600_000_000_000,
        canaries: Vec::new(),
    }
}

fn compose(dir: &Path, rates_text: &str, sites: &[PathBuf], out: &str) -> common::Out {
    write(&dir.join("rates.toml"), rates_text);
    let mut args = vec![
        "compose".to_owned(),
        "--rates".to_owned(),
        s(&dir.join("rates.toml")),
        "--out".to_owned(),
        s(&dir.join(out)),
    ];
    args.extend(sites.iter().map(|p| s(p)));
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&a)
}

fn scripts(dir: &Path) -> Vec<String> {
    files_under(dir)
        .into_iter()
        .map(|f| s(&dir.join(f)))
        .collect()
}

#[test]
fn compose_split_build_and_check_labels_agree() {
    let dir = scratch("compose");
    let sites = services(&dir.join("sites"));
    let o = compose(&dir, &rates(20, 4).write(), &sites, "c");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines[0], "compose seed=5 sides=seen,unseen");
    assert!(
        lines[1].starts_with("composed streams=20 tasks="),
        "{}",
        lines[1]
    );
    assert!(
        lines[1].ends_with(
            " planted=4,4,4,4,4,4,4,4,4,4 near_miss=2,2,2,2,2,2,2,2,2,2 twice=2,2,2,2,2,2,2,2,2,2"
        ) || lines[1].contains(" planted=4,4,4,4,4,4,4,4,4,4 "),
        "{}",
        lines[1]
    );
    assert!(lines[2].starts_with("medians stretch_seconds="));
    let again = compose(&dir, &rates(20, 4).write(), &sites, "c2");
    assert_eq!(again.stdout, o.stdout);
    for f in files_under(&dir.join("c")) {
        assert_eq!(
            fs::read(dir.join("c").join(&f)).ok(),
            fs::read(dir.join("c2").join(&f)).ok(),
            "{f}"
        );
    }
    let names = files_under(&dir.join("c"));
    let joined: String = names
        .iter()
        .map(|f| fs::read_to_string(dir.join("c").join(f)).expect("script"))
        .collect();
    assert!(joined.contains("\"paths\":[\"/main?thing=two%20%26%20three\"]"));
    assert_eq!(
        names
            .iter()
            .filter(|n| n.ends_with(".marked.jsonl"))
            .count(),
        20
    );
    assert_eq!(names.len(), 40);
    let mut args = vec![
        "split".to_owned(),
        "--sides".to_owned(),
        "seen,unseen".to_owned(),
        "--seed".to_owned(),
        "5".to_owned(),
        "--out".to_owned(),
        s(&dir.join("split")),
    ];
    args.extend(scripts(&dir.join("c")));
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = run(&a);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines[0], "groups 2");
    assert!(lines[1].starts_with("side seen streams=10 "));
    assert!(lines[2].starts_with("side unseen streams=10 "));
    for (side, tier) in [("seen", "gen-dev"), ("unseen", "gen-unseen")] {
        let mut args = vec![
            "build".to_owned(),
            "--tier".to_owned(),
            tier.to_owned(),
            "--seed".to_owned(),
            "5".to_owned(),
            "--out".to_owned(),
            s(&dir.join(tier)),
        ];
        args.extend(scripts(&dir.join("split").join(side)));
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let o = run(&a);
        assert_eq!(o.code, 0, "{}", o.stderr);
        let c = run(&["check-labels", &s(&dir.join(tier))]);
        assert_eq!(c.code, 0, "{}", c.stdout);
        let m = TierManifest::parse(&fs::read(dir.join(tier).join("manifest.toml")).expect("m"))
            .expect("parse");
        assert_eq!(m.counts.streams, 10);
        assert!(m.counts.planted.iter().all(|n| *n == 2));
        assert!(m.counts.twice.iter().all(|n| *n >= 1));
        assert!(m.counts.marked.iter().sum::<u64>() == 10);
        assert!(m
            .counts
            .near_miss
            .iter()
            .zip(m.counts.planted)
            .all(|(n, p)| 4 * n >= p));
        assert!(m.counts.one_off.iter().sum::<u64>() == 10);
        assert_eq!(m.counts.non_seed, 10);
    }
}

#[test]
fn ranges_prints_fifteen_lines() {
    let o = run(&["compose", "--ranges"]);
    assert_eq!(o.code, 0);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines.len(), 15);
    assert!(lines[0].starts_with("range interleave 1 cut_per_mille="));
    assert!(lines[2].contains("stretch_min_ms=") && lines[2].contains("stretch_max_ms="));
    assert_eq!(lines[10], "range near_miss 1 near_miss_per_mille=250");
    assert!(lines.iter().all(|l| l.split(' ').skip(3).all(|kv| kv
        .split_once('=')
        .is_some_and(|(_, v)| v.parse::<u64>().is_ok()))));
    refused(&run(&["compose", "--ranges", "--ranges"]), "usage");
    refused(&run(&["compose", "--ranges", "x.json"]), "usage");
}

#[test]
fn rates_refusals() {
    let dir = scratch("rates");
    let sites = services(&dir.join("sites"));
    let good = rates(20, 4).write();
    for bad in [
        good.replace("streams = 20", "streams = 21"),
        good.replace("planted = 4", "planted = 3"),
        good.replace("streams = 20", "streams = 0"),
        good.replace("interleave = 3", "interleave = 6"),
        good.replace("noise = 3", "noise = 0"),
        good.replace("near_miss = 1", "near_miss = 03"),
        good.replace("day1 = 1767225600000000000", "day1 = 1767225600000000001"),
        good.replace("canaries = []", "canaries = [\"a.test\"]"),
        good.replace("seed = 5", "seed  = 5"),
        good.replace("sides = \"seen,unseen\"", "sides = \"seen,heldout\""),
        good.replace("browser = \"org.chromium.Chromium\"", "browser = \"a/b\""),
        format!("{good}\n"),
        good.replace("seed = 5\n", ""),
        format!("# rates\n{good}"),
    ] {
        refused(&compose(&dir, &bad, &sites, "o"), "rates");
    }
    assert!(!dir.join("o").exists());
}

#[test]
fn compose_refusals() {
    let dir = scratch("refusals");
    let sites = services(&dir.join("sites"));
    refused(&run(&["compose", "--out", "x"]), "usage");
    refused(&compose(&dir, &rates(20, 4).write(), &[], "o"), "usage");
    let missing = vec![dir.join("none.json")];
    refused(
        &compose(&dir, &rates(20, 4).write(), &missing, "o"),
        "site-data",
    );
    let mut twice = sites.clone();
    twice.push(sites[0].clone());
    refused(
        &compose(&dir, &rates(20, 4).write(), &twice, "o"),
        "site-data",
    );
    let mut canary = rates(20, 4);
    canary.canaries = vec![
        "zz-1.test".to_owned(),
        "zz-21.test".to_owned(),
        "zz-22.test".to_owned(),
    ];
    refused(&compose(&dir, &canary.write(), &sites, "o"), "site-data");
    refused(&compose(&dir, &rates(20, 40).write(), &sites, "o"), "short");
    let o = compose(&dir, &rates(20, 4).write(), &sites, "o");
    assert_eq!(o.code, 0);
    refused(&compose(&dir, &rates(20, 4).write(), &sites, "o"), "exists");
}

#[test]
fn canaries_go_into_every_stream_and_no_episode() {
    let dir = scratch("canaries");
    let sites = services(&dir.join("sites"));
    let mut r = rates(20, 4);
    r.sides = (Sites::Check, Sites::Heldout);
    r.canaries = vec!["zz-21.test".to_owned(), "zz-22.test".to_owned()];
    refused(&compose(&dir, &r.write(), &sites, "bad"), "rates");
    let extra = dir.join("sites/zz-99.json");
    let mut svc: Service =
        halvo_sample::site_data::parse_service(&fs::read(&sites[20]).expect("other"))
            .expect("parse");
    svc.service = "zz-99".to_owned();
    svc.host = Some("zz-99.test".to_owned());
    svc.pages[0].title = "Home thrush99".to_owned();
    svc.pages[1].title = "More thrush99".to_owned();
    write(&extra, &svc.write());
    let mut all = sites.clone();
    all.push(extra);
    r.canaries = vec![
        "zz-21.test".to_owned(),
        "zz-22.test".to_owned(),
        "zz-99.test".to_owned(),
    ];
    let o = compose(&dir, &r.write(), &all, "c");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let mut with_all = 0;
    for f in files_under(&dir.join("c"))
        .into_iter()
        .filter(|f| !f.ends_with(".marked.jsonl"))
    {
        let text = fs::read_to_string(dir.join("c").join(&f)).expect("script");
        let header = text.lines().next().expect("header");
        assert!(header.contains("\"canaries\":[\"zz-21.test\",\"zz-22.test\",\"zz-99.test\"]"));
        let noise_sites = ["zz-21.test", "zz-22.test", "zz-99.test"]
            .iter()
            .filter(|c| text.contains(&format!("\"site\":\"{c}\"")))
            .count();
        if noise_sites == 3 {
            with_all += 1;
        }
    }
    assert_eq!(with_all, 20);
}

/// One fixture: its name, its files and whether the reader takes them.
type Case = (String, Vec<Vec<u8>>, bool);

/// The reader's own fixtures of tools/fixture-server/sites.test.mjs, each with the verdict that its test
/// asserts: true where the reader takes the file.
fn pairs() -> Vec<Case> {
    let shop = r#"{"v":1,"service":"acorn-shop","category":"shop_order","kind":"web","host":"acorn-shop.test","app":null,"parameters":["item","size"],"values":{"item":["blue cup","tea & honey"],"size":["small"]},"pages":[{"page":"sign-in","title":"Sign in - Acorn","text":"Enter your password.","password_input":true,"card_title":false},{"page":"item","title":"{item} - Acorn","text":"One {item}, size {size}.","password_input":false,"card_title":false},{"page":"pay","title":"Pay 4242 4242 4242 4242 - Acorn","text":"Paying.","password_input":false,"card_title":true},{"page":"browse","title":"Browse - Acorn","text":"All items.","password_input":false,"card_title":false}],"flows":[{"flow":"buy","role":"task","pages":["sign-in","item","pay"]},{"flow":"look","role":"near_miss","pages":["sign-in","browse"]}]}"#;
    let native = r#"{"v":1,"service":"acorn-sheet","category":"spreadsheet_fill","kind":"native","host":null,"app":"org.libreoffice.script","parameters":["sheet"],"values":{"sheet":["budget"]},"pages":[{"page":"book","title":"{sheet}.ods - LibreOffice Calc","text":"Rows.","password_input":false,"card_title":false}],"flows":[{"flow":"fill","role":"task","pages":["book"]},{"flow":"read","role":"near_miss","pages":["book"]}]}"#;
    let other = r#"{"v":1,"service":"acorn-news","category":"other","kind":"web","host":"acorn-news.test","app":null,"parameters":[],"values":{},"pages":[{"page":"home","title":"News - Acorn","text":"Stories.","password_input":false,"card_title":false}],"flows":[{"flow":"read","role":"other","pages":["home"]}]}"#;
    let j = |s: &str| serde_json::to_string(s).expect("json");
    let mut out: Vec<Case> = Vec::new();
    let mut add = |name: &str, text: String, taken: bool| {
        out.push((
            name.to_owned(),
            vec![format!("{text}\n").into_bytes()],
            taken,
        ))
    };
    let r = |base: &str, from: &str, to: &str| -> String {
        assert!(base.contains(from), "{from}");
        base.replacen(from, to, 1)
    };
    add("shop", shop.to_owned(), true);
    add("native", native.to_owned(), true);
    add("other", other.to_owned(), true);
    add("space", r(shop, "\"v\":1,", "\"v\":1, "), false);
    add("pretty", shop.replace(",\"", ",\n  \""), false);
    add("key-twice", r(shop, "\"v\":1,", "\"v\":1,\"v\":1,"), false);
    out.push(("no-lf".to_owned(), vec![shop.as_bytes().to_vec()], false));
    out.push((
        "crlf".to_owned(),
        vec![format!("{shop}\r\n").into_bytes()],
        false,
    ));
    out.push((
        "second-line".to_owned(),
        vec![format!("{shop}\n\n").into_bytes()],
        false,
    ));
    out.push((
        "bom".to_owned(),
        vec![[&[0xEF, 0xBB, 0xBF][..], format!("{shop}\n").as_bytes()].concat()],
        false,
    ));
    let mut add = |name: &str, text: String, taken: bool| {
        out.push((
            name.to_owned(),
            vec![format!("{text}\n").into_bytes()],
            taken,
        ))
    };
    add(
        "escape",
        r(shop, "\"Browse - Acorn\"", "\"Br\\u006fwse - Acorn\""),
        false,
    );
    add(
        "slash",
        r(shop, "\"All items.\"", "\"All\\/items.\""),
        false,
    );
    add("number", r(shop, "\"v\":1,", "\"v\":1.0,"), false);
    add(
        "surrogate",
        r(shop, "\"Browse - Acorn\"", "\"Browse \\ud800 - Acorn\""),
        false,
    );
    add("not-json", "{\"v\":1,".to_owned(), false);
    add("array", "[]".to_owned(), false);
    add("null", "null".to_owned(), false);
    add(
        "swapped",
        r(
            shop,
            "{\"v\":1,\"service\":\"acorn-shop\",",
            "{\"service\":\"acorn-shop\",\"v\":1,",
        ),
        false,
    );
    add("missing-key", r(shop, "\"app\":null,", ""), false);
    add(
        "extra-key",
        format!("{},\"extra\":1}}", &shop[..shop.len() - 1]),
        false,
    );
    add(
        "page-key-order",
        r(
            shop,
            "{\"page\":\"browse\",\"title\":\"Browse - Acorn\",",
            "{\"title\":\"Browse - Acorn\",\"page\":\"browse\",",
        ),
        false,
    );
    add(
        "flow-key-order",
        r(
            shop,
            "{\"flow\":\"look\",\"role\":\"near_miss\",\"pages\":[\"sign-in\",\"browse\"]}",
            "{\"flow\":\"look\",\"pages\":[\"sign-in\",\"browse\"],\"role\":\"near_miss\"}",
        ),
        false,
    );
    add(
        "page-extra",
        r(
            shop,
            "\"card_title\":false}",
            "\"card_title\":false,\"extra\":true}",
        ),
        false,
    );
    add("v2", r(shop, "\"v\":1,", "\"v\":2,"), false);
    add("v-string", r(shop, "\"v\":1,", "\"v\":\"1\","), false);
    for (k, name) in [
        "",
        "Acorn-shop",
        "-acorn",
        "acorn-",
        "acorn_shop",
        "acorn.shop",
        &"a".repeat(33),
    ]
    .iter()
    .enumerate()
    {
        add(
            &format!("service-{k}"),
            other.replace("acorn-news", name),
            false,
        );
    }
    add(
        "service-32",
        other.replace("acorn-news", &"a".repeat(32)),
        true,
    );
    add(
        "category-upper",
        r(other, "\"other\",\"kind\"", "\"Other\",\"kind\""),
        false,
    );
    add(
        "category-travel",
        r(other, "\"other\",\"kind\"", "\"travel\",\"kind\""),
        false,
    );
    add(
        "kind-app",
        r(other, "\"kind\":\"web\"", "\"kind\":\"app\""),
        false,
    );
    add(
        "host-other",
        r(
            other,
            "\"host\":\"acorn-news.test\"",
            "\"host\":\"other.test\"",
        ),
        false,
    );
    add(
        "host-dot",
        r(
            other,
            "\"host\":\"acorn-news.test\"",
            "\"host\":\"acorn-news.test.\"",
        ),
        false,
    );
    add(
        "host-null",
        r(other, "\"host\":\"acorn-news.test\"", "\"host\":null"),
        false,
    );
    add(
        "web-app",
        r(other, "\"app\":null", "\"app\":\"org.example\""),
        false,
    );
    add(
        "native-host",
        r(native, "\"host\":null", "\"host\":\"acorn-sheet.test\""),
        false,
    );
    add(
        "native-no-app",
        r(native, "\"app\":\"org.libreoffice.script\"", "\"app\":null"),
        false,
    );
    for (k, app) in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "a\u{7}b",
        "a\u{7f}b",
        "a\u{9b}b",
        &"x".repeat(256),
    ]
    .iter()
    .enumerate()
    {
        add(
            &format!("app-{k}"),
            r(
                native,
                "\"app\":\"org.libreoffice.script\"",
                &format!("\"app\":{}", j(app)),
            ),
            false,
        );
    }
    add(
        "app-soffice",
        r(
            native,
            "\"app\":\"org.libreoffice.script\"",
            "\"app\":\"soffice.bin\"",
        ),
        true,
    );
    for (k, name) in ["", "Item", "1item", "it-em", &"i".repeat(33)]
        .iter()
        .enumerate()
    {
        let t = r(shop, "\"parameters\":[\"item\",\"size\"],\"values\":{\"item\":[\"blue cup\",\"tea & honey\"],\"size\":[\"small\"]}", &format!("\"parameters\":[{0},\"size\"],\"values\":{{{0}:[\"blue cup\"],\"size\":[\"small\"]}}", j(name)));
        add(
            &format!("parameter-{k}"),
            r(
                &t,
                "\"title\":\"{item} - Acorn\"",
                &format!("\"title\":{}", j(&format!("{{{name}}} - Acorn"))),
            ),
            false,
        );
    }
    add(
        "parameter-twice",
        r(
            shop,
            "\"parameters\":[\"item\",\"size\"]",
            "\"parameters\":[\"item\",\"item\"]",
        ),
        false,
    );
    let values = "\"values\":{\"item\":[\"blue cup\",\"tea & honey\"],\"size\":[\"small\"]}";
    add(
        "values-order",
        r(
            shop,
            values,
            "\"values\":{\"size\":[\"small\"],\"item\":[\"blue cup\"]}",
        ),
        false,
    );
    add(
        "values-short",
        r(shop, values, "\"values\":{\"item\":[\"blue cup\"]}"),
        false,
    );
    add(
        "values-extra",
        r(
            shop,
            values,
            "\"values\":{\"item\":[\"blue cup\"],\"size\":[\"small\"],\"extra\":[\"x\"]}",
        ),
        false,
    );
    add(
        "values-empty",
        r(shop, "\"size\":[\"small\"]", "\"size\":[]"),
        false,
    );
    add(
        "values-twice",
        r(
            shop,
            "\"size\":[\"small\"]",
            "\"size\":[\"small\",\"small\"]",
        ),
        false,
    );
    for (k, v) in ["", &"x".repeat(65), "a\tb", "a\u{1f}b", "a\u{7f}b"]
        .iter()
        .enumerate()
    {
        add(
            &format!("value-{k}"),
            r(
                shop,
                "\"size\":[\"small\"]",
                &format!("\"size\":[{}]", j(v)),
            ),
            false,
        );
    }
    add(
        "value-number",
        r(shop, "\"size\":[\"small\"]", "\"size\":[7]"),
        false,
    );
    add(
        "value-32-e",
        r(
            shop,
            "\"size\":[\"small\"]",
            &format!("\"size\":[{}]", j(&"é".repeat(32))),
        ),
        true,
    );
    add(
        "value-33-e",
        r(
            shop,
            "\"size\":[\"small\"]",
            &format!("\"size\":[{}]", j(&format!("{}x", "é".repeat(32)))),
        ),
        false,
    );
    add(
        "value-lone",
        r(shop, "\"size\":[\"small\"]", "\"size\":[\"\\ud800\"]"),
        false,
    );
    add(
        "value-nul",
        r(shop, "\"size\":[\"small\"]", "\"size\":[\"a\\u0000b\"]"),
        false,
    );
    add(
        "value-low",
        r(shop, "\"size\":[\"small\"]", "\"size\":[\"\\udc00x\"]"),
        false,
    );
    add(
        "app-number",
        r(native, "\"app\":\"org.libreoffice.script\"", "\"app\":5"),
        false,
    );
    add(
        "card-in-text",
        r(
            shop,
            "\"title\":\"Pay 4242 4242 4242 4242 - Acorn\",\"text\":\"Paying.\"",
            "\"title\":\"Pay - Acorn\",\"text\":\"Card 4242 4242 4242 4242.\"",
        ),
        false,
    );
    add("no-page", r(other, "\"pages\":[{\"page\":\"home\",\"title\":\"News - Acorn\",\"text\":\"Stories.\",\"password_input\":false,\"card_title\":false}],\"flows\":[{\"flow\":\"read\",\"role\":\"other\",\"pages\":[\"home\"]}]", "\"pages\":[],\"flows\":[]"), false);
    add(
        "page-twice",
        r(shop, "{\"page\":\"browse\"", "{\"page\":\"item\""),
        false,
    );
    add(
        "flow-twice",
        r(shop, "{\"flow\":\"look\"", "{\"flow\":\"buy\""),
        false,
    );
    for (k, id) in ["", "Browse", "browse_all", &"b".repeat(33), "api"]
        .iter()
        .enumerate()
    {
        let t = r(
            shop,
            "{\"page\":\"browse\"",
            &format!("{{\"page\":{}", j(id)),
        );
        add(
            &format!("page-id-{k}"),
            r(
                &t,
                "\"pages\":[\"sign-in\",\"browse\"]",
                &format!("\"pages\":[\"sign-in\",{}]", j(id)),
            ),
            false,
        );
        if *id != "api" {
            add(
                &format!("flow-id-{k}"),
                r(shop, "{\"flow\":\"look\"", &format!("{{\"flow\":{}", j(id))),
                false,
            );
        }
    }
    add(
        "d4-title",
        r(shop, "\"Browse - Acorn\"", "\"Browse {colour} - Acorn\""),
        false,
    );
    add(
        "d4-text",
        r(shop, "\"All items.\"", "\"All {colour} items.\""),
        false,
    );
    for (k, t) in [
        "Browse {} - Acorn",
        "Browse {Item} - Acorn",
        "Browse { item } - Acorn",
        &format!("Browse {{{}}} - Acorn", "a".repeat(33)),
    ]
    .iter()
    .enumerate()
    {
        add(
            &format!("braces-{k}"),
            r(shop, "\"Browse - Acorn\"", &j(t)),
            true,
        );
    }
    add(
        "flow-no-page",
        r(shop, "\"pages\":[\"sign-in\",\"browse\"]", "\"pages\":[]"),
        false,
    );
    add(
        "flow-unknown-page",
        r(
            shop,
            "\"pages\":[\"sign-in\",\"browse\"]",
            "\"pages\":[\"sign-in\",\"cart\"]",
        ),
        false,
    );
    add(
        "flow-repeats",
        r(
            shop,
            "\"pages\":[\"sign-in\",\"browse\"]",
            "\"pages\":[\"sign-in\",\"browse\",\"browse\",\"item\"]",
        ),
        true,
    );
    add(
        "role",
        r(shop, "\"role\":\"near_miss\"", "\"role\":\"near-miss\""),
        false,
    );
    add(
        "d6-two-tasks",
        r(shop, "\"role\":\"near_miss\"", "\"role\":\"task\""),
        false,
    );
    add(
        "d6-two-near",
        r(shop, "\"role\":\"task\"", "\"role\":\"near_miss\""),
        false,
    );
    add(
        "d6-other-flow",
        format!(
            "{},{{\"flow\":\"idle\",\"role\":\"other\",\"pages\":[\"browse\"]}}]}}",
            &shop[..shop.len() - 2]
        ),
        false,
    );
    add(
        "d6-native-pop",
        r(
            native,
            ",{\"flow\":\"read\",\"role\":\"near_miss\",\"pages\":[\"book\"]}",
            "",
        ),
        false,
    );
    add(
        "d6-other-task",
        r(other, "\"role\":\"other\"", "\"role\":\"task\""),
        false,
    );
    add(
        "d6-other-none",
        r(
            other,
            "\"flows\":[{\"flow\":\"read\",\"role\":\"other\",\"pages\":[\"home\"]}]",
            "\"flows\":[]",
        ),
        false,
    );
    add(
        "d7-no-password",
        r(shop, "\"password_input\":true", "\"password_input\":false"),
        false,
    );
    add(
        "d7-two-password",
        r(
            shop,
            "\"All items.\",\"password_input\":false",
            "\"All items.\",\"password_input\":true",
        ),
        false,
    );
    add(
        "d7-password-second",
        r(
            shop,
            "\"pages\":[\"sign-in\",\"item\",\"pay\"]",
            "\"pages\":[\"item\",\"sign-in\",\"pay\"]",
        ),
        false,
    );
    add(
        "d7-no-card",
        r(
            shop,
            "\"Paying.\",\"password_input\":false,\"card_title\":true",
            "\"Paying.\",\"password_input\":false,\"card_title\":false",
        ),
        false,
    );
    add("d7-two-cards", r(shop, "\"title\":\"Browse - Acorn\",\"text\":\"All items.\",\"password_input\":false,\"card_title\":false", "\"title\":\"Card 4242 4242 4242 4242\",\"text\":\"All items.\",\"password_input\":false,\"card_title\":true"), false);
    add(
        "d7-card-not-in-task",
        r(
            &r(
                shop,
                "\"pages\":[\"sign-in\",\"item\",\"pay\"]",
                "\"pages\":[\"sign-in\",\"item\"]",
            ),
            "\"pages\":[\"sign-in\",\"browse\"]",
            "\"pages\":[\"sign-in\",\"pay\"]",
        ),
        false,
    );
    add(
        "d7-card-number",
        r(
            shop,
            "Pay 4242 4242 4242 4242 - Acorn",
            "Pay 4242424242424242 - Acorn",
        ),
        false,
    );
    add(
        "d7-other-card",
        r(other, "\"card_title\":false", "\"card_title\":true"),
        false,
    );
    add(
        "d7-other-card-ok",
        r(
            &r(other, "\"card_title\":false", "\"card_title\":true"),
            "\"News - Acorn\"",
            "\"Card 4242 4242 4242 4242\"",
        ),
        true,
    );
    for name in ["category", "site", "trigger", "recurrence"] {
        let t = r(
            shop,
            values,
            &format!(
                "\"values\":{{\"item\":[\"blue cup\",\"tea & honey\"],\"{name}\":[\"small\"]}}"
            ),
        );
        let t = r(
            &t,
            "\"parameters\":[\"item\",\"size\"]",
            &format!("\"parameters\":[\"item\",\"{name}\"]"),
        );
        add(
            &format!("d9-{name}"),
            r(
                &t,
                "\"One {item}, size {size}.\"",
                &format!("\"One {{item}}, {name} {{{name}}}.\""),
            ),
            false,
        );
    }
    add(
        "d9-other",
        r(
            &r(
                other,
                "\"parameters\":[],\"values\":{}",
                "\"parameters\":[\"topic\"],\"values\":{\"topic\":[\"local\"]}",
            ),
            "\"News - Acorn\"",
            "\"News {topic} - Acorn\"",
        ),
        false,
    );
    for (k, t) in [" Browse - Acorn", "Browse - Acorn ", "Browse  - Acorn"]
        .iter()
        .enumerate()
    {
        add(
            &format!("d10-title-{k}"),
            r(shop, "\"Browse - Acorn\"", &j(t)),
            false,
        );
    }
    for (k, v) in [" small", "small ", "very  small"].iter().enumerate() {
        add(
            &format!("d10-value-{k}"),
            r(
                shop,
                "\"size\":[\"small\"]",
                &format!("\"size\":[{}]", j(v)),
            ),
            false,
        );
    }
    add(
        "d10-text",
        r(
            shop,
            "\"All items.\"",
            "\" two  spaces in text are allowed \"",
        ),
        true,
    );
    add(
        "unused",
        r(
            &r(
                shop,
                "\"parameters\":[\"item\",\"size\"]",
                "\"parameters\":[\"item\",\"size\",\"colour\"]",
            ),
            "\"size\":[\"small\"]}",
            "\"size\":[\"small\"],\"colour\":[\"red\"]}",
        ),
        false,
    );
    add(
        "type-password",
        r(shop, "\"password_input\":true", "\"password_input\":1"),
        false,
    );
    add(
        "type-title",
        r(shop, "\"title\":\"Sign in - Acorn\"", "\"title\":null"),
        false,
    );
    add(
        "type-parameters",
        r(
            shop,
            "\"parameters\":[\"item\",\"size\"]",
            "\"parameters\":\"item\"",
        ),
        false,
    );
    add("type-values", r(shop, values, "\"values\":[]"), false);
    add(
        "type-pages",
        format!(
            "{}\"pages\":{{}},\"flows\"{}",
            &shop[..shop.find("\"pages\":[").expect("pages")],
            &shop[shop.find(",\"flows\"").expect("flows") + 8..]
        ),
        false,
    );
    add(
        "type-flow-pages",
        r(
            shop,
            "\"pages\":[\"sign-in\",\"item\",\"pay\"]",
            "\"pages\":\"sign-in\"",
        ),
        false,
    );
    add("type-page", r(shop, "{\"page\":\"sign-in\",\"title\":\"Sign in - Acorn\",\"text\":\"Enter your password.\",\"password_input\":true,\"card_title\":false}", "\"sign-in\""), false);
    out.push((
        "d8-two-files".to_owned(),
        vec![
            format!("{other}\n").into_bytes(),
            format!("{}\n", other.replace("News - Acorn", "Other news - Acorn")).into_bytes(),
        ],
        false,
    ));
    let mut bad = format!("{shop}\n").into_bytes();
    let at = bad.windows(6).position(|w| w == b"Browse").expect("Browse");
    bad[at] = 0xFF;
    out.push(("not-utf8".to_owned(), vec![bad], false));
    out
}

fn node_takes(files: &[PathBuf]) -> bool {
    let checker = common::repo().join("tools/fixture-server/check-sites.mjs");
    let o = std::process::Command::new("node")
        .arg(&checker)
        .args(files)
        .output()
        .expect("node runs: the pairs test needs Node, which both CI jobs install");
    match o.status.code() {
        Some(0) => true,
        Some(2) => {
            assert!(String::from_utf8_lossy(&o.stderr).starts_with("REFUSED site-data: "));
            false
        }
        other => panic!("check-sites.mjs exited {other:?}"),
    }
}

#[test]
fn compose_takes_and_refuses_each_fixture_of_the_reader_as_the_reader_does() {
    let dir = scratch("pairs");
    write(&dir.join("rates.toml"), &rates(20, 4).write());
    let cases = pairs();
    assert_eq!(cases.len(), 135);
    let verdicts: Vec<(bool, bool)> = std::thread::scope(|scope| {
        let chunks: Vec<Vec<(usize, &Case)>> = (0..4)
            .map(|t| {
                cases
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| k % 4 == t)
                    .collect()
            })
            .collect();
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                let dir = dir.clone();
                scope.spawn(move || {
                    chunk
                        .into_iter()
                        .map(|(k, c)| (k, verdict(&dir, k, c)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut all: Vec<(usize, (bool, bool))> = handles
            .into_iter()
            .flat_map(|h| h.join().expect("thread"))
            .collect();
        all.sort_by_key(|(k, _)| *k);
        all.into_iter().map(|(_, v)| v).collect()
    });
    let (mut taken, mut refused_n) = (0, 0);
    for ((name, _, expect), (by_node, by_compose)) in cases.iter().zip(verdicts) {
        assert_eq!(by_node, *expect, "the reader on {name}");
        assert_eq!(by_compose, *expect, "compose on {name}");
        if *expect {
            taken += 1;
        } else {
            refused_n += 1;
        }
    }
    println!("pairs taken={taken} refused={refused_n}");
}

/// The verdicts of the reader's command and of compose on one case: true where each takes the files.
fn verdict(dir: &Path, k: usize, (_, files, _): &Case) -> (bool, bool) {
    {
        let paths: Vec<PathBuf> = files
            .iter()
            .enumerate()
            .map(|(i, bytes)| {
                let p = dir.join(format!("case-{k}-{i}.json"));
                fs::write(&p, bytes).expect("write");
                p
            })
            .collect();
        let by_node = node_takes(&paths);
        let mut args = vec![
            "compose".to_owned(),
            "--rates".to_owned(),
            s(&dir.join("rates.toml")),
            "--out".to_owned(),
            s(&dir.join(format!("out-{k}"))),
        ];
        args.extend(paths.iter().map(|p| s(p)));
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let o = run(&a);
        (by_node, compose_takes(&o, k))
    }
}

#[test]
fn compose_refuses_a_title_that_breaks_d12_and_d13() {
    let dir = scratch("d12");
    let other = r#"{"v":1,"service":"acorn-news","category":"other","kind":"web","host":"acorn-news.test","app":null,"parameters":[],"values":{},"pages":[{"page":"home","title":"News - Acorn","text":"Stories.","password_input":false,"card_title":false}],"flows":[{"flow":"read","role":"other","pages":["home"]}]}"#;
    for (k, title) in ["", &"x".repeat(1025), "News\u{1}Acorn", "News\u{7f}Acorn"]
        .iter()
        .enumerate()
    {
        let p = dir.join(format!("t{k}.json"));
        write(
            &p,
            &format!(
                "{}\n",
                other.replace(
                    "\"News - Acorn\"",
                    &serde_json::to_string(title).expect("j")
                )
            ),
        );
        refused(
            &compose(&dir, &rates(20, 4).write(), &[p], "o"),
            "site-data",
        );
    }
    let p = dir.join("long-ok.json");
    write(
        &p,
        &format!("{}\n", other.replace("News - Acorn", &"x".repeat(1024))),
    );
    refused(&compose(&dir, &rates(20, 4).write(), &[p], "o"), "short");
}

#[test]
fn a_template_on_both_halves_is_short_with_its_category() {
    let dir = scratch("shared-template");
    let sites = services(&dir.join("sites"));
    let p = dir.join("sites/zz-10.json");
    let text = fs::read_to_string(&p).expect("service");
    write(
        &p,
        &text
            .replace("quill10", "quill9")
            .replace("Look quill9 1", "Look quill9 0"),
    );
    let o = compose(&dir, &rates(20, 4).write(), &sites, "o");
    refused(&o, "short");
    assert_eq!(o.stderr, "REFUSED short: email\n");
}

#[test]
fn a_native_twice_task_takes_the_other_service_of_its_app_out_of_its_stream() {
    let dir = scratch("twice-native");
    let mut sites = services(&dir.join("sites"));
    for (i, p) in sites.clone().iter().take(20).enumerate() {
        let mut svc =
            halvo_sample::site_data::parse_service(&fs::read(p).expect("service")).expect("parse");
        svc.service = format!("zz-dup-{i}");
        if svc.web {
            svc.host = Some(format!("zz-dup-{i}.test"));
        }
        for page in &mut svc.pages {
            page.title = page.title.replace("quill", "grebe");
        }
        let q = dir.join(format!("sites/zz-dup-{i}.json"));
        write(&q, &svc.write());
        sites.push(q);
    }
    let o = compose(&dir, &rates(20, 26).write(), &sites, "full");
    refused(&o, "short");
    assert_eq!(o.stderr, "REFUSED short: spreadsheet_fill\n");
    let o = compose(&dir, &rates(20, 24).write(), &sites, "room");
    assert_eq!(o.code, 0, "{}", o.stderr);
}

#[test]
fn ranges_takes_no_other_flag_and_no_other_argument() {
    let dir = scratch("ranges-alone");
    write(&dir.join("rates.toml"), &rates(20, 4).write());
    let rates_file = s(&dir.join("rates.toml"));
    let out = s(&dir.join("o"));
    for args in [
        vec!["compose", "--ranges", "--out", out.as_str()],
        vec!["compose", "--ranges", "--rates", rates_file.as_str()],
        vec!["compose", "--rates", rates_file.as_str(), "--ranges"],
    ] {
        let o = run(&args);
        refused(&o, "usage");
        assert_eq!(o.stderr, "REFUSED usage: --ranges\n");
    }
    assert!(!dir.join("o").exists());
    let o = run(&["compose", "--ranges"]);
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout.lines().count(), 15);
    assert!(o.stderr.is_empty());
}

fn compose_takes(o: &common::Out, k: usize) -> bool {
    let one_line = o.stderr.lines().count() == 1;
    match o.code {
        0 => true,
        2 if one_line && o.stderr.starts_with("REFUSED short: ") => true,
        2 if one_line && o.stderr.starts_with("REFUSED site-data: ") => false,
        code => panic!("compose on case {k}: exit {code}, {}", o.stderr),
    }
}
