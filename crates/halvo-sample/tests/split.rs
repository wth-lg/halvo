mod common;

use std::fs;
use std::path::Path;

use common::{episode, item, refused, run, s, scratch, step_line, write, HEADER, SEC};

/// A script file of one stream: a planted routine on its own site, and a one-off on `shared` when given.
fn stream_file(dir: &Path, id: &str, own: &str, shared: Option<&str>, title: &str) -> String {
    let mut lines = vec![
        HEADER.to_owned(),
        format!(r#"{{"line":"stream","stream":"{id}","sites":"seen"}}"#),
        step_line(&format!("{id}-a"), own, title, false),
    ];
    for day in 1..=3 {
        lines.push(episode(
            id,
            &format!("{id}-e{day}"),
            &format!("{id}-t1"),
            "email",
            "planted",
            day,
            &[item(&format!("{id}-a"), 0, 30 * SEC, 0, 1, 0)],
        ));
    }
    if let Some(site) = shared {
        lines.push(step_line(
            &format!("{id}-b"),
            site,
            &format!("Shared {id}"),
            false,
        ));
        lines.push(episode(
            id,
            &format!("{id}-e9"),
            &format!("{id}-t2"),
            "ride_order",
            "one_off",
            9,
            &[item(&format!("{id}-b"), 0, 30 * SEC, 0, 0, 0)],
        ));
    }
    let p = dir.join(format!("{id}.jsonl"));
    write(
        &p,
        &lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    );
    s(&p)
}

fn split(dir: &Path, seed: &str, out: &str, files: &[String]) -> common::Out {
    let mut args = vec!["split", "--sides", "seen,unseen", "--seed", seed, "--out"];
    let o = s(&dir.join(out));
    args.push(&o);
    args.extend(files.iter().map(String::as_str));
    run(&args)
}

#[test]
fn whole_groups_go_to_one_side_and_the_split_is_repeatable() {
    let dir = scratch("split");
    let files = vec![
        stream_file(&dir, "a1", "one-a.test", Some("hub-a.test"), "One A"),
        stream_file(&dir, "a2", "two-a.test", Some("hub-a.test"), "Two A"),
        stream_file(
            &dir,
            "b1",
            "one-b.test",
            Some("hub-b.test"),
            "Weekly report",
        ),
        stream_file(&dir, "b2", "two-b.test", None, "Weekly report"),
    ];
    let o = split(&dir, "3", "x", &files);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines[0], "groups 2");
    let again = split(&dir, "3", "y", &files);
    assert_eq!(again.stdout, o.stdout);
    for side in ["seen", "unseen"] {
        for f in ["a1", "a2", "b1", "b2"] {
            let rel = format!("{side}/{f}.jsonl");
            assert_eq!(
                fs::read(dir.join("x").join(&rel)).ok(),
                fs::read(dir.join("y").join(&rel)).ok()
            );
        }
    }
    let seen_a1 = fs::read_to_string(dir.join("x/seen/a1.jsonl")).expect("a1");
    let seen_a2 = fs::read_to_string(dir.join("x/seen/a2.jsonl")).expect("a2");
    assert_eq!(
        seen_a1.lines().count() > 1,
        seen_a2.lines().count() > 1,
        "a1 and a2 share a site"
    );
    let seen_b1 = fs::read_to_string(dir.join("x/seen/b1.jsonl")).expect("b1");
    let seen_b2 = fs::read_to_string(dir.join("x/seen/b2.jsonl")).expect("b2");
    assert_eq!(
        seen_b1.lines().count() > 1,
        seen_b2.lines().count() > 1,
        "b1 and b2 share a template"
    );
    assert_ne!(seen_a1.lines().count() > 1, seen_b1.lines().count() > 1);
    let unseen_side = if seen_a1.lines().count() > 1 {
        "b1"
    } else {
        "a1"
    };
    let moved =
        fs::read_to_string(dir.join(format!("x/unseen/{unseen_side}.jsonl"))).expect("moved");
    assert!(moved.starts_with(&format!("{HEADER}\n")));
    assert!(moved.contains("\"sites\":\"unseen\""));
    assert!(!moved.contains("\"sites\":\"seen\""));
    let header_only =
        fs::read_to_string(dir.join(format!("x/seen/{unseen_side}.jsonl"))).expect("header");
    assert_eq!(header_only, format!("{HEADER}\n"));
    let side_lines: Vec<&str> = lines[1..].to_vec();
    assert!(
        side_lines[0].starts_with("side seen streams=2 sites=3 templates=")
            || side_lines[0].starts_with("side seen streams=2 sites=2 templates="),
        "{}",
        side_lines[0]
    );
}

