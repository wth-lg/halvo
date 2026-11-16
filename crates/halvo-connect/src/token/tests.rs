use super::{Token, TokenError};

const FORM: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~+/";

#[test]
fn debug_prints_no_value() {
    let token = Token::new("unit-value.7".to_owned()).expect("a b64token is a token");
    assert_eq!(format!("{token:?}"), "Token(..)");
    assert_eq!(format!("{token:#?}"), "Token(..)");
}

#[test]
fn value_is_kept_whole() {
    let token = Token::new("a.b-c_d~e+f/g==".to_owned()).expect("a b64token is a token");
    assert_eq!(token.value(), "a.b-c_d~e+f/g==");
}

#[test]
fn empty_value_is_refused() {
    assert_eq!(Token::new(String::new()).err(), Some(TokenError::Empty));
}

#[test]
fn every_byte_of_the_form_and_no_other() {
    assert!(Token::new(FORM.to_owned()).is_ok());
    for byte in 0u8..=127 {
        let c = char::from(byte);
        let taken = FORM.contains(c) || c == '=';
        assert_eq!(Token::new(format!("a{c}")).is_ok(), taken, "byte {byte}");
    }
    for value in ["é", "aé", "\u{2028}"] {
        assert_eq!(Token::new(value.to_owned()).err(), Some(TokenError::Form));
    }
}

#[test]
fn padding_only_at_the_end() {
    assert!(Token::new("ab==".to_owned()).is_ok());
    for value in ["=", "==", "a=b", "=a", "a==b="] {
        assert_eq!(
            Token::new(value.to_owned()).err(),
            Some(TokenError::Form),
            "{value}"
        );
    }
}

#[test]
fn error_texts() {
    assert_eq!(TokenError::Empty.to_string(), "the token is empty");
    assert_eq!(
        TokenError::Form.to_string(),
        "the token is not a b64token of RFC 6750"
    );
}
