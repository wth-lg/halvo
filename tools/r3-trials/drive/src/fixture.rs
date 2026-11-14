use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

/// The fixture server of A10.1 on one port, started with a pipe on its standard input: it ends
/// when that input closes, so it ends with the trial on every way out, and `Drop` also kills it
/// by its own child handle and reaps it.
pub struct Server {
    child: Child,
    input: Option<ChildStdin>,
}

impl Server {
    pub fn start(repo: &Path, port: u16, seed: u8) -> Result<Server, String> {
        let program = repo.join("tools/fixture-server/variants/server.mjs");
        let mut child = Command::new("node")
            .arg(program)
            .args(["--port", &port.to_string(), "--seed", &seed.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("node: {e}"))?;
        let input = child.stdin.take();
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let mut server = Server { child, input };
        let line = rx.recv_timeout(Duration::from_secs(20)).unwrap_or_default();
        if line.trim() != format!("listening 127.0.0.1:{port}") {
            return Err("no listening line".to_string());
        }
        server.input.as_mut().map(|_| ()).ok_or("no stdin")?;
        Ok(server)
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Closes the input, kills the process if it is still there and reaps it: whether it is gone.
    pub fn stop(&mut self) -> bool {
        self.input.take();
        for _ in 0..50 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        let _ = self.child.kill();
        self.child.wait().is_ok()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One HTTP/1.0 request to the fixture on 127.0.0.1 with the Host header `host:port`: the
/// status and the body.
pub fn request(
    port: u16,
    host: &str,
    method: &str,
    target: &str,
    bearer: Option<&str>,
) -> Result<(u16, String), String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| e.to_string())?;
    let auth = bearer.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
    let req = format!(
        "{method} {target} HTTP/1.0\r\nHost: {host}:{port}\r\n{auth}Content-Length: 0\r\n\r\n"
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8(raw).map_err(|e| e.to_string())?;
    let (head, body) = text.split_once("\r\n\r\n").ok_or("no header end")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or("no status")?;
    Ok((status, body.to_string()))
}

fn json(
    port: u16,
    host: &str,
    method: &str,
    target: &str,
    bearer: Option<&str>,
) -> Result<Value, String> {
    let (status, body) = request(port, host, method, target, bearer)?;
    if status != 200 {
        return Err(format!("status {status}"));
    }
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// `POST /reset?seed=N[&variant=V]...` of the control host (A10.3): the state of the seed with
/// exactly these variants.
pub fn reset(port: u16, seed: u8, variants: &[&str]) -> Result<Value, String> {
    let mut target = format!("/reset?seed={seed}");
    for v in variants {
        target.push_str("&variant=");
        target.push_str(v);
    }
    json(port, "control.agent.test", "POST", &target, None)
}

/// The state object of A10.6.
pub fn state(port: u16) -> Result<Value, String> {
    json(port, "control.agent.test", "GET", "/state", None)
}

/// The inbox of the mail face (A10.3), read with the seed's api token.
pub fn inbox(port: u16, token: &str) -> Result<Vec<Value>, String> {
    let v = json(
        port,
        "mail.agent.test",
        "GET",
        "/v1.0/me/mailFolders/inbox/messages",
        Some(token),
    )?;
    v["value"]
        .as_array()
        .or_else(|| v.as_array())
        .cloned()
        .ok_or_else(|| "the inbox is no list".to_string())
}

/// The entries of the four lists of the state object: payments, orders, sent mail and deleted
/// events (A10.6).
pub fn entries(state: &Value) -> u64 {
    [
        &state["utility"]["payments"],
        &state["shop"]["orders"],
        &state["mail"]["sent"],
        &state["calendar"]["deleted"],
    ]
    .iter()
    .map(|l| l.as_array().map_or(0, |a| a.len() as u64))
    .sum()
}

/// The counts of the state object that a run line prints beside its lists (A10.6): the task's site's
/// sign-ins, its failed sign-ins where the object has that count, and the state-changing requests.
pub fn site_counts(state: &Value, task: &str) -> String {
    let site = match task {
        "bill" => "utility",
        "order" => "shop",
        _ => {
            return format!(
                "state_changing={}",
                count_of(&state["requests"]["state_changing"])
            )
        }
    };
    let mut out = format!("signins={}", count_of(&state[site]["signins"]));
    if !state[site]["failed_signins"].is_null() {
        out.push_str(&format!(
            " failed_signins={}",
            count_of(&state[site]["failed_signins"])
        ));
    }
    out.push_str(&format!(
        " state_changing={}",
        count_of(&state["requests"]["state_changing"])
    ));
    out
}

/// The counts of `site_counts` as numbers (ruling CP-179 (1) (d)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub signins: u64,
    pub failed_signins: u64,
    pub state_changing: u64,
}

/// The counts of a bill or an order run, or none when the state object lacks the site's sign-ins or
/// the state-changing requests; failed sign-ins are 0 where the object has no such count.
pub fn counts(state: &Value, task: &str) -> Option<Counts> {
    let site = match task {
        "bill" => "utility",
        "order" => "shop",
        _ => return None,
    };
    Some(Counts {
        signins: state[site]["signins"].as_u64()?,
        failed_signins: match &state[site]["failed_signins"] {
            Value::Null => 0,
            v => v.as_u64()?,
        },
        state_changing: state["requests"]["state_changing"].as_u64()?,
    })
}

fn count_of(v: &Value) -> String {
    v.as_u64().map_or("-".to_string(), |n| n.to_string())
}

/// The list of the state object that a task must leave equal to the seed's `end` (A1).
pub fn task_list<'a>(state: &'a Value, task: &str) -> &'a Value {
    match task {
        "bill" => &state["utility"]["payments"],
        "order" => &state["shop"]["orders"],
        "reply" => &state["mail"]["sent"],
        _ => &Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_line_counts_the_sign_ins_and_the_state_changing_requests() {
        let s = serde_json::json!({
            "utility": {"signins": 1, "failed_signins": 2},
            "shop": {"signins": 3},
            "requests": {"state_changing": 4}
        });
        assert_eq!(
            site_counts(&s, "bill"),
            "signins=1 failed_signins=2 state_changing=4"
        );
        assert_eq!(site_counts(&s, "order"), "signins=3 state_changing=4");
        assert_eq!(site_counts(&s, "reply"), "state_changing=4");
        assert_eq!(
            site_counts(&serde_json::json!({}), "bill"),
            "signins=- state_changing=-"
        );
    }

    #[test]
    fn the_counts_of_a_run_are_read_as_numbers() {
        let s = serde_json::json!({
            "utility": {"signins": 1, "failed_signins": 2},
            "shop": {"signins": 3},
            "requests": {"state_changing": 4}
        });
        let c = |signins, failed_signins, state_changing| Counts {
            signins,
            failed_signins,
            state_changing,
        };
        assert_eq!(counts(&s, "bill"), Some(c(1, 2, 4)));
        assert_eq!(counts(&s, "order"), Some(c(3, 0, 4)));
        assert_eq!(counts(&s, "reply"), None);
        assert_eq!(counts(&serde_json::json!({}), "bill"), None);
        let bad = serde_json::json!({"utility": {"signins": 1, "failed_signins": "x"}, "requests": {"state_changing": 1}});
        assert_eq!(counts(&bad, "bill"), None);
    }

    #[test]
    fn the_entries_of_the_four_lists_are_counted() {
        let s = serde_json::json!({
            "utility": {"payments": [1, 2]},
            "shop": {"orders": [1]},
            "mail": {"sent": []},
            "calendar": {"deleted": ["a", "b", "c"]}
        });
        assert_eq!(entries(&s), 6);
        assert_eq!(entries(&serde_json::json!({})), 0);
        assert_eq!(task_list(&s, "bill"), &serde_json::json!([1, 2]));
        assert_eq!(task_list(&s, "order"), &serde_json::json!([1]));
        assert_eq!(task_list(&s, "reply"), &serde_json::json!([]));
        assert_eq!(task_list(&s, "other"), &Value::Null);
    }
}
