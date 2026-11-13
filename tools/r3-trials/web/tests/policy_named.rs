//! CP-145 (6): the page reader's third form, through the trial program's `--policy-named NAME FILE
//! VALUE_READ`, on the kept text of Edge's policy page on the Windows runner (run 43384012622,
//! `edge-RemoteDebuggingAllowed.txt`, line breaks made LF) and on the controls of (6) (c), which
//! read the same before and after the form.

use std::path::PathBuf;

/// The kept text of repeat 3: the name as `Remot eDebuggingAllow e`, its last letter joined to the
/// value on the next line (`dfalse Platform Device Mandator`).
const KEPT: &str = include_str!("data/edge_win_rda_page.txt");
const NAME_LINES: [&str; 3] = [
    "Remot eDebuggingAllow e",
    "dfalse Platform Device Mandator",
    "yOK",
];

fn read(tag: &str, text: &str, name: &str, value: &str) -> String {
    use r3_web::start::Start;
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("policy-named");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{tag}.txt"));
    std::fs::write(&file, text).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_web-trial"))
        .args(["--policy-named", name])
        .arg(&file)
        .arg(value)
        .stdin(std::process::Stdio::null())
        .start_output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The kept text with its three lines of the setting replaced by `rows`.
fn with_rows(rows: &str) -> String {
    let joined = NAME_LINES.join("\n");
    assert!(
        KEPT.contains(&joined),
        "the kept text no longer holds the setting's lines"
    );
    KEPT.replace(&joined, rows)
}

#[test]
fn the_kept_edge_page_names_the_setting_by_the_value_that_ends_it() {
    let line = read("kept", KEPT, "RemoteDebuggingAllowed", "0");
    assert!(line.contains("named=yes"), "{line}");
    let crlf = read(
        "kept-crlf",
        &KEPT.replace('\n', "\r\n"),
        "RemoteDebuggingAllowed",
        "0",
    );
    assert!(crlf.contains("named=yes"), "with CR LF line breaks: {crlf}");
}

#[test]
fn the_controls_of_the_third_form_read_not_named() {
    let name = "RemoteDebuggingAllowed";
    let cases = [
        ("without_the_row", with_rows("Other row"), "0"),
        (
            "longer_whole",
            with_rows("RemoteDebuggingAllowedOrigins false Platform Device Mandatory\nOK"),
            "0",
        ),
        (
            "longer_broken",
            with_rows("RemoteDebuggingAllowed\nOrigins false Platform Device Mandatory\nOK"),
            "0",
        ),
        (
            "longer_split_joined",
            with_rows("Remot eDebuggingAllow edOrigin\nsfalse Platform Device Mandator\nyOK"),
            "0",
        ),
        (
            "another_value",
            with_rows("Remot eDebuggingAllow e\ndtrue Platform Device Mandator\nyOK"),
            "0",
        ),
        (
            "value_not_the_end_of_its_word",
            with_rows("Remot eDebuggingAllow e\ndfalsely Platform Device Mandator\nyOK"),
            "0",
        ),
        (
            "not_at_a_line_start",
            with_rows("OtherPolicy Remot eDebuggingAllow e\ndfalse Platform Device Mandator\nyOK"),
            "0",
        ),
        (
            "a_mark_between_letters",
            with_rows("Remot-eDebuggingAllow e\ndfalse Platform Device Mandator\nyOK"),
            "0",
        ),
        ("a_value_the_job_did_not_write", KEPT.to_string(), "1"),
    ];
    let mut named = Vec::new();
    for (tag, text, value) in &cases {
        let line = read(tag, text, name, value);
        if !line.contains("named=no") {
            named.push(format!("{tag}: {line}"));
        }
    }
    assert!(named.is_empty(), "controls that read named: {named:?}");
}

#[test]
fn the_kept_proxy_page_reads_as_before() {
    let text = include_str!("data/edge_win_proxy_page.txt");
    let with = read("proxy", text, "ProxyMode", "direct");
    let without = read("proxy-no-value", text, "ProxyMode", "");
    assert!(with.contains("named=yes how=whole"), "{with}");
    assert_eq!(with, without);
}
