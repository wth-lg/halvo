use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use halvo_route_gate::out::{write_new, Refused};

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    loop {
        let dir = base.join(format!(
            "route-gate-out-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
}

fn path(dir: &Path, name: &str) -> String {
    dir.join(name).to_str().expect("utf-8 path").to_owned()
}

#[test]
fn a_file_that_appears_at_out_before_the_link_keeps_its_bytes() {
    let dir = scratch();
    let out = path(&dir, "generated.tsv");
    let made = write_new(&out, "the table\n", || {
        std::fs::write(&out, "bytes of another call\n").expect("the other call writes");
    });
    assert_eq!(made, Err(Refused::Exists));
    assert_eq!(
        std::fs::read_to_string(&out).expect("kept"),
        "bytes of another call\n"
    );
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the scratch directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("utf-8")
        })
        .collect();
    names.sort();
    names
}

#[test]
fn the_directory_of_out_holds_no_side_name_after_a_call() {
    let dir = scratch();
    let written = path(&dir, "written.tsv");
    assert_eq!(write_new(&written, "the table\n", || {}), Ok(()));
    assert_eq!(
        std::fs::read_to_string(&written).expect("written"),
        "the table\n"
    );
    assert_eq!(names(&dir), ["written.tsv"]);

    let kept = path(&dir, "kept.tsv");
    std::fs::write(&kept, "bytes of another call\n").expect("fixture");
    assert_eq!(write_new(&kept, "the table\n", || {}), Err(Refused::Exists));
    assert_eq!(names(&dir), ["kept.tsv", "written.tsv"]);

    let late = path(&dir, "late.tsv");
    let made = write_new(&late, "the table\n", || {
        let during = names(&dir);
        assert_eq!(during.len(), 3, "{during:?}");
        std::fs::write(&late, "bytes of another call\n").expect("the other call writes");
    });
    assert_eq!(made, Err(Refused::Exists));
    assert_eq!(names(&dir), ["kept.tsv", "late.tsv", "written.tsv"]);

    let missing = dir.join("missing").join("out.tsv");
    assert_eq!(
        write_new(missing.to_str().expect("utf-8"), "the table\n", || {}),
        Err(Refused::Unwritable)
    );
    assert_eq!(names(&dir), ["kept.tsv", "late.tsv", "written.tsv"]);
}
