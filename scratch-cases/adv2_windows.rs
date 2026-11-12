#![cfg(windows)]

use halvo_control::paths::{pipe_name, DataDir, PathError, ScratchDir};

#[test]
fn adv2_w1_one_data_dir_has_one_pipe_name_before_and_after_it_exists() {
    let root = tempfile::tempdir().unwrap();
    let dir = DataDir::from_flag(root.path().join("Data")).unwrap();
    let sid = "S-1-5-21-1-2-3-1001";
    let before = pipe_name(sid, &dir);
    std::fs::create_dir(dir.path()).unwrap();
    let after = pipe_name(sid, &dir);
    assert_eq!(
        before,
        after,
        "the pipe name of {} changed when the directory was created",
        dir.path().display()
    );
}

#[test]
fn adv2_w2_scratch_dir_refuses_a_trailing_dot_alias_of_a_missing_data_dir() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::from_flag(root.path().join("data")).unwrap();
    let alias = root.path().join("data.");
    let got = ScratchDir::new(&alias, &data);
    if got.is_ok() {
        std::fs::create_dir(&alias).unwrap();
        println!(
            "after mkdir of the alias the data dir exists: {}",
            data.path().exists()
        );
    }
    assert_eq!(got, Err(PathError::InsideDataDir));
}

#[test]
fn adv2_w3_scratch_dir_refuses_an_admin_share_spelling_of_an_existing_data_dir() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::from_flag(root.path().join("data")).unwrap();
    std::fs::create_dir(data.path()).unwrap();
    let real = data.path().canonicalize().unwrap();
    let text = real.to_str().unwrap().trim_start_matches(r"\\?\").to_string();
    let (drive, rest) = text.split_once(':').unwrap();
    let unc = format!(r"\\localhost\{drive}${rest}");
    let reachable = std::path::Path::new(&unc).is_dir();
    let got = ScratchDir::new(&unc, &data);
    println!("{unc} reachable={reachable} got={got:?}");
    if reachable {
        assert_eq!(got, Err(PathError::InsideDataDir));
    }
}
