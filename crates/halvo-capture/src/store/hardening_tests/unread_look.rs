use std::io::ErrorKind;
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
const PAYLOAD: &[u8] = b"a batch whose look after the commit cannot read the log's entry";
const TOTALS: [KindTotal<'static>; 1] = [KindTotal {
    kind: "keystroke",
    events: 3,
    bytes: 7,
}];
const UNREAD: ErrorKind = ErrorKind::PermissionDenied;

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

fn unread(e: &StoreError) -> bool {
    matches!(e, StoreError::Io(kind) if *kind == UNREAD)
}

#[test]
fn a_write_whose_look_cannot_read_the_logs_entry_answers_io_keeps_its_file_and_the_next_open_holds_the_batch(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let id = random_segment_id().unwrap();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    code_seams::force_io_at(code_seams::LOG_ENTRY_READ, Some(UNREAD));
    let written = write(&mut store, &id, noon_today());
    code_seams::force_io_at(code_seams::LOG_ENTRY_READ, None);
    let file_kept = segment_path(&store_dir, id).exists();
    let other = random_segment_id().unwrap();
    let write_after = write(&mut store, &other, noon_today());
    let read_after = store.read_segment(id, &mut Vec::new()).map(|_| ());
    let inventory_after = store.inventory();
    let pass_after = store.crypto_shred_expired();
    drop(store);
    let file_after_the_close = segment_path(&store_dir, id).exists();
    let mut reopened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let mut payload = Vec::new();
    let read_next = reopened.read_segment(id, &mut payload).map(|_| ());
    let inventory_next = reopened.inventory();
    let retry = write(&mut reopened, &id, noon_today());
    println!(
        "S4-write written={written:?} file_kept={file_kept} write_after={write_after:?} read_after={read_after:?} inventory_after={inventory_after:?} pass_after={pass_after:?} file_after_the_close={file_after_the_close} next_open reconciled={:?} read={read_next:?} payload_same={} inventory={inventory_next:?} retry={retry:?}",
        reopened.reconciled(),
        payload == PAYLOAD
    );
    assert_eq!(
        written,
        Err(WriteFault::Io(UNREAD)),
        "the write answered {written:?}"
    );
    assert!(
        file_kept,
        "the write removed its file after the look could not read the log's entry"
    );
    assert_eq!(
        write_after,
        Err(WriteFault::Io(UNREAD)),
        "a later write on that handle answered {write_after:?}"
    );
    assert!(
        matches!(&read_after, Err(e) if unread(e)),
        "a later read on that handle answered {read_after:?}"
    );
    assert!(
        matches!(&inventory_after, Err(e) if unread(e)),
        "the inventory on that handle answered {inventory_after:?}"
    );
    assert!(
        matches!(&pass_after, Err(e) if unread(e)),
        "a pass on that handle answered {pass_after:?}"
    );
    assert!(file_after_the_close, "the close removed the file");
    assert!(
        read_next.is_ok() && payload == PAYLOAD,
        "the next open does not read the batch back: {read_next:?}"
    );
    assert_eq!(
        inventory_next
            .unwrap()
            .iter()
            .map(|row| (row.kind.as_str(), row.events, row.bytes))
            .collect::<Vec<_>>(),
        vec![("keystroke", 3, 7)],
        "the next open's inventory"
    );
    assert!(
        matches!(retry, Err(WriteFault::AlreadyWritten { .. })),
        "a retry after the next open answered {retry:?}"
    );
}

#[test]
fn a_pass_whose_look_cannot_read_the_logs_entry_answers_io_to_every_later_call_reports_no_deletion_and_the_next_open_leaves_clean_pages(
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
    code_seams::force_io_at(code_seams::LOG_ENTRY_READ, Some(UNREAD));
    let pass = store.crypto_shred_expired();
    code_seams::force_io_at(code_seams::LOG_ENTRY_READ, None);
    let pass_after = store.crypto_shred_expired();
    let read_after = store.read_segment(id, &mut Vec::new()).map(|_| ());
    let inventory_after = store.inventory();
    let write_after = write(&mut store, &random_segment_id().unwrap(), noon_today());
    drop(store);
    let file_after_the_close = segment_path(&store_dir, id).exists();
    let reopened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let places = in_the_pages(&store_dir, &key, &wrapped);
    let reconciled = reopened.reconciled();
    let inventory_next = reopened.inventory();
    println!(
        "S4-pass pass={pass:?} pass_after={pass_after:?} read_after={read_after:?} inventory_after={inventory_after:?} write_after={write_after:?} file_after_the_close={file_after_the_close} next_open reconciled={reconciled:?} inventory={inventory_next:?} file={} places_of_the_wrapped_key={places:?}",
        segment_path(&store_dir, id).exists()
    );
    assert!(
        matches!(&pass, Err(e) if unread(e)),
        "the pass answered {pass:?}"
    );
    assert!(
        matches!(&pass_after, Err(e) if unread(e)),
        "a later pass on that handle answered {pass_after:?}"
    );
    assert!(
        matches!(&read_after, Err(e) if unread(e)),
        "a later read on that handle answered {read_after:?}"
    );
    assert!(
        matches!(&inventory_after, Err(e) if unread(e)),
        "the inventory on that handle answered {inventory_after:?}"
    );
    assert_eq!(
        write_after,
        Err(WriteFault::Io(UNREAD)),
        "a later write on that handle answered {write_after:?}"
    );
    assert!(file_after_the_close, "the file left before the next open");
    assert_eq!(
        reconciled.rows_shredded, 0,
        "the next open counted the shred: {reconciled:?}"
    );
    assert!(
        places.is_empty(),
        "the next open answered Ok with the wrapped key in {places:?}"
    );
    assert!(
        !segment_path(&store_dir, id).exists(),
        "the next open left the segment's file"
    );
    assert!(
        inventory_next.unwrap().is_empty(),
        "the next open lists the shredded batch"
    );
}
