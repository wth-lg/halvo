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
/// refused, an ASCII one as `NotAnAddress` and one outside ASCII, a no-break space or U+3000 among them, as
/// `NotAscii`. Refuses the whole slot with the first fault in the order of the text, which names its piece by
/// its number counted from 1: an empty piece, so also an empty text; a piece that holds a byte outside ASCII; a
/// piece that is no address by the adapter's rule for a sender; and a piece that is one address with an earlier
/// piece by [`same_address`]. The adapter's rule for a sender takes a piece of at most 320 bytes with one `@`, at
/// least one byte on each side of it, every byte printable ASCII, no space and none of the eleven bytes
/// `"(),:;<>[\]`, and in the part after the `@` no dot at its start, none at its end and no two in a row. A part
/// after the `@` with no dot, and a dot at an end of the part before it, pass. Its answer is [`Senders`], which
/// only this reader makes:
///
/// ```
/// use std::num::NonZeroUsize;
/// use halvo_schema::executable::{reply_senders, SendersError};
/// let piece = |n| NonZeroUsize::new(n).unwrap();
/// let senders = reply_senders("ann@people.test, bob@people.test").unwrap();
/// assert_eq!(
///     senders.iter().collect::<Vec<_>>(),
///     ["ann@people.test", "bob@people.test"]
/// );
/// let senders = reply_senders("\tann@people.test\u{a0}").unwrap();
/// assert_eq!(senders.iter().collect::<Vec<_>>(), ["ann@people.test"]);
/// assert_eq!(
///     reply_senders("ann@people.test,").err(),
///     Some(SendersError::Empty(piece(2)))
/// );
/// assert_eq!(
///     reply_senders("Ann Berg").err(),
///     Some(SendersError::NotAnAddress(piece(1)))
/// );
/// assert_eq!(
///     reply_senders("bob@people.test.").err(),
///     Some(SendersError::NotAnAddress(piece(1)))
/// );
/// assert_eq!(
///     reply_senders("bob@people.test, ännchen@people.test").err(),
///     Some(SendersError::NotAscii(piece(2)))
/// );
/// assert_eq!(
///     reply_senders("ann@people.test, ANN@people.test").err(),
///     Some(SendersError::Twice { earlier: piece(1), later: piece(2) })
/// );
/// ```
pub fn reply_senders(slot: &str) -> Result<Senders, SendersError> {
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
    Ok(Senders { list: senders })
}

/// The answer of [`reply_senders`]: the senders that it read from the slot, at least one, each as read, in the
/// order of the text. It has no public field and no public way to make it but the reader, so that no caller holds
/// senders that the reader never read. `Debug` prints their count alone. A struct literal, `Default`, `From`, a
/// collect, a parse and a deserialization do not compile:
///
/// ```compile_fail,E0451
/// let senders = halvo_schema::executable::Senders { list: vec!["ann berg@people.test".to_owned()] };
/// ```
///
/// ```compile_fail,E0599
/// let senders = halvo_schema::executable::Senders::default();
/// ```
///
/// ```compile_fail,E0308
/// let senders = halvo_schema::executable::Senders::from(vec!["ann berg@people.test".to_owned()]);
/// ```
///
/// ```compile_fail,E0277
/// let senders: halvo_schema::executable::Senders = vec!["ann berg@people.test".to_owned()].into();
/// ```
///
/// ```compile_fail,E0277
/// let senders: halvo_schema::executable::Senders = ["ann berg@people.test"].into_iter().collect();
/// ```
///
/// ```compile_fail,E0277
/// let senders = "ann berg@people.test".parse::<halvo_schema::executable::Senders>();
/// ```
///
/// ```compile_fail,E0277
/// let senders: halvo_schema::executable::Senders = serde_json::from_str("[\"ann berg@people.test\"]").unwrap();
/// ```
///
/// ```
/// let senders = halvo_schema::executable::reply_senders("ann@people.test").unwrap();
/// assert!(senders.holds("ANN@People.TEST"));
/// ```
#[derive(Clone)]
pub struct Senders {
    list: Vec<String>,
}

impl Senders {
    /// Each sender as the reader read it, in the order of the text.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.list.iter().map(String::as_str)
    }

    /// Whether `address` is one of the senders by [`same_address`].
    pub fn holds(&self, address: &str) -> bool {
        self.list.iter().any(|sender| same_address(sender, address))
    }
}

impl fmt::Debug for Senders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Senders")
            .field("count", &self.list.len())
            .finish_non_exhaustive()
    }
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
/// or frame; neither `Display` nor `Debug` prints one (CP-165 (5) (c)).
#[derive(Clone, Copy, PartialEq, Eq)]
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

impl fmt::Debug for SendersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SendersError::Empty(_) => "Empty(..)",
            SendersError::NotAscii(_) => "NotAscii(..)",
            SendersError::NotAnAddress(_) => "NotAnAddress(..)",
            SendersError::Twice { .. } => "Twice { .. }",
        })
    }
}

impl std::error::Error for SendersError {}

#[cfg(test)]
mod tests;
