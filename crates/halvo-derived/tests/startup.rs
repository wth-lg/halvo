mod common;

use std::sync::{Arc, Barrier};

use common::{key, registry, Scratch, Seq, TestClock};
use halvo_derived::{Host, Store};

#[test]
fn stores_opened_at_once_on_many_threads_of_a_new_process_all_open() {
    let threads = 16;
    let barrier = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|n| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let scratch = Scratch::new(&format!("startup-{n}"));
                let clock = TestClock::at_day(20_767);
                barrier.wait();
                Store::open(
                    scratch.path(),
                    &key(),
                    &registry(),
                    Host::new(Box::new(clock), Box::new(common::NoProposals)),
                    &mut Seq(1),
                )
                .map(drop)
                .map_err(|e| e.to_string())
            })
        })
        .collect();
    let failed: Vec<String> = handles
        .into_iter()
        .filter_map(|h| h.join().unwrap().err())
        .collect();
    assert!(
        failed.is_empty(),
        "{} opens failed: {failed:?}",
        failed.len()
    );
}
