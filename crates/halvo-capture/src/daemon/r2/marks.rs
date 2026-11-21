//! The handler of the two mark requests (contracts.md C24.12 of the workflows
//! run): for a `run_start` or a `run_end` it builds one record of the reserved
//! kind `run_mark` with that edge and hands it to the entry of the store owner.
//!
//! The entry answers true when it appended the record to the batch that the
//! owner is filling, and false when the owner did not take it: while it
//! discards, and past its bound while its drain fails. The handler learns no
//! cause. After a false it holds one edge at most, with no instant and no
//! content, and offers it again on each pass of the owner. The record of an
//! offer carries the instant of that offer.
//!
//! What a request does, by the edge that the handler holds:
//!
//! ```text
//! held        run_start                        run_end
//! none        offered; refused: holds start    offered; refused: holds end
//! start       nothing                          drops the held start
//! end         holds a restated start           nothing
//! restated    nothing                          offered; refused: holds end
//! ```
//!
//! A request answers true only when the owner took its record. A restated
//! start stands in the place of a held end, so the run that was open stays
//! open; it is offered as a start, and a `run_end` that meets it is offered as
//! any `run_end` and drops it. The handler has three entries, and each takes
//! the owner and the instant from its caller: a request, the offer of a pass,
//! and the drop of the held edge when a delete of everything begins. The
//! process has one handler, [`handler`].

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use halvo_control::r2::RunEdge;

use crate::floor::record::RunMark;

/// The entry of the store owner for a record of the reserved kind, as the
/// handler reaches it. Its one impl is for the owner, in the builds that hold
/// one.
pub(in crate::daemon) trait MarkEntry {
    /// Appends `mark` to the batch that the owner is filling. False when the
    /// owner did not take it: nothing is queued then, and no counter moves.
    fn take_mark(&mut self, mark: RunMark) -> bool;
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
impl<S, C> MarkEntry for crate::daemon::owner::Owner<S, C>
where
    S: crate::daemon::port::CaptureStore,
    C: crate::daemon::maintenance::Clocks,
{
    fn take_mark(&mut self, mark: RunMark) -> bool {
        self.append_run_mark(mark)
    }
}

/// The edge that the handler holds after an offer that the owner did not take.
#[derive(Clone, Copy)]
enum Held {
    /// A `start` that the owner did not take.
    Start,
    /// An `end` that the owner did not take.
    End,
    /// A `start` in the place of a held `end`.
    Restated,
}

impl Held {
    /// The edge of the record that is offered for it.
    const fn edge(self) -> RunEdge {
        match self {
            Held::Start | Held::Restated => RunEdge::Start,
            Held::End => RunEdge::End,
        }
    }
}

/// The handler of the mark requests: the edge that it holds, or none.
pub(in crate::daemon) struct Marks {
    held: Option<Held>,
}

impl Marks {
    /// A handler that holds nothing.
    pub(in crate::daemon) const fn new() -> Marks {
        Marks { held: None }
    }

    /// A mark request with `edge` at the instant `at`. The answer is the
    /// `stored` of the reply: true when the owner took the record of this
    /// request.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the dispatch lines of the capture run call it")
    )]
    pub(in crate::daemon) fn request(
        &mut self,
        owner: &mut impl MarkEntry,
        at: SystemTime,
        edge: RunEdge,
    ) -> bool {
        match (self.held, edge) {
            (None, _) | (Some(Held::Restated), RunEdge::End) => {
                let stored = taken(owner, at, edge);
                self.held = (!stored).then_some(match edge {
                    RunEdge::Start => Held::Start,
                    RunEdge::End => Held::End,
                });
                stored
            }
            (Some(Held::Start), RunEdge::End) => {
                self.held = None;
                false
            }
            (Some(Held::End), RunEdge::Start) => {
                self.held = Some(Held::Restated);
                false
            }
            (Some(Held::Start | Held::Restated), RunEdge::Start)
            | (Some(Held::End), RunEdge::End) => false,
        }
    }

    /// The offer of one pass of the owner at the instant `at`: a held edge is
    /// offered again, and is held no longer once the owner took it.
    pub(in crate::daemon) fn offer(&mut self, owner: &mut impl MarkEntry, at: SystemTime) {
        if self.held.is_some_and(|held| taken(owner, at, held.edge())) {
            self.held = None;
        }
    }

    /// Drops the held edge: the call of a delete of everything, before it
    /// removes anything.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the delete call site of the capture run calls it")
    )]
    pub(in crate::daemon) fn drop_held(&mut self) {
        self.held = None;
    }
}

