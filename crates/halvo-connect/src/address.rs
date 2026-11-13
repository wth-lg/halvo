use std::fmt;

const ADDRESS_MAX: usize = 320;
const SPECIALS: &[u8] = b"\"(),:;<>[\\]";

/// Whether `a` and `b` are one address of the reply rule: the same bytes without regard to ASCII case. A byte
/// outside ASCII compares as it is. It answers one question, whether an address is on the person's list, the
/// account's address or a listed sender, and it is none of the byte-for-byte compares of a draft's recipient and
/// not the compare of `admit`. `ReplyRule::selects` holds each address against another with it; the runner's own
/// call of A5.1 is not built yet:
///
/// ```
/// assert!(halvo_connect::same_address("ann@people.test", "ANN@People.TEST"));
/// assert!(!halvo_connect::same_address("élan@people.test", "ÉLAN@people.test"));
/// ```
pub fn same_address(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The senders of the slot `reply_senders` (CP-112): the text cut at each comma, each piece without the ASCII
/// spaces at its ends, in the order of the text. Refuses the whole slot for an empty piece, so also for an empty
/// text, for a piece that is no address by the adapter's rule for a sender, and for a piece that is one address
/// with an earlier piece by [`same_address`], with the first fault in the order of the text. The adapter's rule
/// for a sender takes a piece of at most 320 bytes with one `@`, at least one byte on each side of it, and every
/// byte printable ASCII, no space and none of the eleven bytes `"(),:;<>[\]`:
///
/// ```
/// use halvo_connect::{reply_senders, SendersError};
/// assert_eq!(
///     reply_senders("ann@people.test, bob@people.test"),
///     Ok(vec!["ann@people.test".to_owned(), "bob@people.test".to_owned()])
/// );
/// assert_eq!(reply_senders("ann@people.test,"), Err(SendersError::Empty));
/// assert_eq!(reply_senders("Ann Berg"), Err(SendersError::NotAnAddress));
/// assert_eq!(
///     reply_senders("ann@people.test, ANN@people.test"),
///     Err(SendersError::Twice)
/// );
/// ```
pub fn reply_senders(slot: &str) -> Result<Vec<String>, SendersError> {
    let mut senders: Vec<String> = Vec::new();
    for piece in slot.split(',') {
        let piece = piece.trim_matches(' ');
        if piece.is_empty() {
            return Err(SendersError::Empty);
        }
        if !is_sender(piece) {
            return Err(SendersError::NotAnAddress);
        }
        if senders.iter().any(|sender| same_address(sender, piece)) {
            return Err(SendersError::Twice);
        }
        senders.push(piece.to_owned());
    }
    Ok(senders)
}

pub(crate) fn is_sender(text: &str) -> bool {
    text.len() <= ADDRESS_MAX
        && text
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !SPECIALS.contains(&byte))
        && text.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        })
}

/// Why the reader refused the slot `reply_senders`. No variant holds text of the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendersError {
    /// A piece is empty once its ASCII spaces are trimmed.
    Empty,
    /// A piece is no address by the adapter's rule for a sender, which the doc of [`reply_senders`] states.
    NotAnAddress,
    /// A piece is one address with an earlier piece.
    Twice,
}

impl fmt::Display for SendersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SendersError::Empty => "the list of senders holds an empty entry",
            SendersError::NotAnAddress => "an entry of the list of senders is not an address",
            SendersError::Twice => "the list of senders names one address twice",
        })
    }
}

impl std::error::Error for SendersError {}

#[cfg(test)]
mod tests;
