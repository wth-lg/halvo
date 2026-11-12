mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{corpus, locks, Scratch, RUN};
use halvo_sheet::{Folder, Range, RunId, Value};

fn marked(name: &str, mark: impl Fn(&Path)) -> (Scratch, PathBuf) {
    let scratch = Scratch::new(name);
    let workbook = scratch.path("values.xlsx");
    fs::copy(corpus("values.xlsx"), &workbook).expect("copy");
    mark(&workbook);
    (scratch, workbook)
}

fn made(scratch: &Scratch, workbook: &Path) -> [PathBuf; 3] {
    let folder = Folder::new(&scratch.dir, locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let range = Range::parse("B5").expect("a range");
    let values = [vec![Value::Number(77.0)]];
    let prepared = folder
        .prepare(workbook, &run, "Values", range, &values)
        .expect("prepared");
    let checkpoint = prepared.checkpoint().to_path_buf();
    let copy = prepared.copy().to_path_buf();
    folder.replace(&prepared).expect("replaced");
    [checkpoint, copy, workbook.to_path_buf()]
}

fn run_tool(program: &str, args: &[&std::ffi::OsStr]) -> String {
    let out = Command::new(program)
        .args(args)
        .output()
        .expect("the tool runs");
    assert!(out.status.success(), "{program}: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[cfg(target_os = "macos")]
#[test]
fn the_workbooks_extended_attributes_and_access_list_go_to_the_checkpoint_and_the_copy() {
    let attribute = "com.example.halvo-carried";
    let quarantine = "com.apple.quarantine";
    let (scratch, workbook) = marked("carry-mac", |workbook| {
        let file = workbook.as_os_str();
        run_tool(
            "xattr",
            &["-w".as_ref(), attribute.as_ref(), "kept".as_ref(), file],
        );
        let flag = "0081;00000000;Halvo test;";
        run_tool(
            "xattr",
            &["-w".as_ref(), quarantine.as_ref(), flag.as_ref(), file],
        );
        run_tool(
            "chmod",
            &["+a".as_ref(), "everyone allow readextattr".as_ref(), file],
        );
    });
    let [checkpoint, copy, replaced] = made(&scratch, &workbook);
    for (label, file) in [("checkpoint", &checkpoint), ("workbook", &replaced)] {
        let file = file.as_os_str();
        let value = run_tool("xattr", &["-p".as_ref(), attribute.as_ref(), file]);
        assert_eq!(value.trim(), "kept", "{label}");
        let flag = run_tool("xattr", &["-p".as_ref(), quarantine.as_ref(), file]);
        let flags = flag.split(';').next();
        assert_eq!(flags, Some("0081"), "{label}: still quarantined: {flag}");
        let listed = run_tool("ls", &["-led".as_ref(), file]);
        assert!(
            listed.contains("everyone allow readextattr"),
            "{label}: the access list"
        );
    }
    assert!(!copy.exists(), "the copy is the workbook now");
}

#[cfg(windows)]
fn descriptor(file: &Path, scratch: &Scratch) -> String {
    let out = scratch.path(&format!(
        "acl-{}.txt",
        file.file_name().expect("a name").to_string_lossy()
    ));
    run_tool(
        "icacls",
        &[file.as_os_str(), "/save".as_ref(), out.as_os_str()],
    );
    let bytes = fs::read(&out).expect("read the saved list");
    fs::remove_file(&out).expect("remove the saved list");
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair.get(1).copied().unwrap_or(0)]))
        .collect();
    let text = String::from_utf16_lossy(&units);
    let line = text
        .lines()
        .find(|line| line.contains("D:"))
        .expect("a descriptor line");
    line.trim().to_string()
}

#[cfg(windows)]
#[test]
fn the_workbooks_named_streams_and_access_list_go_to_the_checkpoint_and_the_copy() {
    let zone = b"[ZoneTransfer]\r\nZoneId=3\r\n";
    let (scratch, workbook) = marked("carry-windows", |workbook| {
        let mut stream = workbook.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        fs::write(&stream, zone).expect("write the stream");
        let mut other = workbook.as_os_str().to_os_string();
        other.push(":halvo.notes");
        fs::write(&other, b"notes").expect("write the stream");
        run_tool(
            "icacls",
            &[
                workbook.as_os_str(),
                "/grant".as_ref(),
                "*S-1-1-0:(R)".as_ref(),
            ],
        );
    });
    let source = descriptor(&workbook, &scratch);
    assert!(
        source.contains(";;;WD)"),
        "the source has the entry: {source}"
    );
    let [checkpoint, copy, replaced] = made(&scratch, &workbook);
    for (label, file) in [("checkpoint", &checkpoint), ("workbook", &replaced)] {
        let mut stream = file.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        assert_eq!(fs::read(&stream).expect("the stream"), zone, "{label}");
        let mut other = file.as_os_str().to_os_string();
        other.push(":halvo.notes");
        assert_eq!(fs::read(&other).expect("the stream"), b"notes", "{label}");
        assert_eq!(
            descriptor(file, &scratch),
            source,
            "{label}: the access list"
        );
    }
    assert!(!copy.exists(), "the copy is the workbook now");
}

#[test]
fn a_workbook_with_nothing_to_carry_is_prepared_as_before() {
    let (scratch, workbook) = marked("carry-none", |_| {});
    let [checkpoint, _, replaced] = made(&scratch, &workbook);
    assert!(checkpoint.exists() && replaced.exists());
}
