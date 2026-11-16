mod common;

mod dir_swapped_while_waiting {
    use std::fs;
    use std::path::Path;
    use std::thread;
    use std::time::Duration;

    use super::common::{link_dir, Scratch, RUN};
    use halvo_checkpoint::{checkpoint_name, remove, Expect, Locks, Removal, Row, RunId, Sha256};

    #[test]
    fn a_directory_swapped_for_a_link_while_remove_waits_for_the_lock_is_never_followed_outside() {
        let scratch = Scratch::new("remove-dir-swap");
        let run = RunId::parse(RUN).expect("a run id");
        let name = checkpoint_name(Path::new("book.xlsx"), &run).expect("a checkpoint name");
        let name = name.to_str().expect("a plain name").to_owned();
        let bytes = b"the bytes of a checkpoint".to_vec();

        fs::create_dir_all(scratch.path("data")).expect("make the data directory");
        let locks =
            Locks::open(&scratch.path("data"), Duration::from_secs(10)).expect("open the locks");

        scratch.write(&format!("folder/sub/{name}"), &bytes);
        let outside = scratch.write(&format!("outside/{name}"), &bytes);

        let folder = scratch.path("folder");
        let sub = scratch.path("folder/sub");

        let held = locks.hold(&sub).expect("hold the lock of the directory");

        let worker = {
            let locks = locks.clone();
            let folder = folder.clone();
            let run = run.clone();
            let bytes = bytes.clone();
            thread::spawn(move || {
                let row = Row {
                    folder: &folder,
                    workbook: Path::new("sub/book.xlsx"),
                    run: &run,
                    expect: Expect::Made {
                        sha256: Sha256::of(&bytes),
                        bytes: bytes.len() as u64,
                    },
                };
                remove(&locks, &row)
            })
        };

        thread::sleep(Duration::from_millis(500));
        fs::rename(&sub, scratch.path("folder/sub-before")).expect("move the directory aside");
        link_dir(&scratch.path("outside"), &sub);
        drop(held);

        let answer = worker.join().expect("the remove thread");
        assert!(
            outside.exists(),
            "remove() reached a file outside the row's folder through a link on the way and \
             answered {answer:?}"
        );
        assert_ne!(
            answer,
            Ok(Removal::Removed),
            "the file it removed was outside the folder"
        );
    }
}

#[cfg(unix)]
mod written_in_place_while_read {
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::FileExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::common::{Scratch, RUN};
    use halvo_checkpoint::{checkpoint_name, remove, Expect, Locks, Removal, Row, RunId, Sha256};

    #[test]
    fn a_checkpoint_written_in_place_while_remove_reads_it_is_left() {
        let scratch = Scratch::new("remove-write-in-place");
        let run = RunId::parse(RUN).expect("a run id");
        let name = checkpoint_name(Path::new("book.xlsx"), &run).expect("a checkpoint name");
        let name = name.to_str().expect("a plain name").to_owned();

        fs::create_dir_all(scratch.path("data")).expect("make the data directory");
        let locks =
            Locks::open(&scratch.path("data"), Duration::from_secs(10)).expect("open the locks");

        let bytes: Vec<u8> = (0..4u32 << 20).map(|i| (i % 251) as u8).collect();
        let sha256 = Sha256::of(&bytes);

        for (round, delay_us) in [
            0u64, 200, 500, 1000, 2000, 3000, 5000, 8000, 12000, 20000, 40000,
        ]
        .into_iter()
        .enumerate()
        {
            let folder = scratch.path(&format!("folder-{round}"));
            let path = scratch.write(&format!("folder-{round}/{name}"), &bytes);
            let writer_file = OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("open the checkpoint for writing");

            let go = Arc::new(AtomicBool::new(false));
            let writer = {
                let go = go.clone();
                let path = path.clone();
                thread::spawn(move || {
                    while !go.load(Ordering::Acquire) {
                        std::hint::spin_loop();
                    }
                    let start = Instant::now();
                    while start.elapsed() < Duration::from_micros(delay_us) {
                        std::hint::spin_loop();
                    }
                    writer_file
                        .write_at(b"X", 0)
                        .expect("write into the checkpoint");
                    path.exists()
                })
            };

            let row = Row {
                folder: &folder,
                workbook: Path::new("book.xlsx"),
                run: &run,
                expect: Expect::Made {
                    sha256,
                    bytes: bytes.len() as u64,
                },
            };
            go.store(true, Ordering::Release);
            let answer = remove(&locks, &row);
            let written_before_removal = writer.join().expect("the writer thread");

            assert!(
                !(answer == Ok(Removal::Removed) && written_before_removal),
                "round {round} (delay {delay_us} us): the person changed the checkpoint in place \
                 before it was removed, and remove() answered {answer:?}"
            );
        }
    }
}

