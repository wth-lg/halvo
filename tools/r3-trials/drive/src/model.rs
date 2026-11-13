use std::ffi::OsString;
use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::thread::JoinHandle;

use halvo_cap::{ChildSpec, Exit, Limit, Outcome};
use serde_json::Value;

/// The cap of A13.1 for every model child of the trial.
pub const LIMIT_BYTES: u64 = 8_589_934_592;

/// How to start the child: `r3-model --weights FILE --settings FILE`, its standard error kept in
/// a file of the run's own directory.
#[derive(Clone, Debug)]
pub struct Spec {
    pub program: PathBuf,
    pub weights: PathBuf,
    pub settings: PathBuf,
    pub stderr: PathBuf,
}

/// One reply line of A12.2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

/// One request of A12.2 as its line, keys in the order of the contract.
pub fn request_line(
    id: u64,
    system: &str,
    prompt: &str,
    snapshot: Option<&str>,
    grammar: &str,
) -> String {
    let s = |t: &str| serde_json::to_string(t).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        "{{\"id\":{id},\"system\":{},\"prompt\":{},\"snapshot\":{},\"grammar\":\"{grammar}\",\"level\":\"medium\"}}",
        s(system),
        s(prompt),
        snapshot.map_or("null".to_string(), s)
    )
}

/// Parses one reply line of A12.2 for the request `id`.
pub fn parse_reply(line: &str, id: u64) -> Result<Reply, String> {
    let v: Value = serde_json::from_str(line).map_err(|_| "the reply is no JSON".to_string())?;
    if v["id"].as_u64() != Some(id) {
        return Err("the reply has another id".to_string());
    }
    let count = |k: &str| v[k].as_u64().ok_or_else(|| format!("the reply has no {k}"));
    Ok(Reply {
        text: v["text"]
            .as_str()
            .ok_or("the reply has no text")?
            .to_string(),
        tokens_in: count("tokens_in")?,
        tokens_out: count("tokens_out")?,
    })
}

/// Why the child gave no reply.
#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    /// The model lock is held by another loader (A12.2: `REFUSED lock: held`).
    LockHeld,
    /// The cap stopped the child (A13.1): the run is a miss.
    Capped,
    /// The child ended or answered out of form; the text says how.
    Child(String),
}

/// A running child under the cap. Dropping it closes the child's standard input, so the child
/// ends at the end of its input (A12.2) even when the trial ends by a panic.
pub struct Child {
    input: Option<PipeWriter>,
    output: BufReader<PipeReader>,
    capped: Option<JoinHandle<Result<Outcome, String>>>,
    stderr: PathBuf,
    next: u64,
}

fn stderr_text(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

impl Child {
    /// Starts the child under the cap and reads its ready line.
    pub fn start(spec: &Spec) -> Result<Child, Fault> {
        let fault = |e: std::io::Error| Fault::Child(e.to_string());
        let (child_in, input) = std::io::pipe().map_err(fault)?;
        let (output, child_out) = std::io::pipe().map_err(fault)?;
        let err = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&spec.stderr)
            .map_err(fault)?;
        let args: Vec<OsString> = vec![
            "--weights".into(),
            spec.weights.clone().into_os_string(),
            "--settings".into(),
            spec.settings.clone().into_os_string(),
        ];
        let mut child = ChildSpec::new(spec.program.clone(), args);
        child.stdin = Stdio::from(child_in);
        child.stdout = Stdio::from(child_out);
        child.stderr = Stdio::from(err);
        let limit = Limit::new(LIMIT_BYTES).ok_or(Fault::Child("limit".into()))?;
        let capped =
            std::thread::spawn(move || halvo_cap::run(child, limit).map_err(|e| e.to_string()));
        let mut started = Child {
            input: Some(input),
            output: BufReader::new(output),
            capped: Some(capped),
            stderr: spec.stderr.clone(),
            next: 1,
        };
        let mut ready = String::new();
        let read = started.output.read_line(&mut ready).map_err(fault)?;
        if read == 0 || !ready.starts_with("ready ") {
            let outcome = started.finish();
            if stderr_text(&started.stderr).contains("REFUSED lock: held") {
                return Err(Fault::LockHeld);
            }
            return Err(match outcome {
                Ok(o) if o.capped => Fault::Capped,
                Ok(o) => Fault::Child(format!("no ready line, exit {:?}", o.exit)),
                Err(e) => Fault::Child(e),
            });
        }
        Ok(started)
    }

    /// Sends one request and reads its reply.
    pub fn ask(
        &mut self,
        system: &str,
        prompt: &str,
        snapshot: Option<&str>,
        grammar: &str,
    ) -> Result<Reply, Fault> {
        let id = self.next;
        self.next += 1;
        let line = request_line(id, system, prompt, snapshot, grammar);
        let sent = self
            .input
            .as_mut()
            .map(|w| writeln!(w, "{line}").and_then(|()| w.flush()));
        let mut reply = String::new();
        let read = match sent {
            Some(Ok(())) => self.output.read_line(&mut reply).unwrap_or(0),
            _ => 0,
        };
        if read == 0 {
            return Err(match self.finish() {
                Ok(o) if o.capped => Fault::Capped,
                Ok(o) => Fault::Child(format!("the child ended, exit {:?}", o.exit)),
                Err(e) => Fault::Child(e),
            });
        }
        parse_reply(reply.trim_end(), id).map_err(Fault::Child)
    }

    /// Closes the child's input and waits for its end under the cap: its outcome.
    pub fn finish(&mut self) -> Result<Outcome, String> {
        self.input.take();
        match self.capped.take() {
            Some(h) => h
                .join()
                .map_err(|_| "the cap thread panicked".to_string())?,
            None => Err("finished before".to_string()),
        }
    }

    /// Ends the run of the child: whether the cap stopped it.
    pub fn end(mut self) -> Result<bool, String> {
        let o = self.finish()?;
        match o.exit {
            Exit::NotStarted => Err("the child did not start".to_string()),
            _ => Ok(o.capped),
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        self.input.take();
        if let Some(h) = self.capped.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_line_has_the_keys_of_a12_2_in_order() {
        assert_eq!(
            request_line(3, "Be brief.", "Pay \"now\".", Some("e1 button \"Pay\"\ne2 text \"x\""), "action"),
            "{\"id\":3,\"system\":\"Be brief.\",\"prompt\":\"Pay \\\"now\\\".\",\"snapshot\":\"e1 button \\\"Pay\\\"\\ne2 text \\\"x\\\"\",\"grammar\":\"action\",\"level\":\"medium\"}"
        );
        assert_eq!(
            request_line(1, "s", "p", None, "text"),
            "{\"id\":1,\"system\":\"s\",\"prompt\":\"p\",\"snapshot\":null,\"grammar\":\"text\",\"level\":\"medium\"}"
        );
    }

    #[test]
    fn a_reply_line_is_read_for_its_id() {
        let line = "{\"id\":2,\"text\":\"{\\\"action\\\":\\\"done\\\"}\",\"tokens_in\":340,\"tokens_out\":31,\"first_token_ms\":10,\"ms\":20}";
        assert_eq!(
            parse_reply(line, 2),
            Ok(Reply {
                text: "{\"action\":\"done\"}".into(),
                tokens_in: 340,
                tokens_out: 31
            })
        );
        assert!(parse_reply(line, 3).is_err());
        assert!(parse_reply("{\"id\":2,\"text\":\"x\"}", 2).is_err());
        assert!(parse_reply("x", 2).is_err());
    }
}
