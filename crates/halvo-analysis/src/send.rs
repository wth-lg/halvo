//! The send of one batch to the owner of the derived store: one or more analysis writes on the owner's worker
//! channel, one frame for each request and one reply for each, the last write alone with the commit position.

use std::fmt;
use std::io::{self, Read, Write};

use halvo_schema_core::ids::{Position, RequestId, StoreId};
use halvo_schema_core::record::Episode;
use halvo_schema_core::request::{
    decode_reply, AnalysisRole, AnalysisWrite, ErrorName, PushRefused, ReplyResult,
};
use halvo_seam::frame::{read_frame, write_frame, FrameError, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};

/// Why a batch was not committed, as the log names it: a name and no text of the owner.
pub(crate) enum SendFault {
    /// The system gave no random bytes for a request id.
    Random,
    /// A read or a write of the owner's channel failed.
    Io(io::ErrorKind),
    /// The owner's channel ended before the reply or inside it.
    End,
    /// The reply's frame is empty or above the bound of a reply.
    Frame,
    /// The reply is no reply of the request sent: it does not decode, it names another request, or its result is
    /// not null.
    Reply,
    /// The worker's own write does not serialize.
    Write(ErrorName),
    /// The owner refused the write with this error name.
    Refused(ErrorName),
}

fn error_name(name: ErrorName) -> &'static str {
    match name {
        ErrorName::Declared(_) => "declared",
        named => named.as_str(),
    }
}

impl fmt::Display for SendFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendFault::Random => f.write_str("random"),
            SendFault::Io(kind) => write!(f, "io:{kind:?}"),
            SendFault::End => f.write_str("end"),
            SendFault::Frame => f.write_str("frame"),
            SendFault::Reply => f.write_str("reply"),
            SendFault::Write(name) => write!(f, "write:{}", error_name(*name)),
            SendFault::Refused(name) => write!(f, "refused:{}", error_name(*name)),
        }
    }
}

impl From<FrameError> for SendFault {
    fn from(e: FrameError) -> SendFault {
        match e {
            FrameError::End => SendFault::End,
            FrameError::Length(_) => SendFault::Frame,
            FrameError::Io(e) => SendFault::Io(e.kind()),
        }
    }
}

/// The worker's end of the owner's worker channel, for the batch of one store.
pub(crate) struct Sender<'a, R, W> {
    pub(crate) reader: R,
    pub(crate) writer: W,
    pub(crate) store: StoreId,
    /// Sixteen random bytes of the system, for a request id.
    pub(crate) random: &'a mut dyn FnMut() -> Option<[u8; 16]>,
}

impl<R: Read, W: Write> Sender<'_, R, W> {
    fn new_write(&mut self, role: &AnalysisRole) -> Result<AnalysisWrite, SendFault> {
        let id = RequestId::from_random((self.random)().ok_or(SendFault::Random)?);
        Ok(AnalysisWrite::new(role, id, self.store.clone()))
    }

    /// Sends one write and reads its reply, which must name the write and hold the null result.
    fn exchange(&mut self, write: &AnalysisWrite) -> Result<(), SendFault> {
        let bytes = write.to_bytes().map_err(SendFault::Write)?;
        write_frame(&mut self.writer, &bytes, CLIENT_FRAME_MAX)?;
        let frame = read_frame(&mut self.reader, SERVER_FRAME_MAX)?.ok_or(SendFault::End)?;
        let reply = decode_reply(&frame).map_err(|_| SendFault::Reply)?;
        if reply.id.as_ref() != Some(write.id()) {
            return Err(SendFault::Reply);
        }
        match reply.outcome {
            Ok(ReplyResult::Null) => Ok(()),
            Ok(_) => Err(SendFault::Reply),
            Err(name) => Err(SendFault::Refused(name)),
        }
    }

    /// Sends `episodes` as one batch and returns how many of them it sent. It fills a write with whole episodes in
    /// order and starts the next write when one more episode would pass the bound of a write. An episode that passes
    /// the bound alone is sent with steps cut from the end of its steps; one that does not fit with no step, and one
    /// that its reader would refuse, is not sent. Every write but the last has a null commit, and the last carries
    /// `commit`, so an owner whose channel ends before that write stores nothing of the batch.
    pub(crate) fn batch(
        &mut self,
        episodes: Vec<Episode>,
        commit: Position,
    ) -> Result<u64, SendFault> {
        let role = AnalysisRole::claim();
        let mut write = self.new_write(&role)?;
        let mut sent = 0;
        for episode in episodes {
            let mut next = Some(episode);
            while let Some(episode) = next.take() {
                match write.push(episode) {
                    Ok(()) => sent += 1,
                    Err(PushRefused::Malformed(_)) => {}
                    Err(PushRefused::TooLarge(back)) if write.episodes().is_empty() => {
                        if let Some(cut) = cut_to_fit(&role, &write, back) {
                            if write.push(cut).is_ok() {
                                sent += 1;
                            }
                        }
                    }
                    Err(PushRefused::TooLarge(back)) => {
                        self.exchange(&write)?;
                        write = self.new_write(&role)?;
                        next = Some(back);
                    }
                }
            }
        }
        write.commit(commit);
        self.exchange(&write)?;
        Ok(sent)
    }
}

/// The episode with the longest head of its steps that an empty write of `like`'s size takes, or none when it does
/// not fit with no step.
fn cut_to_fit(role: &AnalysisRole, like: &AnalysisWrite, mut episode: Episode) -> Option<Episode> {
    let steps = std::mem::take(&mut episode.steps);
    let fits = |count: usize| {
        let mut probe = AnalysisWrite::new(role, like.id().clone(), like.store().clone());
        let mut head = episode.clone();
        head.steps = steps[..count].to_vec();
        probe.push(head).is_ok()
    };
    if !fits(0) {
        return None;
    }
    let (mut fit, mut over) = (0, steps.len());
    while over - fit > 1 {
        let middle = fit + (over - fit) / 2;
        if fits(middle) {
            fit = middle;
        } else {
            over = middle;
        }
    }
    episode.steps = steps[..fit].to_vec();
    Some(episode)
}