#[test]
fn split_refusals() {
    let dir = scratch("split-refusals");
    let one = stream_file(&dir, "a1", "one-a.test", Some("hub-a.test"), "One A");
    let two = stream_file(&dir, "a2", "two-a.test", Some("hub-a.test"), "Two A");
    refused(
        &split(&dir, "3", "o", &[one.clone(), two.clone()]),
        "no-split",
    );
    refused(&split(&dir, "x", "o", std::slice::from_ref(&one)), "usage");
    refused(
        &run(&[
            "split",
            "--sides",
            "seen,check",
            "--seed",
            "1",
            "--out",
            "o",
            &one,
        ]),
        "usage",
    );
    refused(
        &run(&[
            "split",
            "--sides",
            "seen,unseen",
            "--seed",
            "1",
            "--out",
            "o",
        ]),
        "usage",
    );
    let other = dir.join("again");
    fs::create_dir_all(&other).expect("dir");
    let copy = other.join("a1.jsonl");
    fs::copy(&one, &copy).expect("copy");
    refused(&split(&dir, "3", "o", &[one.clone(), s(&copy)]), "script");
    refused(
        &split(&dir, "3", "o", &[s(&dir.join("none.jsonl"))]),
        "script",
    );
    let three = stream_file(&dir, "b1", "one-b.test", None, "Weekly report");
    let four = stream_file(&dir, "b2", "two-b.test", None, "Weekly report");
    fs::create_dir_all(dir.join("taken")).expect("dir");
    refused(
        &split(&dir, "3", "taken", &[one, two, three, four]),
        "exists",
    );
    assert!(!dir.join("o").exists());
}

const CANARY_HEADER: &str =
    r#"{"line":"script","v":1,"day1":1767225600000000000,"canaries":["cn-one.test"]}"#;

fn canary_stream(dir: &Path, id: &str, own: &str, title: &str) -> String {
    let mut lines = vec![
        CANARY_HEADER.to_owned(),
        format!(r#"{{"line":"stream","stream":"{id}","sites":"seen"}}"#),
        step_line(&format!("{id}-a"), own, title, false),
        step_line("cn-home", "cn-one.test", "Front page", false),
    ];
    for day in 1..=3 {
        lines.push(episode(
            id,
            &format!("{id}-e{day}"),
            &format!("{id}-t1"),
            "email",
            "planted",
            day,
            &[item(&format!("{id}-a"), 0, 30 * SEC, 0, 1, 0)],
        ));
    }
    lines.push(format!(
        r#"{{"line":"noise","stream":"{id}","day":9,"item":{}}}"#,
        item("cn-home", 0, 30 * SEC, 0, 1, 0)
    ));
    let p = dir.join(format!("{id}.jsonl"));
    write(
        &p,
        &lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    );
    s(&p)
}

#[test]
fn a_canary_site_links_no_streams_and_counts_in_no_side() {
    let dir = scratch("split-canary");
    let files = vec![
        canary_stream(&dir, "a1", "one-a.test", "Morning list"),
        canary_stream(&dir, "b1", "one-b.test", "Evening chart"),
    ];
    let o = split(&dir, "3", "x", &files);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "groups 2\nside seen streams=1 sites=1 templates=1\nside unseen streams=1 sites=1 templates=1\n"
    );
}

