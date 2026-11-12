use halvo_secrets::{Secret, MAX_SECRET_LEN};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

const CANARIES: [&[u8; 16]; 4] = [
    b"wipe-canary-0001",
    b"wipe-canary-0002",
    b"wipe-canary-0003",
    b"wipe-canary-0004",
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

fn bytes_with(canary: &[u8], capacity: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(b"aaaaaaaa");
    bytes.extend_from_slice(canary);
    bytes
}

fn seen(index: usize) -> usize {
    SEEN[index].load(Ordering::SeqCst)
}

#[test]
fn a_secret_overwrites_its_whole_buffer_before_the_buffer_is_freed() {
    let control = bytes_with(CANARIES[0], 24);
    drop(control);
    assert_eq!(
        seen(0),
        1,
        "the detector did not see the control's bytes freed"
    );

    let secret = Secret::new(bytes_with(CANARIES[1], 24)).expect("a secret of 24 bytes");
    assert_eq!(secret.expose().len(), 24);
    drop(secret);
    assert_eq!(seen(1), 0, "a dropped secret freed its bytes as they were");

    let mut spare = bytes_with(CANARIES[2], 64);
    spare.truncate(8);
    let secret = Secret::new(spare).expect("a secret of 8 bytes");
    assert!(secret.expose() == b"aaaaaaaa");
    drop(secret);
    assert_eq!(
        seen(2),
        0,
        "a dropped secret freed its spare capacity as it was"
    );

    let mut long = bytes_with(CANARIES[3], MAX_SECRET_LEN + 1);
    long.resize(MAX_SECRET_LEN + 1, b'a');
    assert!(Secret::new(long).is_none());
    assert_eq!(seen(3), 0, "a refused secret freed its bytes as they were");
}
