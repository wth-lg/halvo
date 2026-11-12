use std::alloc::{GlobalAlloc, Layout, System};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use halvo_seam::fixture::{FixtureTransport, OpenError};
use halvo_seam::input_set::{InputSet, SetError};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(n: usize) {
    let live = LIVE.fetch_add(n, Ordering::SeqCst) + n;
    PEAK.fetch_max(live, Ordering::SeqCst);
}

fn shrink(n: usize) {
    LIVE.fetch_sub(n, Ordering::SeqCst);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc_zeroed(layout);
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        shrink(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            if new_size > layout.size() {
                grow(new_size - layout.size());
            } else {
                shrink(layout.size() - new_size);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const APP_TITLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/r2/input-sets/app-title.toml"
));

const BOUND: usize = 4_096;

fn new_dir(test: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    for n in 0..100_000 {
        let dir = base.join(format!("halvo-seam-{test}-{n}"));
        if fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
    panic!("no new directory under CARGO_TARGET_TMPDIR after 100000 names");
}

fn measured<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = LIVE.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let out = f();
    (out, PEAK.load(Ordering::SeqCst) - before)
}

#[test]
fn set_reader_memory() {
    let dir = new_dir("set_reader_memory");
    let ten = dir.join("ten-mib.toml");
    {
        let mut w = BufWriter::new(File::create(&ten).unwrap());
        w.write_all(APP_TITLE.as_bytes()).unwrap();
        let chunk = [b'x'; 65_536];
        for _ in 0..160 {
            w.write_all(&chunk).unwrap();
        }
        w.flush().unwrap();
    }
    let sixty_four = dir.join("sixty-four-mib.toml");
    File::create(&sixty_four)
        .unwrap()
        .set_len(64 * 1_048_576)
        .unwrap();
    let none: &[PathBuf] = &[];
    for (path, line) in [(&ten, 6), (&sixty_four, 1)] {
        let bytes = fs::metadata(path).unwrap().len();
        let (read, peak) = measured(|| InputSet::read(path));
        println!(
            "set_reader_memory read file_bytes={bytes} peak_above_before={peak} bound={BOUND}"
        );
        assert!(
            matches!(read, Err(SetError::Form { line: l, .. }) if l == line),
            "{read:?}"
        );
        assert!(peak <= BOUND, "the set reader held {peak} bytes");
        let (opened, peak) = measured(|| FixtureTransport::open(none, 1, Some(path)).err());
        println!(
            "set_reader_memory open file_bytes={bytes} peak_above_before={peak} bound={BOUND}"
        );
        assert!(
            matches!(opened, Some(OpenError::InputSet(SetError::Form { line: l, .. })) if l == line),
            "{opened:?}"
        );
        assert!(peak <= BOUND, "the transport's open held {peak} bytes");
    }
    File::create(&ten).unwrap();
    File::create(&sixty_four).unwrap();
}
