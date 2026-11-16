mod common;

use std::fs;

use common::{refused, run, s, scratch, write};

#[test]
fn texts_are_masked_then_cut_in_the_order_of_raw() {
    let dir = scratch("mask");
    let long = format!("{} 4242 4242 4242 4242", "é".repeat(39_000));
    let raw = format!(
        "{{\"step\":\"a1\",\"source\":\"render\",\"accessibility_text\":\"card 4242 4242 4242 4242 here\",\"extracted_text\":null}}\n{{\"step\":\"b2\",\"source\":\"render\",\"accessibility_text\":{},\"extracted_text\":\"plain\"}}\n",
        serde_json::to_string(&long).expect("j")
    );
    write(&dir.join("raw.jsonl"), &raw);
    let o = run(&[
        "mask-layers",
        "--out",
        &s(&dir.join("steps.jsonl")),
        &s(&dir.join("raw.jsonl")),
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "masked steps=2 texts_changed=2 texts_cut=1\n");
    let out = fs::read_to_string(dir.join("steps.jsonl")).expect("out");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "{\"step\":\"a1\",\"source\":\"render\",\"accessibility_text\":\"card [card] here\",\"extracted_text\":null}");
    let second: serde_json::Value = serde_json::from_str(lines[1]).expect("json");
    let text = second["accessibility_text"].as_str().expect("text");
    assert_eq!(text.len(), 78_000);
    assert!(!text.contains("4242"));
    assert_eq!(second["extracted_text"], "plain");
}

#[test]
fn mask_layers_refusals() {
    let dir = scratch("mask-refusals");
    let good = "{\"step\":\"a1\",\"source\":\"render\",\"accessibility_text\":null,\"extracted_text\":null}\n";
    write(&dir.join("raw.jsonl"), good);
    refused(&run(&["mask-layers", &s(&dir.join("raw.jsonl"))]), "usage");
    refused(
        &run(&[
            "mask-layers",
            "--out",
            &s(&dir.join("o.jsonl")),
            &s(&dir.join("none.jsonl")),
        ]),
        "raw",
    );
    for bad in [
        good.replace("render", "camera"),
        good.replace("\"step\":\"a1\",", ""),
        format!("{good}{good}"),
        good.replace(",\"source\"", ", \"source\""),
    ] {
        write(&dir.join("bad.jsonl"), &bad);
        refused(
            &run(&[
                "mask-layers",
                "--out",
                &s(&dir.join("o.jsonl")),
                &s(&dir.join("bad.jsonl")),
            ]),
            "raw",
        );
    }
    write(&dir.join("o.jsonl"), "x\n");
    refused(
        &run(&[
            "mask-layers",
            "--out",
            &s(&dir.join("o.jsonl")),
            &s(&dir.join("raw.jsonl")),
        ]),
        "exists",
    );
    refused(
        &run(&[
            "mask-layers",
            "--out",
            &s(&dir.join("no-dir/o.jsonl")),
            &s(&dir.join("raw.jsonl")),
        ]),
        "output",
    );
    assert_eq!(
        fs::read_to_string(dir.join("o.jsonl")).ok().as_deref(),
        Some("x\n")
    );
}
