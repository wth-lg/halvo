use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{rows, start_time_of, walk, Hands, Owner, SystemHands};

const OWNER_VAR: &str = "HALVO_DELETE_WALK_OWNER";
const OWNER_LIFE: Duration = Duration::from_secs(120);
const THIS_TEST: &str = "daemon::delete_all::windows_tests::the_delete_walk_ends_a_running_owner_on_windows_before_any_unlink";

struct Watching<'a> {
    owner: &'a Mutex<Child>,
    unlinks: Mutex<Vec<bool>>,
}

impl Watching<'_> {
    fn note_an_unlink(&self) {
        let ended = self.owner.lock().unwrap().try_wait().unwrap().is_some();
        self.unlinks.lock().unwrap().push(ended);
    }

    fn take(&self) -> Vec<bool> {
        std::mem::take(&mut self.unlinks.lock().unwrap())
    }
}

impl Hands for Watching<'_> {
    fn end_owner(&self, owner: Owner) -> bool {
        SystemHands.end_owner(owner)
    }

    fn destroy_key_file(&self, data_dir: &Path, file: &str) -> bool {
        self.note_an_unlink();
        SystemHands.destroy_key_file(data_dir, file)
    }

    fn remove_entry(&self, data_dir: &Path, name: &str) -> u64 {
        self.note_an_unlink();
        SystemHands.remove_entry(data_dir, name)
    }
}

struct EndsItsOwnChild<'a>(&'a Mutex<Child>);

impl Drop for EndsItsOwnChild<'_> {
    fn drop(&mut self) {
        let mut child = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn a_derived_row(dir: &Path, pid: u32, started: u64) {
    let derived = dir.join("derived");
    std::fs::create_dir_all(&derived).unwrap();
    std::fs::write(derived.join("kept"), b"kept").unwrap();
    std::fs::write(
        derived.join("owner.pid"),
        format!("pid={pid} started={started}\n"),
    )
    .unwrap();
}

#[test]
fn the_delete_walk_ends_a_running_owner_on_windows_before_any_unlink() {
    if std::env::var_os(OWNER_VAR).is_some() {
        std::thread::sleep(OWNER_LIFE);
        return;
    }
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", THIS_TEST, "--nocapture", "--test-threads=1"])
        .env(OWNER_VAR, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the owner starts");
    let pid = child.id();
    let owner = Mutex::new(child);
    let _guard = EndsItsOwnChild(&owner);
    let until = Instant::now() + Duration::from_secs(10);
    let started = loop {
        if let Some(started) = start_time_of(pid) {
            break started;
        }
        assert!(Instant::now() < until, "no start time of the owner {pid}");
        std::thread::sleep(Duration::from_millis(50));
    };
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    let rows = rows();
    let hands = Watching {
        owner: &owner,
        unlinks: Mutex::new(Vec::new()),
    };

    a_derived_row(dir, pid, started + 1);
    let walked = walk(dir, &rows, true, &hands);
    assert_eq!(walked.left, 0, "case 1: {walked:?}");
    assert!(!dir.join("derived").exists(), "case 1: the row stands");
    assert_eq!(hands.take(), [false], "case 1: an unlink after an end");
    assert!(
        owner.lock().unwrap().try_wait().unwrap().is_none(),
        "case 1: a pid file with another start time ended the process"
    );

    a_derived_row(dir, pid, started);
    let walked = walk(dir, &rows, true, &hands);
    assert_eq!(walked.left, 0, "case 2: {walked:?}");
    assert!(!dir.join("derived").exists(), "case 2: the row stands");
    assert_eq!(
        hands.take(),
        [true],
        "case 2: an unlink before the owner ended"
    );
    let status = owner.lock().unwrap().try_wait().unwrap();
    assert_eq!(status.and_then(|status| status.code()), Some(1), "case 2");
}

#[test]
fn the_delete_walk_leaves_the_row_of_an_owner_that_it_may_not_open() {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, ERROR_SUCCESS, HANDLE};
    use windows::Win32::Security::Authorization::{SetSecurityInfo, SE_KERNEL_OBJECT};
    use windows::Win32::Security::{
        InitializeAcl, ACL, ACL_REVISION, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, CREATE_SUSPENDED, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const NO_PROCESS: u32 = 0xFFFF_FFFC;
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "a name of no test"])
        .creation_flags(CREATE_SUSPENDED.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the suspended child starts");
    let pid = child.id();
    let owner = Mutex::new(child);
    let _guard = EndsItsOwnChild(&owner);
    let started = start_time_of(pid).expect("the child's start time");
    let process = HANDLE(owner.lock().unwrap().as_raw_handle());
    let mut acl = ACL::default();
    let size = u32::try_from(std::mem::size_of::<ACL>()).unwrap();
    // SAFETY: `acl` is a live ACL of `size` bytes, which the call fills as a
    // list with no entry.
    unsafe { InitializeAcl(&mut acl, size, ACL_REVISION) }.unwrap();
    // SAFETY: the handle is the child's, open with every right; the call
    // reads the empty list and gives the child that list as its own, with no
    // entry inherited.
    let set = unsafe {
        SetSecurityInfo(
            process,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(std::ptr::from_ref(&acl)),
            None,
        )
    };
    assert_eq!(set, ERROR_SUCCESS, "the child's list was not set");
    // SAFETY: OpenProcess takes plain values; a handle that it gives is
    // closed below.
    let opened = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };
    let refused = opened.as_ref().err().map(windows::core::Error::code);
    if let Ok(handle) = opened {
        // SAFETY: the handle is open and owned here alone.
        let _ = unsafe { CloseHandle(handle) };
    }
    assert_eq!(
        refused,
        Some(ERROR_ACCESS_DENIED.to_hresult()),
        "this process opened its child through an empty list, so no refused open can be made here"
    );
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    let rows = rows();
    let hands = Watching {
        owner: &owner,
        unlinks: Mutex::new(Vec::new()),
    };

    a_derived_row(dir, pid, started);
    let walked = walk(dir, &rows, true, &hands);
    assert_eq!(walked.left, 1, "case 1: {walked:?}");
    assert!(
        dir.join("derived").join("kept").exists(),
        "case 1: the row's file went"
    );
    assert_eq!(hands.take(), Vec::<bool>::new(), "case 1: an unlink");
    assert!(
        owner.lock().unwrap().try_wait().unwrap().is_none(),
        "case 1: the child ended"
    );

    a_derived_row(dir, NO_PROCESS, started);
    let walked = walk(dir, &rows, true, &hands);
    assert_eq!(walked.left, 0, "case 2: {walked:?}");
    assert!(!dir.join("derived").exists(), "case 2: the row stands");
}
