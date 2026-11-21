//! The daemon's part of R2: the hooks of the store owner's pass and of the
//! supervisor's start. The pass offers the held edge of the mark handler; the
//! start does nothing yet.

pub(super) mod marks;

/// The R2 work of one pass of the store owner, after its due work: the mark
/// handler offers the edge that it holds, with the wall clock's reading.
#[cfg_attr(
    not(any(test, all(target_os = "macos", debug_assertions))),
    expect(
        dead_code,
        reason = "the store owner, its one caller, is built in tests and in macOS debug builds alone"
    )
)]
pub(super) fn owner_pass<T: marks::MarkEntry>(owner: &mut T) {
    marks::handler().offer(owner, std::time::SystemTime::now());
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
