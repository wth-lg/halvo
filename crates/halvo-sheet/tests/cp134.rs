//! The cases of the coordinator's ruling CP-134 for this crate.
mod common;

/// CP-134 H1, the review's probe P7 in the crate's form: a FIFO named like a prepared copy of
/// the workbook stands beside it. The sweep opens only a regular file, so `prepare` answers
/// within the test's bound of 20 s and the FIFO stays. An open that followed the FIFO would
/// block while `prepare` holds the lock of the folder; after the bound the test opens the FIFO
/// for writing without blocking, which lets such an open go on, and fails.
#[cfg(unix)]
#[test]
fn the_sweep_opens_no_fifo_named_like_a_prepared_copy() {
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
    use std::path::Path;
    use std::process::Command;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use common::{corpus, Scratch, RUN};
    use halvo_sheet::{Folder, Locks, Range, RunId, Value};

    const O_NONBLOCK: i32 = if cfg!(target_os = "macos") {
        0x0004
    } else {
        0o4000
    };
    let scratch = Scratch::new("cp134-fifo");
    let data = Scratch::new("cp134-fifo-data");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let other = "fedcba9876543210fedcba9876543210";
    let fifo = format!("values.halvo-prepared-{other}-00000000000000ff-0123456789abcdef.xlsx");
    let made = Command::new("mkfifo")
        .arg(scratch.path(&fifo))
        .status()
        .expect("mkfifo runs");
    assert!(made.success(), "the FIFO is made");
    let locks = Locks::open(&data.dir, Duration::from_secs(10)).expect("the locks open");
    let folder = Folder::new(&scratch.dir, locks).expect("the folder opens");
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let run = RunId::parse(RUN).expect("a run id");
        let range = Range::parse("B5").expect("a range");
        let values = [vec![Value::Number(5.0)]];
        let answer = folder
            .prepare(Path::new("values.xlsx"), &run, "Values", range, &values)
            .map(drop);
        let _ = send.send(answer);
    });
    let answer = receive.recv_timeout(Duration::from_secs(20));
    if answer.is_err() {
        let _ = OpenOptions::new()
            .write(true)
            .custom_flags(O_NONBLOCK)
            .open(scratch.path(&fifo));
    }
    println!("cp134 fifo answer_within_20s={answer:?}");
    assert_eq!(answer, Ok(Ok(())), "prepare answered within the bound");
    let kind = fs::symlink_metadata(scratch.path(&fifo)).expect("the FIFO stays");
    assert!(kind.file_type().is_fifo(), "it is still a FIFO");
}
