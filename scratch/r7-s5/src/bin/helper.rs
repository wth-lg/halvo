//! S6: the helper's start. Reads the item it stored in an earlier run, makes one HTTPS
//! request with that item as the bearer, and exits.

use r7_s5_spike::{client, exit_after, keystore, require_hosted_runner};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const ITEM: &str = "helper";
const READ_FAILED: u8 = 20;
const REQUEST_FAILED: u8 = 21;
const BAD_STATUS: u8 = 22;
const NO_ANSWER: i32 = 23;

fn main() -> ExitCode {
    let started = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    require_hosted_runner();
    exit_after(Duration::from_secs(60), NO_ANSWER);

    match args.as_slice() {
        ["store", value] => report("store", keystore::store(ITEM, value.as_bytes())),
        ["delete"] => report("delete", keystore::delete(ITEM)),
        ["run", url, run_id] => run(started, url, run_id),
        _ => {
            eprintln!("usage: helper store <value> | run <url> <run id> | delete");
            ExitCode::from(2)
        }
    }
}

fn report(what: &str, outcome: Result<(), keystore::KeyStoreError>) -> ExitCode {
    match outcome {
        Ok(()) => {
            println!("S6 helper {what}: ok ({})", keystore::BACKEND);
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("S6 helper {what}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(started: Instant, url: &str, run_id: &str) -> ExitCode {
    let item = match keystore::read(ITEM) {
        Ok(item) => item,
        Err(error) => {
            println!("S6 helper: locked credential: {error}");
            return ExitCode::from(READ_FAILED);
        }
    };
    let read = started.elapsed();
    let bearer = String::from_utf8_lossy(&item);

    match client::ping(url, run_id, &bearer, None) {
        Ok(ping) if ping.status == 200 => {
            println!(
                "phases_us read={} client={} request={} total={}",
                read.as_micros(),
                ping.client_build.as_micros(),
                ping.request.as_micros(),
                started.elapsed().as_micros()
            );
            ExitCode::SUCCESS
        }
        Ok(ping) => {
            println!("S6 helper: unexpected status {}", ping.status);
            ExitCode::from(BAD_STATUS)
        }
        Err(error) => {
            println!("S6 helper: request failed: {}", client::error_chain(&error));
            ExitCode::from(REQUEST_FAILED)
        }
    }
}
