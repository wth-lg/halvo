use std::fs;
use std::path::{Path, PathBuf};

use halvo_seam::fixture::{FixtureTransport, OpenError};
use halvo_seam::input_set::{InputSet, SetError};

const SET_MAX: usize = 1_024;

const APP_TITLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/r2/input-sets/app-title.toml"
));

fn new_dir(test: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    for n in 0..100_000 {
        let dir = base.join(format!("halvo-seam-{test}-{n}"));
        if fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
    panic!("no new directory under CARGO_TARGET_TMPDIR after 100000 names");
}

#[test]
fn the_set_reader_takes_at_most_1024_bytes_and_one_more() {
    let dir = new_dir("set_reader_bound");
    let at_bound = dir.join("at-bound.toml");
    let over = dir.join("over.toml");
    let mut text = APP_TITLE.to_owned();
    text.push_str(&"\n".repeat(SET_MAX - APP_TITLE.len()));
    fs::write(&at_bound, &text).unwrap();
    text.push('\n');
    fs::write(&over, &text).unwrap();
    let lines_in_bound = text.as_bytes()[..SET_MAX]
        .iter()
        .filter(|b| **b == b'\n')
        .count();
    match InputSet::read(&at_bound) {
        Err(SetError::Form { line, .. }) => assert_eq!(line, 6),
        other => panic!("{other:?}"),
    }
    match InputSet::read(&over) {
        Err(SetError::Form { line, .. }) => assert_eq!(line, lines_in_bound + 1),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_set_path_that_is_no_regular_file_is_unreadable() {
    let dir = new_dir("set_reader_directory");
    assert!(matches!(
        InputSet::read(&dir),
        Err(SetError::Unreadable { .. })
    ));
    let none: &[PathBuf] = &[];
    assert!(matches!(
        FixtureTransport::open(none, 1, Some(&dir)),
        Err(OpenError::InputSet(SetError::Unreadable { .. }))
    ));
}

#[test]
fn a_set_path_that_is_a_symbolic_link_is_unreadable_also_when_it_names_a_regular_file() {
    use std::io::{ErrorKind, Write};
    let dir = new_dir("set_reader_link");
    let file = dir.join("app-title.toml");
    fs::write(&file, APP_TITLE).unwrap();
    assert!(InputSet::read(&file).is_ok());
    let link = dir.join("link.toml");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&file, &link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&file, &link);
    if let Err(e) = made {
        writeln!(
            std::io::stderr(),
            "set_reader_link: no symbolic link could be made here ({:?}), so the link case did not run",
            e.kind()
        )
        .unwrap();
        return;
    }
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
            && fs::metadata(&link).unwrap().is_file()
    );
    match InputSet::read(&link) {
        Err(SetError::Unreadable { path, kind }) => {
            assert_eq!(path, link);
            assert_eq!(kind, ErrorKind::InvalidInput);
        }
        other => panic!("{other:?}"),
    }
    let none: &[PathBuf] = &[];
    assert!(matches!(
        FixtureTransport::open(none, 1, Some(&link)),
        Err(OpenError::InputSet(SetError::Unreadable { .. }))
    ));
}

#[cfg(unix)]
#[test]
fn the_set_reader_refuses_a_fifo_as_unreadable_without_waiting() {
    use std::sync::mpsc;
    use std::time::Duration;
    let dir = new_dir("set_reader_fifo");
    let fifo = dir.join("set.toml");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    let (tx, rx) = mpsc::channel();
    let path = fifo.clone();
    std::thread::spawn(move || {
        let read = InputSet::read(&path);
        let none: &[PathBuf] = &[];
        let opened = FixtureTransport::open(none, 1, Some(&path));
        let _ = tx.send((read, opened.err()));
    });
    let Ok((read, opened)) = rx.recv_timeout(Duration::from_secs(10)) else {
        let _ = fs::OpenOptions::new().write(true).open(&fifo);
        panic!("the set reader waited 10 s on a FIFO");
    };
    assert!(matches!(read, Err(SetError::Unreadable { .. })), "{read:?}");
    assert!(
        matches!(
            opened,
            Some(OpenError::InputSet(SetError::Unreadable { .. }))
        ),
        "{opened:?}"
    );
}

#[cfg(not(unix))]
#[test]
#[ignore = "skipped: this system has no FIFO"]
fn the_set_reader_refuses_a_fifo_as_unreadable_without_waiting() {}
