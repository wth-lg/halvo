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