/// Offers the owner one record with `edge` at `at`. A record that the
/// constructor refuses as too long, which neither edge gives, reads as not
/// taken.
fn taken(owner: &mut impl MarkEntry, at: SystemTime, edge: RunEdge) -> bool {
    let mut p = serde_json::Map::new();
    p.insert("edge".to_owned(), edge.as_str().into());
    RunMark::new(at, p).is_ok_and(|mark| owner.take_mark(mark))
}

static MARKS: Mutex<Marks> = Mutex::new(Marks::new());

/// The one handler of the process, held for the length of one call. The pass
/// of the owner offers from it; the dispatch of a mark request and the call
/// site of a delete of everything reach it here.
pub(in crate::daemon) fn handler() -> MutexGuard<'static, Marks> {
    MARKS.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use halvo_control::r2::RunEdge;

    use super::Marks;
    use crate::daemon::maintenance::Clocks;
    use crate::daemon::model_store::{noon, Harness};
    use crate::daemon::owner::{Answer, Command};
    use crate::floor::record::{DecodedPayload, DecodedRecord};

    fn resumed(harness: &mut Harness) {
        assert!(matches!(
            harness.ask(Command::ClosePause),
            Answer::Resumed(_)
        ));
    }

    /// The records that the model holds after a settle, each a mark: its
    /// edge and its instant.
    fn marks_stored(harness: &mut Harness) -> Vec<(String, i64)> {
        assert!(matches!(harness.ask(Command::Settle), Answer::Settled(_)));
        harness
            .store
            .records()
            .iter()
            .map(edge_and_instant)
            .collect()
    }

    fn edge_and_instant(record: &DecodedRecord) -> (String, i64) {
        assert_eq!(record.kind(), "run_mark");
        assert_eq!(record.app(), None);
        let DecodedPayload::RunMark(p) = record.payload() else {
            panic!("a record of the kind run_mark holds another payload");
        };
        assert_eq!(p.len(), 1, "the payload holds a key beside edge");
        let edge = p.get("edge").and_then(|edge| edge.as_str());
        (edge.expect("no edge").to_owned(), record.at())
    }

    fn nanos(at: SystemTime) -> i64 {
        crate::event::unix_nanos(at)
    }

    #[test]
    fn start_then_end_in_order() {
        let mut harness = Harness::new();
        let mut marks = Marks::new();
        resumed(&mut harness);
        let end = noon() + Duration::from_secs(1);
        assert!(marks.request(&mut harness.owner, noon(), RunEdge::Start));
        assert!(marks.request(&mut harness.owner, end, RunEdge::End));
        assert_eq!(
            marks_stored(&mut harness),
            [
                ("start".to_owned(), nanos(noon())),
                ("end".to_owned(), nanos(end))
            ]
        );
    }

    #[test]
    fn a_paused_run_leaves_nothing() {
        let mut harness = Harness::new();
        let mut marks = Marks::new();
        assert!(!marks.request(&mut harness.owner, noon(), RunEdge::Start));
        assert!(!marks.request(&mut harness.owner, noon(), RunEdge::End));
        resumed(&mut harness);
        marks.offer(&mut harness.owner, noon());
        assert!(marks_stored(&mut harness).is_empty());
    }

    #[test]
    fn a_held_start_is_stored_later() {
        let mut harness = Harness::new();
        let mut marks = Marks::new();
        assert!(!marks.request(&mut harness.owner, noon(), RunEdge::Start));
        assert!(!marks.request(&mut harness.owner, noon(), RunEdge::Start));
        harness.clocks.advance(Duration::from_secs(60));
        resumed(&mut harness);
        let resume = harness.clocks.wall();
        let offer = resume + Duration::from_secs(1);
        marks.offer(&mut harness.owner, offer);
        marks.offer(&mut harness.owner, offer + Duration::from_secs(1));
        let stored = marks_stored(&mut harness);
        assert_eq!(stored, [("start".to_owned(), nanos(offer))]);
        assert!(stored[0].1 >= nanos(resume));
    }

    #[test]
    fn a_delete_drops_the_held_start() {
        let mut harness = Harness::new();
        let mut marks = Marks::new();
        assert!(!marks.request(&mut harness.owner, noon(), RunEdge::Start));
        marks.drop_held();
        resumed(&mut harness);
        marks.offer(&mut harness.owner, noon());
        assert!(marks_stored(&mut harness).is_empty());
    }
}
