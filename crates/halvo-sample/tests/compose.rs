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

/// One case of the shared list: its name, its files and whether the reader takes them.
type Case = (String, Vec<Vec<u8>>, bool);

fn from_hex(h: &str) -> Vec<u8> {
    assert!(
        h.len().is_multiple_of(2)
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "hex"
    );
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex"))
        .collect()
}

fn pairs() -> Vec<Case> {
    let text = fs::read_to_string(common::repo().join("tools/fixture-server/site-cases.jsonl"))
        .expect("the shared list of cases");
    let mut bases: std::collections::BTreeMap<String, Vec<u8>> = std::collections::BTreeMap::new();
    let mut out: Vec<Case> = Vec::new();
    for line in text.lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("a line of the list");
        if let Some(t) = v.get("text") {
            bases.insert(
                v["base"].as_str().expect("base").to_owned(),
                format!("{}\n", t.as_str().expect("text")).into_bytes(),
            );
            continue;
        }
        let files = v["files"]
            .as_array()
            .expect("files")
            .iter()
            .map(|f| {
                let base: &[u8] = match f["base"].as_str() {
                    Some(b) => &bases[b],
                    None => &[],
                };
                let head = f["head"].as_u64().expect("head") as usize;
                let tail = f["tail"].as_u64().expect("tail") as usize;
                assert!(head + tail <= base.len(), "head and tail");
                [
                    &base[..head],
                    &from_hex(f["put"].as_str().expect("put"))[..],
                    &base[base.len() - tail..],
                ]
                .concat()
            })
            .collect();
        out.push((
            v["case"].as_str().expect("case").to_owned(),
            files,
            v["taken"].as_bool().expect("taken"),
        ));
    }
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
    let names: std::collections::BTreeSet<&str> = cases.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(
        cases.len(),
        155,
        "the cases of tools/fixture-server/site-cases.jsonl"
    );
    assert_eq!(names.len(), cases.len());
    assert!(cases.iter().any(|c| c.2) && cases.iter().any(|c| !c.2));
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
