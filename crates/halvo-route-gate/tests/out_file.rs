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
