// Temporary probe of verifier-128 at 51f7acf9 for the hosted windows job. It ships nowhere.
// Every store and every root lives in a tempfile::tempdir of the test. It prints names,
// lengths and answers only.

use super::*;

fn v128w_tree(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                out.push(format!("<{:?}>", e.kind()));
                return;
            }
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let kind = if meta.file_type().is_symlink() {
                "link"
            } else if meta.is_dir() {
                "dir"
            } else {
                "file"
            };
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let len = if meta.is_file() { meta.len() } else { 0 };
            out.push(format!("{rel}:{kind}:{len}"));
            if meta.is_dir() {
                walk(base, &path, out);
            }
        }
    }
    if std::fs::symlink_metadata(dir).is_err() {
        return vec!["<nothing at the name>".to_string()];
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn v128w_answer<T>(answer: &StoreResult<T>) -> String {
    match answer {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("{e:?} (display: {e})"),
    }
}

#[test]
fn v128w_a_create_in_a_directory_whose_name_is_not_unicode() {
    use std::os::windows::ffi::OsStringExt;
    for name in ["not-unicode", "the control, a plain name"] {
        let dir = tempfile::tempdir().unwrap();
        let store_dir = if name == "not-unicode" {
            let mut wide: Vec<u16> = "not-unicode-".encode_utf16().collect();
            wide.push(0xD800);
            dir.path().join(std::ffi::OsString::from_wide(&wide))
        } else {
            dir.path().join("plain-name")
        };
        let roots_dir = dir.path().join("roots-of-the-probe");
        let root_id = RootId::fresh().unwrap();
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
        println!("V128W {name:?} create={}", v128w_answer(&created));
        drop(created);
        println!(
            "V128W {name:?} after the create: the directory stands={} store_dir={:?} roots_dir={:?}",
            store_dir.is_dir(),
            v128w_tree(&store_dir),
            v128w_tree(&roots_dir)
        );
        let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
        println!("V128W {name:?} open={}", v128w_answer(&opened));
        drop(opened);
        let again = EncryptedStore::create(
            &store_dir,
            Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )),
            DEFAULT_RETENTION,
        );
        println!("V128W {name:?} a second create under the same root id={}", v128w_answer(&again));
        drop(again);
        let tail = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
        println!("V128W {name:?} tail={}", v128w_answer(&tail));
        println!(
            "V128W {name:?} after the tail: store_dir={:?} roots_dir={:?}",
            v128w_tree(&store_dir),
            v128w_tree(&roots_dir)
        );
    }
}
