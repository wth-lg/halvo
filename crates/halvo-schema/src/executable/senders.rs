use std::fmt;
use std::num::NonZeroUsize;

const ADDRESS_MAX: usize = 320;
const SPECIALS: &[u8] = b"\"(),:;<>[\\]";

/// Whether `a` and `b` are one address of the reply rule: the same bytes without regard to ASCII case. A byte
/// outside ASCII compares as it is. It answers one question, whether an address is on the person's list, the
/// account's address or a listed sender, and it is none of the byte-for-byte compares of a draft's recipient and
/// not the compare of `admit`. `ReplyRule::selects` of halvo-connect holds each address against another with it;
/// the runner's own call of A5.1 is not built yet:
///
/// ```
/// assert!(halvo_schema::executable::same_address("ann@people.test", "ANN@People.TEST"));
/// assert!(!halvo_schema::executable::same_address("élan@people.test", "ÉLAN@people.test"));
/// ```
pub fn same_address(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The senders of the slot `reply_senders` (CP-112): the text cut at each comma, each piece without the white
/// space that `char::is_whitespace` names at its ends, in the order of the text; white space inside a piece is
/// no address. Refuses the whole slot with the first fault in the order of the text, which names its piece by
/// its number counted from 1: an empty piece, so also an empty text; a piece that holds a byte outside ASCII; a
/// piece that is no address by the adapter's rule for a sender; and a piece that is one address with an earlier
/// piece by [`same_address`]. The adapter's rule for a sender takes a piece of at most 320 bytes with one `@`, at
/// least one byte on each side of it, every byte printable ASCII, no space and none of the eleven bytes
/// `"(),:;<>[\]`, and in the part after the `@` no dot at its start, none at its end and no two in a row. A part
/// after the `@` with no dot, and a dot at an end of the part before it, pass:
///
/// ```
/// use std::num::NonZeroUsize;
/// use halvo_schema::executable::{reply_senders, SendersError};
/// let piece = |n| NonZeroUsize::new(n).unwrap();
/// assert_eq!(
///     reply_senders("ann@people.test, bob@people.test"),
///     Ok(vec!["ann@people.test".to_owned(), "bob@people.test".to_owned()])
/// );
/// assert_eq!(
///     reply_senders("\tann@people.test\u{a0}"),
///     Ok(vec!["ann@people.test".to_owned()])
/// );
/// assert_eq!(
///     reply_senders("ann@people.test,"),
///     Err(SendersError::Empty(piece(2)))
/// );
/// assert_eq!(
///     reply_senders("Ann Berg"),
///     Err(SendersError::NotAnAddress(piece(1)))
/// );
/// assert_eq!(
///     reply_senders("bob@people.test."),
///     Err(SendersError::NotAnAddress(piece(1)))
/// );
/// assert_eq!(
///     reply_senders("bob@people.test, ännchen@people.test"),
///     Err(SendersError::NotAscii(piece(2)))
/// );
/// assert_eq!(
///     reply_senders("ann@people.test, ANN@people.test"),
///     Err(SendersError::Twice { earlier: piece(1), later: piece(2) })
/// );
/// ```
pub fn reply_senders(slot: &str) -> Result<Vec<String>, SendersError> {
    let mut senders: Vec<String> = Vec::new();
    for (index, piece) in slot.split(',').enumerate() {
        let number = NonZeroUsize::MIN.saturating_add(index);
        let piece = piece.trim_matches(char::is_whitespace);
        if piece.is_empty() {
            return Err(SendersError::Empty(number));
        }
        if !piece.is_ascii() {
            return Err(SendersError::NotAscii(number));
        }
        if !is_sender(piece) {
            return Err(SendersError::NotAnAddress(number));
        }
        if let Some(earlier) = senders
            .iter()
            .position(|sender| same_address(sender, piece))
        {
            return Err(SendersError::Twice {
                earlier: NonZeroUsize::MIN.saturating_add(earlier),
                later: number,
            });
        }
        senders.push(piece.to_owned());
    }
    Ok(senders)
}

/// Whether `text` passes the adapter's rule for a sender, which the doc of [`reply_senders`] states.
pub fn is_sender(text: &str) -> bool {
    text.len() <= ADDRESS_MAX
        && text
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !SPECIALS.contains(&byte))
        && text.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.is_empty()
                && !domain.contains('@')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains("..")
        })
}

/// Why the reader refused the slot `reply_senders`: the first fault, with the number of its piece counted from 1.
/// No variant holds text of the slot. The numbers are for the set-up's answer alone and go into no row, log line
/// or frame; `Display` prints none of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendersError {
    /// The piece is empty once the white space at its ends is trimmed.
    Empty(NonZeroUsize),
    /// The piece holds a byte outside ASCII, which Halvo does not take: a limit of Halvo, apart from
    /// `NotAnAddress`, so that the person is not told that they typed something wrong.
    NotAscii(NonZeroUsize),
    /// The piece is no address by the adapter's rule for a sender, which the doc of [`reply_senders`] states.
    NotAnAddress(NonZeroUsize),
    /// The piece `later` is one address with the earlier piece `earlier`.
    Twice {
        /// The number of the earlier piece.
        earlier: NonZeroUsize,
        /// The number of the later piece.
        later: NonZeroUsize,
    },
}

impl fmt::Display for SendersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SendersError::Empty(_) => "the list of senders holds an empty entry",
            SendersError::NotAscii(_) => {
                "an entry of the list of senders holds a character outside ASCII, which Halvo does not take"
            }
            SendersError::NotAnAddress(_) => "an entry of the list of senders is not an address",
            SendersError::Twice { .. } => "the list of senders names one address twice",
        })
    }
}

impl std::error::Error for SendersError {}

#[cfg(test)]
mod tests;
