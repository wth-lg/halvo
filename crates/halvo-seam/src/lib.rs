//! The read door between the capture store and the analysis: the capture record as a reader sees it, the
//! messages of the seam protocol with their framing, the client, the fixture transport over batch files, the
//! input-set reader with the reduction, and the conformance suite that every server of the protocol passes.

pub mod frame;
pub mod message;
pub mod record;
