use halvo_secrets::{Secret, MAX_SECRET_LEN};
use std::marker::PhantomData;

struct Probe<T>(PhantomData<T>);

trait Fallback {
    const DEBUG: bool = false;
    const DISPLAY: bool = false;
    const CLONE: bool = false;
}

impl<T> Fallback for Probe<T> {}

impl<T: std::fmt::Debug> Probe<T> {
    const DEBUG: bool = true;
}

impl<T: std::fmt::Display> Probe<T> {
    const DISPLAY: bool = true;
}

impl<T: Clone> Probe<T> {
    const CLONE: bool = true;
}

fn traits(debug: bool, display: bool, clone: bool) -> [bool; 3] {
    std::hint::black_box([debug, display, clone])
}

#[test]
fn a_secret_has_no_debug_no_display_and_no_clone() {
    let found = traits(
        Probe::<Secret>::DEBUG,
        Probe::<Secret>::DISPLAY,
        Probe::<Secret>::CLONE,
    );
    assert_eq!(found, [false, false, false]);
}

#[test]
fn the_probe_sees_debug_display_and_clone_where_they_are() {
    let found = traits(
        Probe::<Vec<u8>>::DEBUG,
        Probe::<String>::DISPLAY,
        Probe::<Vec<u8>>::CLONE,
    );
    assert_eq!(found, [true, true, true]);
}

#[test]
fn the_largest_secret_is_16_kib() {
    assert_eq!(MAX_SECRET_LEN, 16384);
}

#[test]
fn a_secret_takes_one_to_the_largest_number_of_bytes() {
    for len in [1, 2, 64, 100, MAX_SECRET_LEN - 1, MAX_SECRET_LEN] {
        let bytes: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let Some(secret) = Secret::new(bytes.clone()) else {
            panic!("a secret of {len} bytes was refused");
        };
        assert!(
            secret.expose() == bytes.as_slice(),
            "the {len} bytes read back differ"
        );
    }
}

#[test]
fn a_secret_refuses_no_bytes_and_one_byte_too_many() {
    assert!(Secret::new(Vec::new()).is_none());
    assert!(Secret::new(vec![b'x'; MAX_SECRET_LEN + 1]).is_none());
    assert!(Secret::new(vec![b'x'; MAX_SECRET_LEN * 4]).is_none());
}
