#![allow(dead_code)]

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use halvo_derived::{Clock, DbKey, Host, IdSource, OpenError, Store};
use halvo_schema_core::kinds::{self, KindEntry};
use halvo_schema_core::record::Instant;
use halvo_sqlcipher::rusqlite::Connection;

pub const DAY_NS: i64 = 86_400_000_000_000;

pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("derived-{name}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn db(&self) -> PathBuf {
        self.path.join("derived").join("derived.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Clone)]
pub struct TestClock(Rc<Cell<i64>>);

impl TestClock {
    pub fn at_day(day: u32) -> TestClock {
        TestClock(Rc::new(Cell::new(
            i64::from(day) * DAY_NS + 12 * 3_600_000_000_000,
        )))
    }

    pub fn set_day(&self, day: u32) {
        self.0.set(i64::from(day) * DAY_NS + 12 * 3_600_000_000_000);
    }

    pub fn set(&self, now: i64) {
        self.0.set(now);
    }

    pub fn now(&self) -> i64 {
        self.0.get()
    }
}

impl Clock for TestClock {
    fn now(&self) -> Instant {
        Instant(self.0.get())
    }

    fn day(&self, at: Instant) -> u32 {
        u32::try_from(at.0.div_euclid(DAY_NS)).unwrap_or(0)
    }

    fn day_start(&self, day: u32) -> Instant {
        Instant(i64::from(day) * DAY_NS)
    }
}

pub struct Seq(pub u64);

impl IdSource for Seq {
    fn fill(&mut self, bytes: &mut [u8]) {
        for chunk in bytes.chunks_mut(8) {
            self.0 += 1;
            let word = self.0.to_be_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
    }
}

pub fn key() -> DbKey {
    DbKey::new([7; 32])
}

pub fn registry() -> Vec<&'static KindEntry> {
    kinds::registry().collect()
}

pub fn open(scratch: &Scratch, clock: &TestClock) -> Result<Store, OpenError> {
    open_with(scratch, clock, &registry())
}

pub fn open_with(
    scratch: &Scratch,
    clock: &TestClock,
    entries: &[&'static KindEntry],
) -> Result<Store, OpenError> {
    Store::open(
        scratch.path(),
        &key(),
        entries,
        Host::new(Box::new(clock.clone())),
        &mut Seq(1 << 40),
    )
}

pub fn raw(scratch: &Scratch) -> Connection {
    let conn = Connection::open(scratch.db()).unwrap();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", "07".repeat(32)))
        .unwrap();
    conn
}
