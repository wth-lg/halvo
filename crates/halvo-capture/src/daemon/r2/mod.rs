//! The daemon's part of R2: the filter that masks a window title before it is
//! stored, and the hooks of the store owner's pass and of the supervisor's
//! start, which do nothing yet.

mod filter;

pub(super) use filter::text_filter;

/// The R2 work of one pass of the store owner, after its due work: none yet.
pub(super) fn owner_pass<T>(_owner: &mut T) {}

/// The R2 children that the supervisor starts after its sources: none yet.
pub(super) fn start_children<T>(_supervisor: &mut T) {}
