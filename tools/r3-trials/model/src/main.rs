mod args;
mod engine;
mod prompt;
mod settings;
mod wire;

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::process::ExitCode;
use std::time::Instant;

use halvo_cap::LockError;

enum Failure {
    Refused(&'static str, String),
    Run(String),
}

fn refused(cause: &'static str) -> impl Fn(String) -> Failure {
    move |detail| Failure::Refused(cause, detail)
}

fn run(argv: &[OsString]) -> Result<(), Failure> {
    engine::quiet_log();
    let args = args::parse(argv)
        .map_err(|e| Failure::Refused("usage", format!("{e}; {}", args::USAGE)))?;
    let text = std::fs::read_to_string(&args.settings)
        .map_err(|e| Failure::Refused("settings", format!("read the settings: {e}")))?;
    let settings = settings::parse(&text).map_err(refused("settings"))?;
    let input: Box<dyn BufRead> = match &args.files {
        Some((requests, _)) => {
            Box::new(BufReader::new(File::open(requests).map_err(|e| {
                Failure::Refused("usage", format!("read the requests: {e}"))
            })?))
        }
        None => Box::new(BufReader::new(std::io::stdin())),
    };
    let lock = match halvo_cap::take_model_lock() {
        Ok(lock) => lock,
        Err(LockError::Held(_)) => return Err(Failure::Refused("lock", "held".to_string())),
        Err(e) => return Err(Failure::Refused("lock", e.to_string())),
    };
    let sizes = engine::tensor_sizes(&args.weights).map_err(refused("weights"))?;
    if let Some(fault) = engine::tensor_fault(&sizes) {
        return Err(Failure::Refused("weights", fault));
    }
    let model_name = args
        .weights
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Failure::Refused("weights", "the weights have no file name".to_string()))?
        .to_string();
    let backend = engine::backend().map_err(Failure::Run)?;
    engine::default_log();
    let start = Instant::now();
    let model = engine::load_model(&backend, &args.weights).map_err(Failure::Run)?;
    let mut context = engine::new_context(&backend, &model, &settings).map_err(Failure::Run)?;
    let load_ms = start.elapsed().as_millis() as u64;
    let mut out: Box<dyn Write> = match &args.files {
        Some((_, replies)) => Box::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(replies)
                .map_err(|e| Failure::Run(format!("open the replies: {e}")))?,
        ),
        None => Box::new(std::io::stdout()),
    };
    let mut emit = |line: String| {
        writeln!(out, "{line}")
            .and_then(|()| out.flush())
            .map_err(|e| Failure::Run(format!("write a line: {e}")))
    };
    emit(wire::ready_line(
        &model_name,
        context.n_ctx(),
        engine::THREADS,
        engine::BATCH,
        load_ms,
    ))?;
    for (n, line) in input.lines().enumerate() {
        let line = line.map_err(|e| Failure::Run(format!("read request line {}: {e}", n + 1)))?;
        let request = wire::parse_request(&line)
            .map_err(|e| Failure::Run(format!("request line {}: {e}", n + 1)))?;
        let reply = engine::answer(&model, &mut context, &settings, &request)
            .map_err(|e| Failure::Run(format!("request {}: {e}", request.id)))?;
        emit(wire::reply_line(&reply))?;
    }
    drop(context);
    drop(model);
    drop(backend);
    drop(lock);
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Refused(cause, detail)) => {
            eprintln!("REFUSED {cause}: {detail}");
            ExitCode::from(2)
        }
        Err(Failure::Run(message)) => {
            eprintln!("r3-model: {message}");
            ExitCode::from(1)
        }
    }
}
