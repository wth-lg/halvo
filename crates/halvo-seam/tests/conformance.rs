use std::fs;
use std::io::{self, PipeReader, PipeWriter};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use halvo_schema::StoreId;
use halvo_seam::conformance::{self, Subject};
use halvo_seam::fixture::{FixtureTransport, Replace};
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_seam::message::{
    decode_client, Cause, ClientMessage, OpenCursor, Refusal, ServerError, ServerMessage, ZERO,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    StoreChangedOnReadNamesAnotherId,
    StoreChangedAtZeroNamesAnotherId,
    OpenWhileOpenServes,
    OpenWhileOpenClosesTheCursor,
    WatermarkRefusedAfterAShred,
}

use Fault::*;

struct Fixture {
    test: &'static str,
    fault: Option<Fault>,
    files: Vec<PathBuf>,
    first_live: u64,
    idle: Option<FixtureTransport>,
    serving: Option<JoinHandle<FixtureTransport>>,
    replace: Option<Replace>,
}

impl Fixture {
    fn new(test: &'static str) -> Fixture {
        Fixture {
            test,
            fault: None,
            files: Vec::new(),
            first_live: 1,
            idle: None,
            serving: None,
            replace: None,
        }
    }

    fn wrong(test: &'static str, fault: Fault) -> Fixture {
        Fixture {
            fault: Some(fault),
            ..Fixture::new(test)
        }
    }

    fn settle(&mut self) -> Result<(), String> {
        if let Some(handle) = self.serving.take() {
            let transport = handle
                .join()
                .map_err(|_| "the server thread panicked".to_owned())?;
            self.idle = Some(transport);
        }
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), String> {
        let transport = FixtureTransport::open(&self.files, self.first_live, None)
            .map_err(|e| format!("open failed: {e:?}"))?;
        self.replace = Some(transport.replace_handle());
        self.idle = Some(transport);
        Ok(())
    }
}

impl Subject for Fixture {
    type Writer = PipeWriter;
    type Reader = PipeReader;

    fn load(&mut self, batch_files: &[&str]) -> Result<(), String> {
        self.settle()?;
        let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let mut n = 0;
        let dir = loop {
            let dir = base.join(format!("halvo-seam-{}-{n}", self.test));
            if fs::create_dir(&dir).is_ok() {
                break dir;
            }
            n += 1;
        };
        self.files = Vec::new();
        for (i, text) in batch_files.iter().enumerate() {
            let path = dir.join(format!("day-{:03}.jsonl", i + 1));
            fs::write(&path, text).map_err(|e| format!("write failed: {e}"))?;
            self.files.push(path);
        }
        self.first_live = 1;
        self.reopen()
    }

    fn connect(&mut self) -> Result<(PipeWriter, PipeReader), String> {
        self.settle()?;
        let mut transport = self.idle.take().ok_or("no store is loaded")?;
        let (requests_in, requests_out) = io::pipe().map_err(|e| e.to_string())?;
        let (replies_in, replies_out) = io::pipe().map_err(|e| e.to_string())?;
        let fault = self.fault;
        let shredded = self.first_live > 1;
        self.serving = Some(std::thread::spawn(move || {
            match fault {
                None => {
                    let _ = transport.serve(requests_in, replies_out);
                }
                Some(fault) => {
                    let mut wrong = Wrong { fault, shredded };
                    wrong.serve(&mut transport, requests_in, replies_out);
                }
            }
            transport
        }));
        Ok((requests_out, replies_in))
    }

    fn restart(&mut self) -> Result<(), String> {
        self.settle()?;
        self.reopen()
    }

    fn shred_oldest(&mut self) -> Result<(), String> {
        self.settle()?;
        self.first_live += 1;
        self.reopen()
    }

    fn replace_live(&mut self) -> Result<(), String> {
        self.replace.as_ref().ok_or("no store is loaded")?.replace();
        self.files = Vec::new();
        self.first_live = 1;
        Ok(())
    }
}

fn other(store: &StoreId) -> StoreId {
    let s = store.as_str();
    let first = if s.starts_with('0') { "1" } else { "0" };
    StoreId::parse(&format!("{first}{}", &s[1..])).unwrap()
}

fn error(cause: Cause) -> ServerMessage {
    ServerMessage::Error(ServerError::new(cause, "wrong server").unwrap())
}

struct Wrong {
    fault: Fault,
    shredded: bool,
}

impl Wrong {
    fn open(&mut self, t: &mut FixtureTransport, open: &OpenCursor) -> ServerMessage {
        if self.fault == WatermarkRefusedAfterAShred
            && self.shredded
            && open.position() == t.watermark()
        {
            return error(Cause::BadPosition);
        }
        open_reply(t, self.fault, open)
    }

