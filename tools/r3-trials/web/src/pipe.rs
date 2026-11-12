//! The browser's debugging channel on a pipe (A11.1, CP-89 (f)): each message is the JSON of one
//! protocol message ended by a NUL byte, in both directions (`--remote-debugging-pipe` in its
//! default JSON mode). The executor's process is the channel's one client.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};

use chromiumoxide::cdp::CdpEventMessage;
use chromiumoxide::types::{CallId, Message, MethodCall, MethodId};
use tokio::sync::mpsc;

/// The most bytes that one message of the browser may take; a longer one ends the channel.
pub const MESSAGE_LIMIT: usize = 256 * 1024 * 1024;

/// The executor's two ends of the pipe, driven by a reading thread and a writing thread.
pub struct PipeLink {
    writes: std::sync::mpsc::Sender<Vec<u8>>,
    pub(crate) reads: mpsc::UnboundedReceiver<Message<CdpEventMessage>>,
    next: usize,
    /// Messages of the browser that were no protocol message of the library's types.
    pub unread: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// One message as the browser reads it: the JSON of `call` and its NUL byte.
pub fn frame(call: &MethodCall) -> serde_json::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(call)?;
    bytes.push(0);
    Ok(bytes)
}

/// Splits what the browser wrote into messages at each NUL byte; the last part, which has no
/// NUL yet, stays in `pending`.
pub fn split_frames(pending: &mut Vec<u8>, more: &[u8]) -> Vec<Vec<u8>> {
    pending.extend_from_slice(more);
    let mut out = Vec::new();
    while let Some(at) = pending.iter().position(|b| *b == 0) {
        let rest = pending.split_off(at + 1);
        let mut one = std::mem::replace(pending, rest);
        one.pop();
        out.push(one);
    }
    out
}

impl PipeLink {
    /// Takes the executor's two ends: `from_browser`, which it reads, and `to_browser`, which it
    /// writes. The channel ends when the browser closes its end or a message is not framed.
    pub fn new(from_browser: File, to_browser: File) -> PipeLink {
        let (tx, reads) = mpsc::unbounded_channel();
        let unread = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let counted = unread.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(from_browser);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match r.read_until(0, &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if buf.last() != Some(&0) || buf.len() > MESSAGE_LIMIT {
                            break;
                        }
                        buf.pop();
                        match serde_json::from_slice::<Message<CdpEventMessage>>(&buf) {
                            Ok(m) => {
                                if tx.send(m).is_err() {
                                    break;
                                }
                            }
                            Err(_) => {
                                counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
        });
        let (writes, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut w = to_browser;
            while let Ok(bytes) = rx.recv() {
                if w.write_all(&bytes).and_then(|_| w.flush()).is_err() {
                    break;
                }
            }
        });
        PipeLink {
            writes,
            reads,
            next: 0,
            unread,
        }
    }

    /// Writes one command and returns its id.
    pub fn submit(
        &mut self,
        method: MethodId,
        session_id: Option<String>,
        params: serde_json::Value,
    ) -> Result<CallId, String> {
        self.next += 1;
        let id = CallId::new(self.next);
        let call = MethodCall {
            id,
            method,
            session_id,
            params,
        };
        let bytes = frame(&call).map_err(|e| e.to_string())?;
        self.writes
            .send(bytes)
            .map_err(|_| "pipe closed".to_string())?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_its_json_and_one_nul_and_the_reader_splits_at_each_nul() {
        let call = MethodCall {
            id: CallId::new(7),
            method: "Browser.getVersion".into(),
            session_id: None,
            params: serde_json::json!({}),
        };
        let bytes = frame(&call).expect("frame");
        assert_eq!(bytes.last(), Some(&0));
        assert_eq!(
            &bytes[..bytes.len() - 1],
            br#"{"id":7,"method":"Browser.getVersion","params":{}}"#
        );
        let mut pending = Vec::new();
        assert!(split_frames(&mut pending, b"{\"id\":1").is_empty());
        let got = split_frames(
            &mut pending,
            b",\"result\":{}}\0{\"id\":2,\"result\":{}}\0{\"id\"",
        );
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], br#"{"id":1,"result":{}}"#);
        assert_eq!(pending, br#"{"id""#);
        let m: Message<CdpEventMessage> = serde_json::from_slice(&got[1]).expect("message");
        assert!(matches!(m, Message::Response(r) if r.id == CallId::new(2)));
    }
}
