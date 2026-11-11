use std::fs;
use std::io::{self, PipeReader, PipeWriter};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use halvo_seam::conformance::{self, Subject};
use halvo_seam::fixture::FixtureTransport;

struct Fixture {
    test: &'static str,
    files: Vec<PathBuf>,
    first_live: u64,
    idle: Option<FixtureTransport>,
    serving: Option<JoinHandle<FixtureTransport>>,
}

impl Fixture {
    fn new(test: &'static str) -> Fixture {
        Fixture {
            test,
            files: Vec::new(),
            first_live: 1,
            idle: None,
            serving: None,
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
        self.serving = Some(std::thread::spawn(move || {
            let _ = transport.serve(requests_in, replies_out);
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