#[test]
fn the_check_and_heldout_pair_names_its_two_sides() {
    let dir = scratch("split-sealed");
    let files = [
        stream_file(&dir, "a1", "one-a.test", Some("hub-a.test"), "One A"),
        stream_file(&dir, "a2", "two-a.test", Some("hub-a.test"), "Two A"),
        stream_file(
            &dir,
            "b1",
            "one-b.test",
            Some("hub-b.test"),
            "Weekly report",
        ),
        stream_file(&dir, "b2", "two-b.test", None, "Weekly report"),
    ];
    let mut args = vec!["split", "--sides", "check,heldout", "--seed", "3", "--out"];
    let out = s(&dir.join("x"));
    args.push(&out);
    args.extend(files.iter().map(String::as_str));
    let o = run(&args);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines[0], "groups 2");
    assert!(
        lines[1].starts_with("side check streams=2 "),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("side heldout streams=2 "),
        "{}",
        lines[2]
    );
    let mut stream_lines = Vec::new();
    for side in ["check", "heldout"] {
        for f in ["a1", "a2", "b1", "b2"] {
            let text = fs::read_to_string(dir.join("x").join(side).join(format!("{f}.jsonl")))
                .expect("side file");
            for l in text
                .lines()
                .filter(|l| l.starts_with(r#"{"line":"stream""#))
            {
                assert!(l.ends_with(&format!(r#""sites":"{side}"}}"#)), "{l}");
                stream_lines.push(l.to_owned());
            }
        }
    }
    assert_eq!(stream_lines.len(), 4);
}

fn linked_stream(
    dir: &Path,
    id: &str,
    step: (&str, &str, &str),
    noise: Option<(&str, &str, &str)>,
) -> String {
    let (step_id, site, title) = step;
    let mut lines = vec![
        HEADER.to_owned(),
        format!(r#"{{"line":"stream","stream":"{id}","sites":"seen"}}"#),
        step_line(step_id, site, title, false),
    ];
    for day in 1..=3 {
        lines.push(episode(
            id,
            &format!("{id}-e{day}"),
            &format!("{id}-t1"),
            "email",
            "planted",
            day,
            &[item(step_id, 0, 30 * SEC, 0, 1, 0)],
        ));
    }
    if let Some((noise_id, noise_site, noise_title)) = noise {
        lines.push(step_line(noise_id, noise_site, noise_title, false));
        lines.push(format!(
            r#"{{"line":"noise","stream":"{id}","day":9,"item":{}}}"#,
            item(noise_id, 0, 30 * SEC, 0, 1, 0)
        ));
    }
    let p = dir.join(format!("{id}.jsonl"));
    write(
        &p,
        &lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    );
    s(&p)
}

fn sides_of(dir: &Path, files: &[String], streams: &[&str]) -> Vec<String> {
    let o = split(dir, "3", "x", files);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout.lines().next(), Some("groups 2"), "{}", o.stdout);
    streams
        .iter()
        .map(|id| {
            let seen = fs::read_to_string(dir.join("x/seen").join(format!("{id}.jsonl")))
                .expect("seen file");
            if seen.lines().count() > 1 {
                "seen".to_owned()
            } else {
                "unseen".to_owned()
            }
        })
        .collect()
}

#[test]
fn two_streams_that_share_a_site_through_episode_items_alone_are_one_group() {
    let dir = scratch("link-episode-site");
    let files = [
        linked_stream(&dir, "x1", ("x1-a", "hub-x.test", "Alpha ledger"), None),
        linked_stream(&dir, "x2", ("x2-a", "hub-x.test", "Beta folder"), None),
        linked_stream(&dir, "y1", ("y1-a", "far-one.test", "Gamma board"), None),
        linked_stream(&dir, "y2", ("y2-a", "far-two.test", "Gamma board"), None),
    ];
    let sides = sides_of(&dir, &files, &["x1", "x2", "y1", "y2"]);
    assert_eq!(sides[0], sides[1]);
    assert_eq!(sides[2], sides[3]);
    assert_ne!(sides[0], sides[2]);
}

#[test]
fn two_streams_that_share_only_the_site_of_a_noise_item_are_one_group() {
    let dir = scratch("link-noise-site");
    let noise = Some(("zz-news", "news-z.test", "Daily digest"));
    let files = [
        linked_stream(&dir, "z1", ("z1-a", "own-one.test", "Alpha ledger"), noise),
        linked_stream(&dir, "z2", ("z2-a", "own-two.test", "Beta folder"), noise),
        linked_stream(&dir, "y1", ("y1-a", "far-one.test", "Gamma board"), None),
        linked_stream(&dir, "y2", ("y2-a", "far-two.test", "Gamma board"), None),
    ];
    let sides = sides_of(&dir, &files, &["z1", "z2", "y1", "y2"]);
    assert_eq!(sides[0], sides[1]);
    assert_eq!(sides[2], sides[3]);
    assert_ne!(sides[0], sides[2]);
}
