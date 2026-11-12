#![cfg(all(target_os = "macos", debug_assertions))]

use halvo_secrets::{Delete, Directory, Fault, Locked, Name, Read, Secret, Store, Write};
use std::path::Path;
use std::process::Command;

const CANARY: &[u8] = b"print-canary-7f3a9c";
const CHILD: &str = "HALVO_SECRETS_PRINT_CHILD";

fn forms(bytes: &[u8]) -> Vec<String> {
    let lower: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let decimal = bytes
        .iter()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    vec![
        String::from_utf8_lossy(bytes).into_owned(),
        lower.to_uppercase(),
        lower,
        decimal,
        format!("{bytes:?}"),
    ]
}

fn holds_a_form(text: &str) -> Vec<String> {
    forms(CANARY)
        .into_iter()
        .filter(|form| text.contains(form.as_str()))
        .collect()
}

#[test]
fn every_form_is_found_where_it_stands() {
    let all = forms(CANARY);
    assert_eq!(all.len(), 5);
    for form in all {
        let text = format!("before {form} after");
        assert!(holds_a_form(&text).contains(&form), "{form}");
    }
    assert!(holds_a_form("before after").is_empty());
}

#[test]
fn no_answer_prints_the_bytes_of_its_secret() {
    let found = Read::Found(Secret::new(CANARY.to_vec()).expect("a secret"));
    let printed = format!("{found:?} {found:#?}");
    assert_eq!(printed, "Found(..) Found(..)");
    let others = [
        format!("{:?}", Read::NotStored),
        format!("{:?}", Read::Locked(Locked::Refused { code: -25293 })),
        format!("{:?}", Read::Fault(Fault { code: 5 })),
        format!("{:?}", Write::Locked(Locked::Mode)),
        format!("{:?}", Delete::Fault(Fault { code: -1 })),
    ];
    for text in others {
        assert!(holds_a_form(&text).is_empty(), "{text}");
    }
}

#[test]
fn child_prints_every_answer_and_panics_with_a_found_secret() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("halvo-secrets-print-{}", std::process::id()));
    let store = Store::in_directory(
        Directory::new(dir).expect("an absolute directory"),
        Name::new("account").expect("a name"),
    );
    let secret = Secret::new(CANARY.to_vec()).expect("a secret");
    println!("write {:?}", store.write(&secret));
    println!("write again {:?}", store.write(&secret));
    let read = store.read();
    println!("read {read:?}");
    eprintln!("read {read:#?}");
    println!("delete {:?}", store.delete());
    panic!("child ends with {read:?}");
}

#[test]
fn a_run_of_the_store_prints_no_form_of_its_secret_in_its_output_or_its_panic() {
    let output = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "child_prints_every_answer_and_panics_with_a_found_secret",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .expect("a child run");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success());
    for line in [
        "write Stored",
        "write again Occupied",
        "read Found(..)",
        "delete Deleted",
        "child ends with Found(..)",
    ] {
        assert!(text.contains(line), "the child printed no line {line:?}");
    }
    assert_eq!(holds_a_form(&text), Vec::<String>::new());
}
