/// A run id of the form of contracts.md A0.6: 32 lower-case hex digits. It names the checkpoint
/// and the prepared copies of a run.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RunId(String);

impl RunId {
    /// Takes 32 lower-case hex digits; `None` for any other text.
    pub fn parse(text: &str) -> Option<RunId> {
        let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
        (text.len() == 32 && text.bytes().all(hex)).then(|| RunId(text.to_string()))
    }

    /// The 32 digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
