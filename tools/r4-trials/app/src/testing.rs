use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const BOUND: Duration = Duration::from_secs(10);

pub(crate) fn join<T>(handle: JoinHandle<T>) -> T {
    let deadline = Instant::now() + BOUND;
    while !handle.is_finished() {
        assert!(
            Instant::now() < deadline,
            "the thread did not end within {BOUND:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
    handle
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

pub(crate) fn within<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    join(thread::spawn(body))
}