mod swapped_before_the_removal {
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::common::{Scratch, RUN};
    use halvo_checkpoint::{checkpoint_name, remove, Expect, Locks, Row, RunId, Sha256};

    #[test]
    fn a_file_moved_into_the_checkpoints_place_during_remove_is_never_removed() {
        let scratch = Scratch::new("remove-file-swap");
        let run = RunId::parse(RUN).expect("a run id");
        let name = checkpoint_name(Path::new("book.xlsx"), &run).expect("a checkpoint name");
        let name = name.to_str().expect("a plain name").to_owned();

        fs::create_dir_all(scratch.path("data")).expect("make the data directory");
        let locks =
            Locks::open(&scratch.path("data"), Duration::from_secs(10)).expect("open the locks");

        let ours = vec![b'c'; 4096];
        let mine = vec![b'p'; 4096];
        let expect = Expect::Made {
            sha256: Sha256::of(&ours),
            bytes: ours.len() as u64,
        };

        let mut took = Vec::new();
        for warm in 0..21 {
            let folder = scratch.path(&format!("warm-{warm}"));
            scratch.write(&format!("warm-{warm}/{name}"), &ours);
            let row = Row {
                folder: &folder,
                workbook: Path::new("book.xlsx"),
                run: &run,
                expect,
            };
            let start = Instant::now();
            let _ = remove(&locks, &row);
            took.push(start.elapsed());
        }
        took.sort();
        let typical = took[took.len() / 2];

        for round in 0..200u32 {
            let folder = scratch.path(&format!("round-{round}"));
            let checkpoint = scratch.write(&format!("round-{round}/{name}"), &ours);
            let person = scratch.write(&format!("round-{round}/mine.xlsx"), &mine);

            let delay = typical.mul_f64(0.5 + 0.6 * f64::from(round) / 200.0);
            let go = Arc::new(AtomicBool::new(false));
            let mover = {
                let go = go.clone();
                let checkpoint = checkpoint.clone();
                thread::spawn(move || {
                    while !go.load(Ordering::Acquire) {
                        std::hint::spin_loop();
                    }
                    let start = Instant::now();
                    while start.elapsed() < delay {
                        std::hint::spin_loop();
                    }
                    fs::rename(&person, &checkpoint).expect("move the person's file into place");
                })
            };

            let row = Row {
                folder: &folder,
                workbook: Path::new("book.xlsx"),
                run: &run,
                expect,
            };
            go.store(true, Ordering::Release);
            let answer = remove(&locks, &row);
            mover.join().expect("the mover thread");

            let now = fs::read(&checkpoint);
            assert!(
                matches!(&now, Ok(bytes) if *bytes == mine),
                "round {round}: the person's file, moved into the checkpoint's place during \
                 remove(), is gone; remove() answered {answer:?}"
            );
        }
    }
}

#[cfg(unix)]
mod link_in_place_of_agents_locks {
    use std::fs;
    use std::time::Duration;

    use super::common::{link_dir, Scratch};
    use halvo_checkpoint::{LockError, Locks};

    #[test]
    fn a_link_in_the_place_of_agents_locks_gives_no_second_holder_the_lock_of_one_directory() {
        let scratch = Scratch::new("lock-dir-link");
        let data = scratch.path("data");
        fs::create_dir_all(&data).expect("make the data directory");
        let folder = scratch.path("folder");
        fs::create_dir_all(&folder).expect("make the workbook's directory");
        let locks = Locks::open(&data, Duration::from_millis(200)).expect("the locks open");
        let first = locks
            .hold(&folder)
            .expect("the first holder takes the lock");

        let agents = data.join("agents");
        fs::rename(agents.join("locks"), agents.join("locks-moved")).expect("move agents/locks");
        let fresh = scratch.path("elsewhere");
        fs::create_dir_all(&fresh).expect("make another directory");
        link_dir(&fresh, &agents.join("locks"));

        let second = locks.hold(&folder);
        let took = second.is_ok();
        let made_elsewhere = scratch.names("elsewhere");
        assert!(
            !took,
            "a second holder took the lock of one directory while the first held it, through a link \
             in the place of agents/locks (lock files made in the link's target: {made_elsewhere:?})"
        );
        assert_eq!(second.err(), Some(LockError::Link));
        drop(first);
    }

