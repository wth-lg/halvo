use std::{fs, path::PathBuf};

struct Example {
    info: String,
    body: Vec<String>,
    item: String,
}

fn request_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/request.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn doc_text(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("///")?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

fn examples(source: &str) -> Vec<Example> {
    let lines: Vec<&str> = source.lines().map(str::trim_end).collect();
    let mut found: Vec<Example> = Vec::new();
    let mut open: Option<Example> = None;
    let mut block_start = 0;
    let mut in_doc = false;
    for (at, line) in lines.iter().enumerate() {
        let Some(text) = doc_text(line) else {
            if in_doc {
                let item = lines[at..]
                    .iter()
                    .map(|line| line.trim())
                    .find(|line| !line.is_empty() && !line.starts_with("#["))
                    .unwrap_or("")
                    .to_string();
                for example in &mut found[block_start..] {
                    example.item = item.clone();
                }
                in_doc = false;
            }
            continue;
        };
        if !in_doc {
            in_doc = true;
            block_start = found.len();
        }
        match open.take() {
            Some(example) if text == "```" => found.push(example),
            Some(mut example) => {
                example.body.push(text.to_string());
                open = Some(example);
            }
            None => {
                if let Some(info) = text.strip_prefix("```") {
                    open = Some(Example {
                        info: info.to_string(),
                        body: Vec::new(),
                        item: String::new(),
                    });
                }
            }
        }
    }
    assert!(
        open.is_none(),
        "a doc example of request.rs has no closing fence"
    );
    found
}

fn assert_case(marker: &str, info: &str) {
    let source = request_source();
    let all = examples(&source);
    let cases: Vec<&Example> = all
        .iter()
        .filter(|example| example.body.iter().any(|line| line.contains(marker)))
        .collect();
    assert_eq!(
        cases.len(),
        1,
        "request.rs holds {} doc examples with the line {marker:?}, not one",
        cases.len()
    );
    assert_eq!(
        cases[0].info, info,
        "the doc example with the line {marker:?} is fenced as ```{}, not ```{info}",
        cases[0].info
    );
    assert!(
        cases[0].item.starts_with("pub struct Request"),
        "the doc example with the line {marker:?} documents {:?}, not pub struct Request",
        cases[0].item
    );
}

#[test]
fn the_default_case_is_a_compile_fail_example_of_request_with_e0277() {
    assert_case(
        "made_by_default::<halvo_wire::request::Request>();",
        "compile_fail,E0277",
    );
}

#[test]
fn the_from_case_is_a_compile_fail_example_of_request_with_e0308() {
    assert_case(".or(holds!(TryFrom<Call>))", "compile_fail,E0308");
}

#[test]
fn the_deserialize_case_is_a_compile_fail_example_of_request_with_e0277() {
    assert_case(
        "made_by_deserialize::<halvo_wire::request::Request>();",
        "compile_fail,E0277",
    );
}

#[test]
fn the_mutable_self_case_is_a_compile_fail_example_of_request_with_e0308() {
    assert_case("let added: Added = named!(", "compile_fail,E0308");
}
