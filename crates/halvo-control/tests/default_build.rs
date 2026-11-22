//! A build of halvo-control that names no feature takes protocol version 2: the decoder hands a body of that
//! version to the server as unknown, where a build without the feature `r2` answers `unsupported_version`.

use halvo_control::proto::{decode_request, Decoded};

#[test]
fn a_build_with_no_feature_flag_takes_version_2() {
    let decoded = decode_request(br#"{"v":2,"body":{"op":"status","indicator":false}}"#);
    assert!(matches!(decoded, Ok(Decoded::Unknown(_))), "{decoded:?}");
}
