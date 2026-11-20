use super::super::db::{database_key, wrapping_key};
use super::super::segment::unwrap_segment_key;
use super::{file_root_store, open_over_file_root};

#[test]
fn two_stores_derive_two_wrapping_keys_and_a_key_that_one_wrapped_does_not_unwrap_under_the_other()
{
    let dir = tempfile::tempdir().unwrap();
    let (a_parent, b_parent) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::create_dir(&a_parent).unwrap();
    std::fs::create_dir(&b_parent).unwrap();
    let (a_dir, a_roots, a_root, a_kept) = file_root_store(&a_parent);
    let (b_dir, b_roots, b_root, _) = file_root_store(&b_parent);
    let a = open_over_file_root(&a_dir, &a_roots, a_root).unwrap();
    let b = open_over_file_root(&b_dir, &b_roots, b_root).unwrap();
    let (a_wrapping, b_wrapping) = (a.wrapping_key().unwrap(), b.wrapping_key().unwrap());
    let wrapped: Vec<u8> = a
        .db()
        .unwrap()
        .connection()
        .query_row(
            "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
            [a_kept.simple().to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let under_its_own = unwrap_segment_key(&a_wrapping, a_kept, &wrapped).is_ok();
    let under_the_other = unwrap_segment_key(&b_wrapping, a_kept, &wrapped).is_ok();
    println!(
        "T3-wrapping-keys differ={} under_its_own={under_its_own} under_the_other={under_the_other}",
        *a_wrapping != *b_wrapping
    );
    assert!(
        *a_wrapping != *b_wrapping,
        "two stores derived one wrapping key"
    );
    assert!(
        under_its_own,
        "a segment key did not unwrap under its own store's wrapping key"
    );
    assert!(
        !under_the_other,
        "a segment key that one store wrapped unwrapped under the other store's wrapping key"
    );
}

/// The database key of the store key whose bytes are 0 to 31: the
/// XChaCha20-Poly1305 ciphertext of 32 zero bytes under that key with the
/// nonce `halvo-store-database-key`, computed by an XChaCha20 that shares no
/// code with the store.
const DATABASE_KEY_OF_0_TO_31: [u8; 32] = [
    0xf9, 0x9e, 0xcb, 0xcc, 0x16, 0x00, 0x45, 0x14, 0x7d, 0xd0, 0x34, 0x15, 0xf7, 0xc4, 0xee, 0x79,
    0xa3, 0x7d, 0x72, 0xc1, 0xc9, 0xec, 0x26, 0x62, 0x5d, 0xb7, 0x0d, 0xae, 0x94, 0xe9, 0xe3, 0xa8,
];

/// The wrapping key of the same store key, under the nonce
/// `halvo-store-segment-wrap`.
const WRAPPING_KEY_OF_0_TO_31: [u8; 32] = [
    0xb7, 0xd5, 0x2e, 0x55, 0x2d, 0x5e, 0x3e, 0x59, 0x38, 0xc9, 0x51, 0xf0, 0x21, 0xfa, 0x55, 0x99,
    0x79, 0x43, 0x87, 0xec, 0xcc, 0xab, 0x77, 0xda, 0x43, 0x97, 0xc0, 0x91, 0x25, 0xb8, 0x38, 0xb3,
];

#[test]
fn a_fixed_store_key_derives_a_database_key_and_a_wrapping_key_of_fixed_bytes() {
    let store_key = crate::custody::StoreKey::from_bytes(std::array::from_fn(|i| i as u8));
    let database = database_key(&store_key).unwrap();
    let wrapping = wrapping_key(&store_key).unwrap();
    println!(
        "S5 database={:02x?} wrapping={:02x?}",
        &database[..],
        &wrapping[..]
    );
    assert_eq!(
        *database, DATABASE_KEY_OF_0_TO_31,
        "the database key of the fixed store key"
    );
    assert_eq!(
        *wrapping, WRAPPING_KEY_OF_0_TO_31,
        "the wrapping key of the fixed store key"
    );
}
