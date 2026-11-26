#![forbid(unsafe_code)]
//! The runner's maker of a snapshot for the driver trial's form `--marks` (R3, unit U0.10). The module `snapshot`
//! below IS the runner's own file, crates/halvo-runner/src/model/snapshot.rs, brought in by its path: this crate
//! holds no copy of it and defines no mask of its own. The three functions here hand its three items on, since the
//! file's own items are visible in their crate alone.

#[allow(dead_code)]
#[path = "../../../../crates/halvo-runner/src/model/snapshot.rs"]
mod snapshot;

pub use halvo_schema::executable::record::Slot;
/// A line of a page as the runner's maker reads it: the type of halvo-web.
pub use halvo_web::cdp::SnapshotLine as Line;

use halvo_schema::executable::text::{Name, OwnValue};

/// The slot `name` with the text `text`, or none where `name` is no name of the schema.
pub fn slot(name: &str, text: &str) -> Option<Slot> {
    Some(Slot {
        name: Name::parse(name).ok()?,
        value: OwnValue::new(text),
    })
}

/// The snapshot of a page's `lines` for a request of a run whose slots are `slots`, as the runner's compile
/// makes it (`Snapshot::masked` of the runner's file).
pub fn masked(lines: &[Line], slots: &[Slot]) -> String {
    snapshot::Snapshot::masked(lines, slots).text().to_owned()
}

/// The snapshot of a message that is answered, as the runner's writer makes it (`Snapshot::of_a_message` of the
/// runner's file).
pub fn message(subject: &str, body: &str, slots: &[Slot]) -> String {
    snapshot::Snapshot::of_a_message(subject, body, slots)
        .text()
        .to_owned()
}

/// Whether a text that a model wrote holds the head of a mark (`holds_a_mark` of the runner's file).
pub fn holds_a_mark(text: &str) -> bool {
    snapshot::holds_a_mark(text)
}
