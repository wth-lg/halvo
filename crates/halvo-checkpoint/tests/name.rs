mod common;

use std::ffi::OsString;
use std::path::Path;

use common::RUN;
use halvo_checkpoint::{checkpoint_name, own_name, RunId, Sha256, FILE_MAX};

fn run() -> RunId {
    RunId::parse(RUN).expect("a run id")
}

#[test]
fn a_run_id_is_32_lower_case_hex_digits() {
    assert_eq!(run().as_str(), RUN);
    for bad in [
        "",
        "0123456789ABCDEF0123456789abcdef",
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "g123456789abcdef0123456789abcdef",
        "../23456789abcdef0123456789abcdef",
        "0123456789abcdef 123456789abcdef",
    ] {
        assert_eq!(RunId::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn the_checkpoint_name_is_the_workbook_name_with_the_run_id_before_its_extension() {
    let cases = [
        ("values.xlsx", format!("values.halvo-checkpoint-{RUN}.xlsx")),
        (
            "dir/values.xlsx",
            format!("values.halvo-checkpoint-{RUN}.xlsx"),
        ),
        ("a.b.xlsx", format!("a.b.halvo-checkpoint-{RUN}.xlsx")),
        (
            "Budget Q1.XLSX",
            format!("Budget Q1.halvo-checkpoint-{RUN}.XLSX"),
        ),
        ("book", format!("book.halvo-checkpoint-{RUN}")),
        (".xlsx", format!(".xlsx.halvo-checkpoint-{RUN}")),
        (
            "caf\u{e9}.xlsx",
            format!("caf\u{e9}.halvo-checkpoint-{RUN}.xlsx"),
        ),
    ];
    for (workbook, want) in cases {
        let got = checkpoint_name(Path::new(workbook), &run());
        assert_eq!(got, Some(OsString::from(want)), "{workbook}");
    }
    assert_eq!(checkpoint_name(Path::new("/"), &run()), None);
    assert_eq!(checkpoint_name(Path::new(""), &run()), None);
}

#[test]
fn an_own_name_puts_the_tag_between_the_name_and_the_extension() {
    let got = own_name(Path::new("values.xlsx"), "prepared-x");
    assert_eq!(got, Some(OsString::from("values.halvo-prepared-x.xlsx")));
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_keeps_its_bytes() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let workbook = std::ffi::OsStr::from_bytes(b"caf\xe9.xlsx");
    let got = checkpoint_name(Path::new(workbook), &run()).expect("a name");
    let mut want = b"caf\xe9.halvo-checkpoint-".to_vec();
    want.extend_from_slice(RUN.as_bytes());
    want.extend_from_slice(b".xlsx");
    assert_eq!(got.into_vec(), want);
}

#[test]
fn a_digest_is_the_sha256_of_the_bytes_and_its_hex_reads_back() {
    let cases = [
        (
            &b""[..],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            &b"abc"[..],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
    ];
    for (bytes, hex) in cases {
        let digest = Sha256::of(bytes);
        assert_eq!(digest.hex(), hex);
        assert_eq!(Sha256::parse(hex), Some(digest));
        assert_eq!(format!("{digest:?}"), format!("Sha256({hex})"));
    }
    let good = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    for bad in [
        "",
        &good[1..],
        &format!("{good}0"),
        &good.to_ascii_uppercase(),
        &format!("g{}", &good[1..]),
    ] {
        assert_eq!(Sha256::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn the_bound_of_a_file_is_64_mib() {
    assert_eq!(FILE_MAX, 67_108_864);
}
