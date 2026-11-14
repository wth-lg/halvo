//! The library in a process of its own, so that a test can end that process by the system's hard kill and read its
//! two streams: `host DATA_DIR SCRIPT OWNER_LOG [HOLD_DIR]` starts one run whose runner is this binary with SCRIPT,
//! prints `started <run id>`, answers every act with yes, logs each request that reaches its owner as one line of
//! OWNER_LOG, and prints `end <RunEnd as Debug>` when the run has ended. With HOLD_DIR, `Confirmer::answers` prints
//! `answers-in`, makes the file HOLD_DIR/asked, waits until the file HOLD_DIR/release exists or 60 seconds have
//! passed, and prints `answers-out` before it answers.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use halvo_agent_client::{
    Config, Confirmer, Human, IssueRefused, LinkError, MarkSink, OriginAnswer, OwnerLink, Parts,
    RunEnd, RunSpec, Secrets, Supervisor,
};
use halvo_schema::executable::frame::{
    ConfirmDecision, ConfirmRequest, HandbackRequest, SecretReply,
};
use halvo_schema::executable::names::{Answer, Mode};
use halvo_schema::executable::text::{Name, RunId, WebOrigin};
use halvo_schema::request::{decode, CoordinatorRole, Decoded};
use halvo_schema::{AgentId, WorkflowId};

struct Owner(Mutex<std::fs::File>);

impl OwnerLink for Owner {
    fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
        let mut line = request.to_vec();
        line.push(b'\n');
        if let Ok(mut f) = self.0.lock() {
            let _ = f.write_all(&line);
        }
        let Ok(Decoded::Raw(raw)) = decode(request) else {
            return Err(LinkError);
        };
        let body = raw.body().get();
        let ok = if body.contains(r#""op":"add_origin""#) {
            r#"{"rev":2}"#
        } else if body.contains(r#""op":"end_run""#) {
            r#"{"ok":"yes"}"#
        } else {
            r#"{"stored":1}"#
        };
        Ok(format!(r#"{{"v":1,"id":"{}","ok":{ok}}}"#, raw.id().as_str()).into_bytes())
    }
}

struct Yes {
    hold: Option<PathBuf>,
}

const HOLD_MAX: Duration = Duration::from_secs(60);

fn say(text: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
}

fn hold_until_released(dir: &Path) {
    say("answers-in");
    let _ = std::fs::write(dir.join("asked"), b"");
    let deadline = std::time::Instant::now() + HOLD_MAX;
    while !dir.join("release").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    say("answers-out");
}

impl Confirmer for Yes {
    fn answers(&self, _run: &RunId, request: &ConfirmRequest) -> Vec<Answer> {
        if let Some(dir) = &self.hold {
            hold_until_released(dir);
        }
        vec![Answer::AllowOnce; request.effects.len()]
    }

    fn issued(&self, _run: &RunId, _decision: &ConfirmDecision) -> Result<(), IssueRefused> {
        Ok(())
    }
}

impl Human for Yes {
    fn handback(&self, _run: &RunId, _request: &HandbackRequest) {}

    fn allow_origin(&self, _run: &RunId, _origin: &WebOrigin) -> OriginAnswer {
        OriginAnswer::Allow
    }

    fn progress(&self, _run: &RunId, _step: NonZeroU32, _of: NonZeroU32) {}
}

impl Secrets for Yes {
    fn read(&self, _name: &Name) -> SecretReply {
        SecretReply::None
    }
}

impl MarkSink for Yes {
    fn start(&self, _run: &RunId) {}

    fn end(&self, _run: &RunId, end: &RunEnd) {
        say(&format!("end {end:?}"));
    }
}

fn id16(salt: u8) -> [u8; 16] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut b = [salt; 16];
    b[..4].copy_from_slice(&std::process::id().to_be_bytes());
    b[4..].copy_from_slice(&nanos.to_be_bytes()[4..]);
    b
}

pub fn run(args: &[String]) {
    let (data, script, owner_log, hold) = match args {
        [data, script, owner_log] => (data, script, owner_log, None),
        [data, script, owner_log, hold] => (data, script, owner_log, Some(PathBuf::from(hold))),
        _ => std::process::exit(64),
    };
    let Ok(log) = OpenOptions::new().create(true).append(true).open(owner_log) else {
        std::process::exit(70);
    };
    let Ok(me) = std::env::current_exe() else {
        std::process::exit(70);
    };
    let yes = Arc::new(Yes { hold });
    let parts = Parts {
        link: Arc::new(Owner(Mutex::new(log))),
        confirmer: yes.clone(),
        human: yes.clone(),
        secrets: yes.clone(),
        marks: yes,
    };
    let sup = Supervisor::new(
        CoordinatorRole::claim(),
        Config::new(PathBuf::from(data), me),
        parts,
    );
    let spec = RunSpec {
        run: RunId::from_random(id16(1)),
        agent: AgentId::from_random(id16(2)),
        workflow: WorkflowId::from_random(id16(3)),
        mode: Mode::Replay,
        browser: PathBuf::from(script),
        model: None,
        headed: false,
    };
    let run = spec.run.clone();
    match sup.start(spec) {
        Ok(()) => say(&format!("started {run}")),
        Err(e) => say(&format!("refused {e}")),
    }
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
