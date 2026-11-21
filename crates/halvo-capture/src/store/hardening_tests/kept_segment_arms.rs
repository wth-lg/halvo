use std::os::unix::fs::PermissionsExt;

use super::super::segment::segment_path;
use super::super::Reconciled;
use super::{file_root_store, open_over_file_root};

enum AtTheShard {
    NoAccess,
    NoSearch,
    Link,
    File,
}

fn an_open_with_the_shard(how: AtTheShard) -> (Reconciled, i64, bool, bool) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, kept) = file_root_store(dir.path());
    let path = segment_path(&store_dir, kept);
    let shard = path.parent().unwrap().to_path_buf();
    let mode = std::fs::metadata(&shard).unwrap().permissions().mode() & 0o777;
    let aside = dir.path().join("shard-aside");
    let set =
        |m: u32| std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(m)).unwrap();
    match how {
        AtTheShard::NoAccess => set(0o000),
        AtTheShard::NoSearch => set(0o600),
        AtTheShard::Link => {
            std::fs::rename(&shard, &aside).unwrap();
            std::os::unix::fs::symlink(&aside, &shard).unwrap();
        }
        AtTheShard::File => {
            std::fs::rename(&shard, &aside).unwrap();
            std::fs::write(&shard, b"not a shard").unwrap();
        }
    }
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
    match how {
        AtTheShard::NoAccess | AtTheShard::NoSearch => set(mode),
        AtTheShard::Link | AtTheShard::File => {
            std::fs::remove_file(&shard).unwrap();
            std::fs::rename(&aside, &shard).unwrap();
        }
    }
    let store = opened.unwrap();
    let live: i64 = store
        .db()
        .unwrap()
        .connection()
        .query_row(
            "SELECT count(*) FROM segment_keys WHERE segment_id = ?1 AND wrapped_key IS NOT NULL",
            [kept.simple().to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let mut plain = Vec::new();
    let read = store.read_segment(kept, &mut plain).is_ok();
    (store.reconciled(), live, path.exists(), read)
}

fn assert_kept(name: &str, (reconciled, live, file, read): (Reconciled, i64, bool, bool)) {
    println!("T3-{name} reconciled={reconciled:?} live={live} file={file} read={read}");
    assert_eq!(
        reconciled.rows_shredded, 0,
        "{name}: the open shredded the row of a segment whose file stands"
    );
    assert_eq!(
        (live, file, read),
        (1, true, true),
        "{name}: the live row, the file and the read of the kept segment"
    );
}

#[test]
fn an_open_keeps_a_live_segment_whose_shard_it_cannot_open() {
    assert_kept(
        "shard-not-openable",
        an_open_with_the_shard(AtTheShard::NoAccess),
    );
}

#[test]
fn an_open_keeps_a_live_segment_whose_shard_it_cannot_search() {
    assert_kept(
        "shard-not-searchable",
        an_open_with_the_shard(AtTheShard::NoSearch),
    );
}

#[test]
fn an_open_keeps_a_live_segment_whose_shard_name_holds_a_link() {
    assert_kept(
        "shard-name-holds-a-link",
        an_open_with_the_shard(AtTheShard::Link),
    );
}

#[test]
fn an_open_keeps_a_live_segment_whose_shard_name_holds_a_file() {
    assert_kept(
        "shard-name-holds-a-file",
        an_open_with_the_shard(AtTheShard::File),
    );
}
