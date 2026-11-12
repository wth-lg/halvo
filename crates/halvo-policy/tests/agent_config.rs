use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    match Command::new(env!("CARGO_BIN_EXE_agent-config"))
        .args(args)
        .output()
    {
        Ok(output) => output,
        Err(e) => panic!("agent-config did not start: {e}"),
    }
}

fn text(bytes: &[u8]) -> String {
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(e) => panic!("not UTF-8: {e}"),
    }
}

fn file(name: &str, content: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("halvo-policy-agent-config");
    if let Err(e) = fs::create_dir_all(&dir) {
        panic!("cannot make {}: {e}", dir.display());
    }
    let path = dir.join(name);
    if let Err(e) = fs::write(&path, content) {
        panic!("cannot write {}: {e}", path.display());
    }
    path
}

fn path_arg(path: &std::path::Path) -> &str {
    match path.to_str() {
        Some(s) => s,
        None => panic!("not UTF-8: {}", path.display()),
    }
}

const PRESETS: &str = "\
preset category=utility_bill_payment runtime=web autonomy=draft_and_confirm result=valid
preset category=tenant_rent runtime=none autonomy=observe result=valid
preset category=lease_renewal_or_change runtime=none autonomy=observe result=valid
preset category=tenant_application_review runtime=none autonomy=observe result=valid
preset category=email runtime=graph_mail autonomy=draft_and_confirm result=valid
preset category=spreadsheet_fill runtime=file autonomy=draft_and_confirm result=valid
preset category=shop_order runtime=web autonomy=draft_and_confirm result=valid
preset category=calendar_cleanup runtime=graph_calendar autonomy=draft_and_confirm result=valid
preset category=ride_order runtime=none autonomy=observe result=valid
preset category=daily_chart_check runtime=none autonomy=observe result=valid
presets seeds=10 valid=10 unattended=0
";

#[test]
fn presets_prints_ten_valid_lines_and_the_count_and_exits_0() {
    let out = run(&["presets"]);
    assert_eq!(text(&out.stdout), PRESETS);
    assert_eq!(text(&out.stderr), "");
    assert_eq!(out.status.code(), Some(0));
}

fn config(tier: &str, allow: &str) -> String {
    format!(
        r#"{{"v":1,"workflow":"30000000000000000000000000000001","agent":"40000000000000000000000000000001","rev":1,"category":"email","tier":"{tier}","reasoning":"medium","autonomy":"draft_and_confirm","allow":[{allow}],"caps":{{"untrusted":"yes","private":"yes","external":"yes"}},"limits":{{"steps":20,"wall_s":120,"model_tokens":0}},"budget":null,"permissions":[],"runtime":"graph_mail","playbook":null}}"#
    )
}

fn validate(name: &str, content: &str) -> (String, String, Option<i32>) {
    let path = file(name, content);
    let out = run(&["validate", path_arg(&path)]);
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

#[test]
fn validate_prints_valid_and_exits_0_for_a_valid_record() {
    let line = format!("{}\n", config("on_device", ""));
    assert_eq!(
        validate("valid.json", &line),
        ("config result=valid\n".to_owned(), String::new(), Some(0))
    );
}

#[test]
fn validate_prints_the_cause_and_exits_1() {
    let line = format!("{}\n", config("frontier", ""));
    assert_eq!(
        validate("tier.json", &line),
        ("config result=tier\n".to_owned(), String::new(), Some(1))
    );
    let line = format!("{}\n", config("on_device", r#""send""#));
    assert_eq!(
        validate("allow.json", &line),
        ("config result=allow\n".to_owned(), String::new(), Some(1))
    );
}

#[test]
fn validate_prints_malformed_for_a_record_that_the_parser_refuses() {
    for (name, content) in [
        ("payment.json", config("on_device", r#""payment""#)),
        ("twice.json", config("on_device", r#""send","send""#)),
        ("text.json", "not a record\n".to_owned()),
        ("empty.json", String::new()),
    ] {
        assert_eq!(
            validate(name, &content),
            (
                "config result=malformed\n".to_owned(),
                String::new(),
                Some(1)
            ),
            "{name}"
        );
    }
}

#[test]
fn a_missing_file_is_refused_with_file_and_exit_2() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("halvo-policy-agent-config");
    let missing = dir.join("no-such-record.json");
    let out = run(&["validate", path_arg(&missing)]);
    assert_eq!(text(&out.stdout), "");
    let err = text(&out.stderr);
    assert!(err.starts_with("REFUSED file: "), "{err}");
    assert_eq!(err.lines().count(), 1);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn bad_usage_is_refused_with_usage_and_exit_2() {
    for args in [
        &[][..],
        &["validate"][..],
        &["validate", "a", "b"][..],
        &["presets", "extra"][..],
        &["preset"][..],
        &["--help"][..],
    ] {
        let out = run(args);
        assert_eq!(text(&out.stdout), "", "{args:?}");
        let err = text(&out.stderr);
        assert!(err.starts_with("REFUSED usage: "), "{args:?}: {err}");
        assert_eq!(err.lines().count(), 1, "{args:?}");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}
