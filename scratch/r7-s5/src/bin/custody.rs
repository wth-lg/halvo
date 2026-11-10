//! S5: one build stores a synthetic item, another build reads it.

use r7_s5_spike::{exit_after, keystore, require_hosted_runner, BUILD_STAMP};
use std::process::ExitCode;
use std::time::Duration;

const ITEM: &str = "custody";
const READ_FAILED: u8 = 10;
const WRONG_VALUE: u8 = 11;
const NO_ANSWER: i32 = 12;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if args == ["stamp"] {
        println!("{BUILD_STAMP}");
        return ExitCode::SUCCESS;
    }
    require_hosted_runner();
    exit_after(Duration::from_secs(60), NO_ANSWER);

    match args.as_slice() {
        ["store", value] => match keystore::store(ITEM, value.as_bytes()) {
            Ok(()) => {
                println!("S5 build {BUILD_STAMP} store: ok ({})", keystore::BACKEND);
                ExitCode::SUCCESS
            }
            Err(error) => {
                println!("S5 build {BUILD_STAMP} store: {error}");
                ExitCode::FAILURE
            }
        },
        ["read", expected] => match keystore::read(ITEM) {
            Ok(value) if value == expected.as_bytes() => {
                println!(
                    "S5 build {BUILD_STAMP} read: ok, the stored value came back with user interaction disabled"
                );
                ExitCode::SUCCESS
            }
            Ok(_) => {
                println!("S5 build {BUILD_STAMP} read: a different value came back");
                ExitCode::from(WRONG_VALUE)
            }
            Err(error) => {
                let kind = if error.not_found {
                    "item not found"
                } else {
                    "locked credential"
                };
                println!("S5 build {BUILD_STAMP} read: {kind}: {error}");
                ExitCode::from(READ_FAILED)
            }
        },
        ["delete"] => match keystore::delete(ITEM) {
            Ok(()) => {
                println!("S5 build {BUILD_STAMP} delete: ok");
                ExitCode::SUCCESS
            }
            Err(error) => {
                println!("S5 build {BUILD_STAMP} delete: {error}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: custody stamp | store <value> | read <expected> | delete");
            ExitCode::from(2)
        }
    }
}