    fn serve(&mut self, t: &mut FixtureTransport, mut input: PipeReader, mut output: PipeWriter) {
        while let Ok(Some(payload)) = read_frame(&mut input, CLIENT_FRAME_MAX) {
            let reply = match decode_client(&payload) {
                Ok(ClientMessage::OpenCursor(open)) => self.open(t, &open),
                Ok(ClientMessage::ReadBatch) => read_reply(t, self.fault),
                Ok(message) => t.handle(&message),
                Err(Refusal::BadRequest) => error(Cause::BadRequest),
                Err(Refusal::BadFrame) => break,
            };
            let Ok(bytes) = reply.to_payload() else {
                break;
            };
            if write_frame(&mut output, &bytes, SERVER_FRAME_MAX).is_err() {
                break;
            }
        }
    }
}

fn open_reply(t: &mut FixtureTransport, fault: Fault, open: &OpenCursor) -> ServerMessage {
    let reply = t.handle(&ClientMessage::OpenCursor(open.clone()));
    match (fault, reply) {
        (StoreChangedAtZeroNamesAnotherId, ServerMessage::Error(e)) if open.position() == ZERO => {
            match e.cause() {
                Cause::StoreChanged(id) => error(Cause::StoreChanged(other(id))),
                _ => ServerMessage::Error(e),
            }
        }
        (OpenWhileOpenServes, ServerMessage::Error(e)) if *e.cause() == Cause::BadRequest => {
            t.handle(&ClientMessage::Close);
            t.handle(&ClientMessage::OpenCursor(open.clone()))
        }
        (OpenWhileOpenClosesTheCursor, ServerMessage::Error(e))
            if *e.cause() == Cause::BadRequest =>
        {
            t.handle(&ClientMessage::Close);
            ServerMessage::Error(e)
        }
        (_, reply) => reply,
    }
}

fn read_reply(t: &mut FixtureTransport, fault: Fault) -> ServerMessage {
    match (fault, t.handle(&ClientMessage::ReadBatch)) {
        (StoreChangedOnReadNamesAnotherId, ServerMessage::Error(e)) => match e.cause() {
            Cause::StoreChanged(id) => error(Cause::StoreChanged(other(id))),
            _ => ServerMessage::Error(e),
        },
        (_, reply) => reply,
    }
}

fn wrong(test: &'static str, faults: &[Fault], want: [Option<&str>; 4]) {
    let mut missed = Vec::new();
    for &fault in faults {
        let mut subject = Fixture::wrong(test, fault);
        let results = conformance::run(&mut subject);
        for ((case, got), want) in results.iter().zip(want) {
            match (want, got) {
                (None, Ok(())) => {}
                (Some(check), Err(e)) if e.contains(&format!("check {check}: ")) => {}
                _ => missed.push(format!(
                    "{fault:?}, case {case}: want {want:?}, got {got:?}"
                )),
            }
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

#[test]
fn conformance_resume() {
    let mut subject = Fixture::new("conformance_resume");
    conformance::resume(&mut subject).unwrap();
}

#[test]
fn conformance_shredded() {
    let mut subject = Fixture::new("conformance_shredded");
    conformance::shredded(&mut subject).unwrap();
}

#[test]
fn conformance_filters() {
    let mut subject = Fixture::new("conformance_filters");
    conformance::filters(&mut subject).unwrap();
}

#[test]
fn conformance_store_changed() {
    let mut subject = Fixture::new("conformance_store_changed");
    conformance::store_changed(&mut subject).unwrap();
}

#[test]
fn conformance_check_store_changed_names_the_current_id() {
    let test = "conformance_check_store_changed_names_the_current_id";
    let check = Some("store_changed_names_the_current_id");
    wrong(
        test,
        &[
            StoreChangedOnReadNamesAnotherId,
            StoreChangedAtZeroNamesAnotherId,
        ],
        [None, None, None, check],
    );
}

#[test]
fn conformance_check_open_while_open() {
    let test = "conformance_check_open_while_open";
    let check = Some("open_while_open");
    wrong(
        test,
        &[OpenWhileOpenServes, OpenWhileOpenClosesTheCursor],
        [None, None, check, None],
    );
}

#[test]
fn conformance_check_open_at_the_watermark_after_a_shred() {
    let test = "conformance_check_open_at_the_watermark_after_a_shred";
    let check = Some("open_at_the_watermark_after_a_shred");
    wrong(
        test,
        &[WatermarkRefusedAfterAShred],
        [None, check, None, None],
    );
}