    #[test]
    fn a_second_locks_of_the_same_data_directory_gets_no_lock_through_a_link_in_place_of_agents_locks(
    ) {
        let scratch = Scratch::new("lock-dir-link-two");
        let data = scratch.path("data");
        fs::create_dir_all(&data).expect("make the data directory");
        let folder = scratch.path("folder");
        fs::create_dir_all(&folder).expect("make the workbook's directory");
        let one = Locks::open(&data, Duration::from_millis(200)).expect("the first locks open");
        let two = Locks::open(&data, Duration::from_millis(200)).expect("the second locks open");
        let first = one.hold(&folder).expect("the first holder takes the lock");

        let agents = data.join("agents");
        fs::rename(agents.join("locks"), agents.join("locks-moved")).expect("move agents/locks");
        let fresh = scratch.path("elsewhere");
        fs::create_dir_all(&fresh).expect("make another directory");
        link_dir(&fresh, &agents.join("locks"));

        let second = two.hold(&folder);
        assert!(
            second.is_err(),
            "the second Locks took the lock of a directory that the first Locks holds: {second:?}"
        );
        drop(first);
    }
}

#[cfg(unix)]
mod agents_locks_replaced_by_a_directory {
    use std::fs;
    use std::time::Duration;

    use super::common::Scratch;
    use halvo_checkpoint::Locks;

    fn setup(name: &str) -> (Scratch, Locks, std::path::PathBuf) {
        let scratch = Scratch::new(name);
        let data = scratch.path("data");
        fs::create_dir_all(&data).expect("make the data directory");
        let folder = scratch.path("folder");
        fs::create_dir_all(&folder).expect("make the workbook's directory");
        let locks = Locks::open(&data, Duration::from_millis(200)).expect("the locks open");
        (scratch, locks, folder)
    }

    #[test]
    fn agents_locks_replaced_by_a_directory_while_held_gives_no_second_holder_the_lock() {
        let (scratch, locks, folder) = setup("lock-dir-replaced");
        let first = locks
            .hold(&folder)
            .expect("the first holder takes the lock");
        let agents = scratch.path("data/agents");
        fs::rename(agents.join("locks"), agents.join("locks-moved")).expect("move agents/locks");
        fs::create_dir(agents.join("locks")).expect("make a new agents/locks");

        let second = locks.hold(&folder);
        let made = scratch.names("data/agents/locks");
        assert!(
            second.is_err(),
            "a second holder took the lock of a directory that the first still holds, through a \
             lock file in the new agents/locks: {made:?}"
        );
        drop(first);
    }
}

#[cfg(windows)]
mod agents_locks_held_on_windows {
    use std::fs;
    use std::time::Duration;

    use super::common::{link_dir, Scratch};
    use halvo_checkpoint::{LockError, Locks};

    #[test]
    fn agents_locks_cannot_be_moved_while_a_lock_is_held_and_a_link_there_is_refused_after() {
        let scratch = Scratch::new("races-windows-agents-locks");
        let data = scratch.path("data");
        fs::create_dir_all(&data).expect("make the data directory");
        let folder = scratch.path("folder");
        fs::create_dir_all(&folder).expect("make the workbook's directory");
        let locks = Locks::open(&data, Duration::from_millis(200)).expect("the locks open");
        let held = locks
            .hold(&folder)
            .expect("the first holder takes the lock");
        let agents = data.join("agents");
        let moved = fs::rename(agents.join("locks"), agents.join("locks-moved"));
        assert!(
            moved.is_err(),
            "the system keeps agents/locks while a lock in it is held"
        );
        assert_eq!(locks.hold(&folder).err(), Some(LockError::Busy));
        drop(held);
        fs::rename(agents.join("locks"), agents.join("locks-moved")).expect("move once free");
        let fresh = scratch.path("elsewhere");
        fs::create_dir_all(&fresh).expect("make another directory");
        link_dir(&fresh, &agents.join("locks"));
        assert_eq!(locks.hold(&folder).err(), Some(LockError::Link));
        assert!(scratch.names("elsewhere").is_empty());
    }
}
