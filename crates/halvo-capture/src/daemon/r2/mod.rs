//! The daemon's part of R2: the hooks of the store owner's pass and of the
//! supervisor's start, and the site reader point of the title sources. The
//! pass offers the held edge of the mark handler and serves the cursor one
//! waiting message; the start does nothing and the point reads no site yet.

pub(super) mod marks;
pub(super) mod cursor;

/// The site reader point of the title sources, for the process `_pid`: it reads no site yet.
pub(crate) fn site_reader<P>(_pid: P) -> Option<String> {
    None
}

/// The R2 work of one pass of the store owner, after its due work: the mark
/// handler offers the edge that it holds, with the wall clock's reading, and
/// the cursor serves the message that waits, unless the pass took an event.
#[cfg_attr(
    not(any(test, all(target_os = "macos", debug_assertions))),
    expect(
        dead_code,
        reason = "the store owner, its one caller, is built in tests and in macOS debug builds alone"
    )
)]
pub(super) fn owner_pass<T: marks::MarkEntry + cursor::ReadEntry>(owner: &mut T, took_event: bool) {
    marks::handler().offer(owner, std::time::SystemTime::now());
    cursor::cursor().pass(owner, took_event);
}

/// The R2 children that the supervisor starts after its sources: none yet.
#[cfg_attr(
    not(any(test, all(target_os = "macos", debug_assertions))),
    expect(
        dead_code,
        reason = "the supervisor, its one caller, is built in tests and in macOS debug builds alone"
    )
)]
pub(super) fn start_children<T>(_supervisor: &mut T) {}
pub mod owner_slot;
