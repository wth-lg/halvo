use super::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::AtomicUsize;

pub(crate) const CANARIES: [&[u8; 16]; 4] = [
    b"buffer-canary-01",
    b"buffer-canary-02",
    b"buffer-canary-03",
    b"keychain-canary1",
];

static SEEN: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];

struct Detector;

unsafe impl GlobalAlloc for Detector {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        System.alloc_zeroed(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        System.alloc_zeroed(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let block = std::slice::from_raw_parts(ptr, layout.size());
        for (seen, canary) in SEEN.iter().zip(CANARIES) {
            if block.windows(canary.len()).any(|window| window == canary) {
                seen.fetch_add(1, Ordering::SeqCst);
            }
        }
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static DETECTOR: Detector = Detector;

pub(crate) fn seen(index: usize) -> usize {
    SEEN[index].load(Ordering::SeqCst)
}

#[test]
fn a_read_buffer_overwrites_what_it_holds_before_it_is_freed() {
    let mut control = vec![0u8; 64];
    control[10..26].copy_from_slice(CANARIES[0]);
    drop(control);
    assert_eq!(
        seen(0),
        1,
        "the detector did not see the control's bytes freed"
    );

    let mut buffer = Buffer::zeroed(64);
    buffer.space()[40..56].copy_from_slice(CANARIES[1]);
    drop(buffer);
    assert_eq!(
        seen(1),
        0,
        "a dropped read buffer freed its bytes as they were"
    );

    let mut buffer = Buffer::zeroed(64);
    buffer.space()[..8].copy_from_slice(b"aaaaaaaa");
    buffer.space()[30..46].copy_from_slice(CANARIES[2]);
    assert_eq!(buffer.capacity(), 64);
    let secret = Secret::new(buffer.into_vec(8)).expect("a secret of 8 bytes");
    assert!(secret.expose() == b"aaaaaaaa");
    drop(secret);
    assert_eq!(
        seen(2),
        0,
        "the bytes past the length of a read freed as they were"
    );
}
