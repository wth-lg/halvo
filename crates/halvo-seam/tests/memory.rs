use std::alloc::{GlobalAlloc, Layout, System};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use halvo_seam::fixture::FixtureTransport;
use halvo_seam::message::{ClientMessage, OpenCursor, ServerMessage};

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

const FILE_MIN: usize = 16 * 1024 * 1024;
const HELD_MAX: usize = 4 * 1024 * 1024;

fn new_dir(test: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let mut n = 0;
    loop {
        let dir = base.join(format!("halvo-seam-{test}-{n}"));
        if fs::create_dir(&dir).is_ok() {
            return dir;
        }
        n += 1;
    }
}

#[test]
fn transport_memory() {
    let dir = new_dir("transport_memory");
    let path = dir.join("day-001.jsonl");
    let mut written = 0usize;
    let mut lines = 0u64;
    {
        let mut out = BufWriter::new(File::create(&path).unwrap());
        while written < FILE_MIN {
            let line = format!(
                "{{\"v\":1,\"id\":\"019b78c9-0000-7000-8000-{lines:012x}\",\"at\":{},\"seq\":{lines},\"kind\":\"keystroke\",\"app\":\"com.example.editor\",\"p\":{{\"class\":\"printable\"}}}}\n",
                1_767_258_000_000_000_000i64 + lines as i64 * 1_000_000_000
            );
            out.write_all(line.as_bytes()).unwrap();
            written += line.len();
            lines += 1;
        }
        out.flush().unwrap();
    }
    let before = LIVE.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let mut transport = FixtureTransport::open(&[&path], 1, None).unwrap();
    let open = ClientMessage::OpenCursor(OpenCursor::fresh(None, None));
    assert!(matches!(transport.handle(&open), ServerMessage::Batch(_)));
    let mut served = 0u64;
    let mut reads = 0u64;
    loop {
        let ServerMessage::Batch(batch) = transport.handle(&ClientMessage::ReadBatch) else {
            panic!("a read was not served");
        };
        served += batch.records.len() as u64;
        reads += 1;
        let done = batch.next == batch.watermark;
        drop(batch);
        if done {
            break;
        }
    }
    let peak = PEAK.load(Ordering::SeqCst);
    drop(transport);
    let held = peak.saturating_sub(before);
    println!("transport_memory file_bytes={written} reads={reads} held_bytes={held}");
    assert_eq!(served, lines);
    assert!(reads as usize >= written / 262_144);
    assert!(held <= HELD_MAX, "the transport held {held} bytes at once");
    File::create(&path).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
}
