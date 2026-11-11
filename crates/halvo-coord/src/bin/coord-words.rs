use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "coord-words export | coord-words fixtures DIR";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["export"] => {
            let mut out = std::io::stdout().lock();
            match out
                .write_all(halvo_coord::words::export().as_bytes())
                .and_then(|()| out.flush())
            {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("coord-words: standard output could not be written: {e}");
                    ExitCode::from(1)
                }
            }
        }
        ["fixtures", dir] if !dir.is_empty() => {
            match halvo_coord::types::fixtures::write(Path::new(dir)) {
                Ok(n) => {
                    println!("fixtures files={n}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("coord-words: the fixtures could not be written: {e}");
                    ExitCode::from(1)
                }
            }
        }
        _ => {
            eprintln!("REFUSED usage: {USAGE}");
            ExitCode::from(2)
        }
    }
}
