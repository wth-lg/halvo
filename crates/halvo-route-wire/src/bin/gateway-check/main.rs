mod files;
mod node;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use halvo_route_wire::{Answer, Body, MAX_BODY_BYTES, MAX_OUTPUT_TOKENS};

use files::{AnswerVector, BodyVector, Refusal};

const BODY_KEYS: [&str; 6] = [
    "tier",
    "effort",
    "max_output_tokens",
    "messages",
    "role",
    "content",
];
const NAMED_KEYS: [&str; 5] = ["effort", "max_output_tokens", "messages", "role", "content"];

struct Args {
    dir: PathBuf,
    names: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(refusal) => {
            eprintln!("REFUSED {refusal}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, Refusal> {
    let args = parse_args(std::env::args_os().skip(1))?;
    let gateway = files::read(&args.dir)?;
    let names = args.names.as_deref().map(files::read_names).transpose()?;

    let mut out = String::new();
    let mut ok = 0;
    for vector in &gateway.vectors {
        if body_holds(vector) {
            ok += 1;
        } else {
            out.push_str(&format!(
                "gateway miss file=vectors n={} name={}\n",
                vector.n, vector.name
            ));
        }
    }
    let mut answers_ok = 0;
    for answer in &gateway.answers {
        if answer_holds(answer) {
            answers_ok += 1;
        } else {
            out.push_str(&format!(
                "gateway miss file=answers n={} name={}\n",
                answer.n, answer.name
            ));
        }
    }

    let measure = files::measure(&gateway.schema);
    let forbidden = names.map(|list| NAMED_KEYS.iter().filter(|key| list.contains(**key)).count());
    let limits = gateway.limits == files::limits_text()
        && measure.maximum == Some(u64::from(MAX_OUTPUT_TOKENS))
        && measure.max_length == u64::try_from(MAX_BODY_BYTES).ok();
    let six: BTreeSet<String> = BODY_KEYS.iter().map(|key| key.to_string()).collect();
    out.push_str(&format!(
        "gateway vectors={} ok={ok} answers={} answers_ok={answers_ok} closed={} keys={} forbidden={} limits={}\n",
        gateway.vectors.len(),
        gateway.answers.len(),
        if measure.closed { "yes" } else { "no" },
        measure.names.len(),
        forbidden.map_or_else(|| "not_checked".to_string(), |count| count.to_string()),
        if limits { "ok" } else { "miss" },
    ));
    let pass = ok == gateway.vectors.len()
        && answers_ok == gateway.answers.len()
        && measure.closed
        && measure.names == six
        && forbidden.unwrap_or(0) == 0
        && limits;

    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return Ok(ExitCode::from(1));
    }
    Ok(if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn parse_args(mut args: impl Iterator<Item = OsString>) -> Result<Args, Refusal> {
    let mut dir = None;
    let mut names = None;
    while let Some(arg) = args.next() {
        let (flag, slot) = match arg.to_str() {
            Some("--dir") => ("--dir", &mut dir),
            Some("--names") => ("--names", &mut names),
            _ => {
                return Err(Refusal::Usage(format!(
                    "unknown argument {}",
                    arg.to_string_lossy()
                )))
            }
        };
        if slot.is_some() {
            return Err(Refusal::Usage(format!("{flag} stands twice")));
        }
        let value = args
            .next()
            .ok_or_else(|| Refusal::Usage(format!("{flag} has no path")))?;
        *slot = Some(PathBuf::from(value));
    }
    let dir = dir.ok_or_else(|| Refusal::Usage("missing --dir".to_string()))?;
    Ok(Args { dir, names })
}

fn body_holds(vector: &BodyVector) -> bool {
    let bytes = vector.body.as_bytes();
    let got = match Body::parse(bytes) {
        Err(error) => Some((false, false, Some(error.as_str()))),
        Ok(read) => match read.to_bytes() {
            Ok(written) if Body::parse(&written).as_ref() == Ok(&read) => {
                Some((true, written == bytes, None))
            }
            _ => None,
        },
    };
    got == Some((vector.valid, vector.canon, vector.error.as_deref()))
}

fn answer_holds(answer: &AnswerVector) -> bool {
    let got = match Answer::parse(answer.status, answer.body.as_bytes()) {
        Ok(Answer::Ok {
            content,
            tokens_in,
            tokens_out,
            cost,
        }) => (
            "ok",
            Some(content),
            Some(tokens_in),
            Some(tokens_out),
            Some(cost),
        ),
        Ok(Answer::Refused(reason)) => (reason.as_str(), None, None, None, None),
        Err(error) => (error.as_str(), None, None, None, None),
    };
    got == (
        answer.expect.as_str(),
        answer.content.clone(),
        answer.tokens_in,
        answer.tokens_out,
        answer.cost,
    )
}
