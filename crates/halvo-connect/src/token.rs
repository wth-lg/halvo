use std::fmt;

/// An api token for one call, as the frame `secret` gives it (A6.2, A8.6). The adapter keeps none: each call takes
/// one by reference and puts it in the `Authorization` header of its requests alone. The value is never
/// formatted: `Debug` prints `Token(..)`, and the type has no `Display`, no `Clone` and no serialization.
///
/// ```compile_fail
/// fn copy(token: &halvo_connect::Token) -> halvo_connect::Token {
///     token.clone()
/// }
/// ```
///
/// ```compile_fail
/// fn show(token: &halvo_connect::Token) -> String {
///     format!("{token}")
/// }
/// ```
///
/// ```
/// fn show(token: &halvo_connect::Token) -> String {
///     format!("{token:?}")
/// }
/// let token = halvo_connect::Token::new("abc".to_owned()).unwrap();
/// assert_eq!(show(&token), "Token(..)");
/// ```
pub struct Token(String);

/// Why a value is not a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenError {
    /// The value is empty.
    Empty,
    /// The value is not a `b64token` of RFC 6750: letters, digits, `-`, `.`, `_`, `~`, `+` and `/`, at least one
    /// of them, then `=` only.
    Form,
}

impl Token {
    /// The token whose value is `value`, the `value` of a `secret_reply` with the answer `stored`. Refuses an empty
    /// value with `Empty` and a value that is not a `b64token` with `Form`, so that every token fits an
    /// `Authorization` header as it stands.
    pub fn new(value: String) -> Result<Token, TokenError> {
        if value.is_empty() {
            return Err(TokenError::Empty);
        }
        let body = value.trim_end_matches('=');
        if body.is_empty() || !body.bytes().all(token_byte) {
            return Err(TokenError::Form);
        }
        Ok(Token(value))
    }

    pub(crate) fn value(&self) -> &str {
        &self.0
    }
}

fn token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'+' | b'/')
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TokenError::Empty => "the token is empty",
            TokenError::Form => "the token is not a b64token of RFC 6750",
        })
    }
}

impl std::error::Error for TokenError {}

#[cfg(test)]
mod tests;
