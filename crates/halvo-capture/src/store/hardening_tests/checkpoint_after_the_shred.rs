use std::path::Path;

use uuid::Uuid;

use super::super::db::{code_seams, random_segment_id, EncryptedStore};
use super::super::pages::{places_of, store_pages};
use super::super::segment::segment_path;
use super::super::{
    KindTotal, SegmentExpiry, SegmentWrite, StoreError, UtcDay, WriteFault, Written,
    DEFAULT_RETENTION,
};
use super::{file_root_store, open_over_file_root};

const NANOS_PER_DAY: i64 = 86_400 * 1_000_000_000;
const PAYLOAD: &[u8] = b"a batch whose shred commits before its checkpoint fails";
const TOTALS: [KindTotal<'static>; 1] = [KindTotal {
    kind: "keystroke",
    events: 3,
    bytes: 7,
}];
const IO_ERROR: i32 = halvo_sqlcipher::rusqlite::ffi::SQLITE_IOERR;

fn noon_today() -> i64 {
    let now = crate::event::unix_nanos(std::time::SystemTime::now());
    now.div_euclid(NANOS_PER_DAY) * NANOS_PER_DAY + NANOS_PER_DAY / 2
}

fn write(store: &mut EncryptedStore, id: &Uuid, first: i64) -> Result<Written, WriteFault> {
    store.write_batch(SegmentWrite {
        segment_id: id,
        day: UtcDay::of_instant(first),
        first_capture: first,
        last_capture: first,
        kinds: &TOTALS,
        payload: PAYLOAD,
    })
}

fn wrapped_key_of(store: &EncryptedStore, id: &Uuid) -> Vec<u8> {
    store
        .db()
        .unwrap()
        .connection()
        .query_row(
            "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
            [id.simple().to_string()],
            |r| r.get(0),
        )
        .unwrap()
}

fn in_the_pages(store_dir: &Path, key: &[u8; 32], wrapped: &[u8]) -> Vec<(&'static str, u32)> {
    places_of(&store_pages(store_dir, key), &[wrapped]).remove(0)
}

fn the_checkpoint_failed(e: &StoreError) -> bool {
    matches!(e, StoreError::Db(code) if *code == IO_ERROR)
}

#[test]
fn an_open_whose_reconcile_shred_commits_and_whose_checkpoint_then_fails_does_not_answer_ok_and_the_next_open_leaves_clean_pages(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let id = random_segment_id().unwrap();
    let (key, wrapped, inventory_before) = {
        let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
        write(&mut store, &id, noon_today()).unwrap();
        let key = *store.database_key().unwrap();
        let wrapped = wrapped_key_of(&store, &id);
        (key, wrapped, store.inventory().unwrap())
    };
    std::fs::remove_file(segment_path(&store_dir, id)).unwrap();
    code_seams::force_at(code_seams::CHECKPOINT_QUERY, Some(IO_ERROR));
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
    code_seams::force_at(code_seams::CHECKPOINT_QUERY, None);
    let answer = match &opened {
        Ok(store) => format!(
            "Ok({:?}) inventory={:?}",
            store.reconciled(),
            store.inventory()
        ),
        Err(e) => format!("Err({e:?})"),
    };
    let refused = matches!(&opened, Err(e) if the_checkpoint_failed(e));
    drop(opened);
    let places_after_the_refusal = in_the_pages(&store_dir, &key, &wrapped);
    let reopened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let places = in_the_pages(&store_dir, &key, &wrapped);
    println!(
        "S1-open inventory_before={inventory_before:?} answer={answer} places_after_the_refusal={places_after_the_refusal:?} next_open reconciled={:?} inventory={:?} places_of_the_wrapped_key={places:?}",
        reopened.reconciled(),
        reopened.inventory()
    );
    assert!(
        refused,
        "an open whose shred's checkpoint failed answered {answer}"
    );
    assert!(
        places.is_empty(),
        "the next open answered Ok with the wrapped key in {places:?}"
    );
    assert!(
        reopened.inventory().unwrap().is_empty(),
        "the next open still lists the shredded segment"
    );
}

#[test]
fn a_pass_whose_shred_commits_and_whose_checkpoint_then_fails_answers_its_error_to_every_later_call_and_the_next_open_leaves_clean_pages(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let due = noon_today() - DEFAULT_RETENTION.as_nanos() as i64 - 2 * NANOS_PER_DAY;
    let id = random_segment_id().unwrap();
    let (key, wrapped) = {
        let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
        write(&mut store, &id, due).unwrap();
        (*store.database_key().unwrap(), wrapped_key_of(&store, &id))
    };
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    code_seams::force_at(code_seams::CHECKPOINT_QUERY, Some(IO_ERROR));
    let pass = store.crypto_shred_expired();
    code_seams::force_at(code_seams::CHECKPOINT_QUERY, None);
    let other = random_segment_id().unwrap();
    let write_after = write(&mut store, &other, noon_today());
    let read_after = store.read_segment(id, &mut Vec::new()).map(|_| ());
    let inventory_after = store.inventory();
    let pass_after = store.crypto_shred_expired();
    drop(store);
    let reopened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let places = in_the_pages(&store_dir, &key, &wrapped);
    println!(
        "S1-pass pass={pass:?} write_after={write_after:?} read_after={read_after:?} inventory_after={inventory_after:?} pass_after={pass_after:?} next_open reconciled={:?} places_of_the_wrapped_key={places:?}",
        reopened.reconciled()
    );
    assert!(
        matches!(&pass, Err(e) if the_checkpoint_failed(e)),
        "the pass answered {pass:?}"
    );
    assert!(
        !matches!(write_after, Ok(_) | Err(WriteFault::AlreadyWritten { .. })),
        "a later write on that handle answered {write_after:?}"
    );
    assert!(
        matches!(&read_after, Err(e) if the_checkpoint_failed(e)),
        "a later read on that handle answered {read_after:?}"
    );
    assert!(
        matches!(&inventory_after, Err(e) if the_checkpoint_failed(e)),
        "the inventory on that handle answered {inventory_after:?}"
    );
    assert!(
        matches!(&pass_after, Err(e) if the_checkpoint_failed(e)),
        "a later pass on that handle answered {pass_after:?}"
    );
    assert!(
        places.is_empty(),
        "the next open answered Ok with the wrapped key in {places:?}"
    );
}

#[test]
fn a_pass_with_nothing_due_whose_checkpoint_fails_keeps_nothing_so_a_later_write_and_pass_answer_ok(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    code_seams::force_at(code_seams::CHECKPOINT_QUERY, Some(IO_ERROR));
    let pass = store.crypto_shred_expired();
    code_seams::force_at(code_seams::CHECKPOINT_QUERY, None);
    let id = random_segment_id().unwrap();
    let write_after = write(&mut store, &id, noon_today());
    let pass_after = store.crypto_shred_expired();
    println!("S10 pass={pass:?} write_after={write_after:?} pass_after={pass_after:?}");
    assert!(
        matches!(&pass, Err(e) if the_checkpoint_failed(e)),
        "the seam did not reach the empty pass's checkpoint: the pass answered {pass:?}"
    );
    assert!(
        write_after.is_ok(),
        "a write after an empty pass whose checkpoint failed answered {write_after:?}"
    );
    assert!(
        pass_after.is_ok(),
        "a pass after an empty pass whose checkpoint failed answered {pass_after:?}"
    );
}
