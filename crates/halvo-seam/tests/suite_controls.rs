use std::fs;
use std::io::{self, PipeReader, PipeWriter};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use halvo_seam::conformance::{self, Subject, CASES};
use halvo_seam::fixture::FixtureTransport;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_seam::message::{
    decode_client, Cause, ClientMessage, OpenCursor, Refusal, ServerError, ServerMessage,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    None,
    RestartLosesASegment,
    ShredNothing,
    LoadKeepsTheOldStore,
    LoadDropsALine,
    IgnoreKinds,
    IgnoreRange,
}

struct Control {
    test: String,
    fault: Fault,
    files: Vec<PathBuf>,
    first_live: u64,
    idle: Option<FixtureTransport>,
    serving: Option<JoinHandle<FixtureTransport>>,
}

impl Control {
    fn new(test: &str, fault: Fault) -> Control {
        Control {
            test: test.to_owned(),
            fault,
            files: Vec::new(),
            first_live: 1,
            idle: None,
            serving: None,
        }
    }

    fn settle(&mut self) {
        if let Some(handle) = self.serving.take() {
            self.idle = Some(handle.join().unwrap());
        }
    }

    fn reopen(&mut self) -> Result<(), String> {
        let t = FixtureTransport::open(&self.files, self.first_live, None)
            .map_err(|e| format!("{e:?}"))?;
        self.idle = Some(t);
        Ok(())
    }
}

fn unfiltered(open: &OpenCursor, fault: Fault) -> OpenCursor {
    let kinds = if fault == Fault::IgnoreKinds {
        None
    } else {
        open.kinds().cloned()
    };
    let range = if fault == Fault::IgnoreRange {
        None
    } else {
        open.range()
    };
    match open.store() {
        None => OpenCursor::fresh(kinds, range),
        Some(s) => OpenCursor::at(s.clone(), open.position(), kinds, range).unwrap(),
    }
}

impl Subject for Control {
    type Writer = PipeWriter;
    type Reader = PipeReader;

    fn load(&mut self, batch_files: &[&str]) -> Result<(), String> {
        self.settle();
        if self.fault == Fault::LoadKeepsTheOldStore && batch_files == conformance::SET_B {
            return self.reopen();
        }
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
            let mut text = text.to_string();
            if self.fault == Fault::LoadDropsALine && i == 1 {
                let cut = text[..text.len() - 1].rfind('\n').map_or(0, |p| p + 1);
                text.truncate(cut);
            }
            let path = dir.join(format!("day-{:03}.jsonl", i + 1));
            fs::write(&path, text).map_err(|e| e.to_string())?;
            self.files.push(path);
        }
        self.first_live = 1;
        self.reopen()
    }

    fn connect(&mut self) -> Result<(PipeWriter, PipeReader), String> {
        self.settle();
        let mut t = self.idle.take().ok_or("no store")?;
        let fault = self.fault;
        let (mut requests_in, requests_out) = io::pipe().map_err(|e| e.to_string())?;
        let (replies_in, mut replies_out) = io::pipe().map_err(|e| e.to_string())?;
        self.serving = Some(std::thread::spawn(move || {
            while let Ok(Some(payload)) = read_frame(&mut requests_in, CLIENT_FRAME_MAX) {
                let reply = match decode_client(&payload) {
                    Ok(ClientMessage::OpenCursor(open)) => {
                        t.handle(&ClientMessage::OpenCursor(unfiltered(&open, fault)))
                    }
                    Ok(other) => t.handle(&other),
                    Err(Refusal::BadRequest) => ServerMessage::Error(
                        ServerError::new(Cause::BadRequest, "refused").unwrap(),
                    ),
                    Err(Refusal::BadFrame) => break,
                };
                let reply = reply.to_payload().unwrap();
                if write_frame(&mut replies_out, &reply, SERVER_FRAME_MAX).is_err() {
                    break;
                }
            }
            t
        }));
        Ok((requests_out, replies_in))
    }

    fn restart(&mut self) -> Result<(), String> {
        self.settle();
        if self.fault == Fault::RestartLosesASegment {
            self.first_live += 1;
        }
        self.reopen()
    }

    fn shred_oldest(&mut self) -> Result<(), String> {
        self.settle();
        if self.fault != Fault::ShredNothing {
            self.first_live += 1;
        }
        self.reopen()
    }
}

#[test]
fn run_passes_a_conforming_server_and_fails_each_broken_one_in_the_case_that_shows_it() {
    let table = [
        (Fault::None, [true, true, true, true]),
        (Fault::RestartLosesASegment, [false, true, true, true]),
        (Fault::ShredNothing, [true, false, true, true]),
        (Fault::IgnoreKinds, [true, true, false, true]),
        (Fault::IgnoreRange, [true, true, false, true]),
        (Fault::LoadKeepsTheOldStore, [true, true, true, false]),
        (Fault::LoadDropsALine, [false, false, false, false]),
    ];
    for (fault, want) in table {
        let mut subject = Control::new(&format!("suite_controls-{fault:?}"), fault);
        let results = conformance::run(&mut subject);
        let names: Vec<&str> = results.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, CASES);
        let got: Vec<bool> = results.iter().map(|(_, r)| r.is_ok()).collect();
        assert_eq!(got, want, "{fault:?}: {results:?}");
    }
}
