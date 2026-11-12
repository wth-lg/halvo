//! Test-only parts of the floor that the crate's tests share: the text filter
//! points of the tests.

use std::sync::Arc;

use super::exclude::{FloorHandle, Keep, NoLineage, PrivateWindow, ProcessId, Subject, Verdict};
use super::kind::Recorded;
use super::text::{write_through, CapturedText, TextField, TextFilterPoint};
use super::{Switch, Switches};
use crate::event::AppIdentity;
use crate::secure_input::{FixedProbe, SecureInputReading};

/// Every switch on.
pub(crate) fn all_switches_on() -> Switches {
    let mut switches = Switches::default();
    for switch in Switch::ALL {
        switches.set(switch, true);
    }
    switches
}

/// The keep verdict of the decision for an event of kind `K` of `app` in
/// process `pid`, with every switch on, no root and an inactive reading.
pub(crate) fn keep<K: Recorded>(app: AppIdentity, pid: u32) -> Keep<K> {
    let floor = FloorHandle::new(None, Arc::new(NoLineage), all_switches_on());
    let subject = Subject::new(app, ProcessId::new(pid));
    let probe = FixedProbe(SecureInputReading::Inactive);
    match floor.decide::<K>(subject, PrivateWindow::Unknown, &probe) {
        Verdict::Keep(keep) => keep,
        Verdict::Drop(reason) => panic!("the decision dropped the event: {reason:?}"),
    }
}

/// A point that stores every text unchanged.
pub(crate) struct IdentityPoint;

impl TextFilterPoint for IdentityPoint {
    type Stored = String;

    fn filter(&self, _field: TextField, text: &str) -> String {
        text.to_owned()
    }
}

/// The text that the encoder writes for `text` in `field` through
/// [`IdentityPoint`].
pub(crate) fn stored(text: &CapturedText, field: TextField) -> String {
    let mut out = String::new();
    let changed = write_through(text, field, &IdentityPoint, &mut out);
    assert!(!changed, "the identity point changed a text");
    serde_json::from_str(&out).expect("the writer wrote a JSON string")
}
