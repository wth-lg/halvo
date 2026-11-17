use std::ffi::{CStr, CString, OsStr};

use super::*;
use crate::store::durable::{self, Dir, OsFs};

const COUNTER_C: &str = r#"
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <sys/param.h>
#include <unistd.h>

#define INTERPOSE(replacement, original)                                      \
  __attribute__((used)) static struct {                                       \
    const void *r;                                                            \
    const void *o;                                                            \
  } interpose_##original __attribute__((section("__DATA,__interpose"))) = {   \
      (const void *)(unsigned long)&replacement,                              \
      (const void *)(unsigned long)&original}

#define LOGGED 4096
static long full_flushes = 0;
static long plain_flushes = 0;
static char paths[LOGGED][MAXPATHLEN];
static char failing[MAXPATHLEN];
static long failing_left = 0;

static int counted(int fd, char *path) {
  const char *mark = getenv("R1_FLUSH_MARK");
  if (!mark || !*mark) return 0;
  if (fcntl(fd, F_GETPATH, path) == -1) return 0;
  return strstr(path, mark) != NULL;
}

static int ends_with(const char *text, const char *end) {
  size_t a = strlen(text), b = strlen(end);
  return b > 0 && b <= a && strcmp(text + a - b, end) == 0;
}

int r1_fcntl(int fd, int cmd, ...) {
  va_list ap;
  va_start(ap, cmd);
  long arg = va_arg(ap, long);
  va_end(ap);
  if (cmd == F_FULLFSYNC) {
    char path[MAXPATHLEN];
    if (counted(fd, path)) {
      if (full_flushes < LOGGED) strlcpy(paths[full_flushes], path, MAXPATHLEN);
      full_flushes++;
      if (failing_left > 0 && ends_with(path, failing)) {
        failing_left--;
        errno = EIO;
        return -1;
      }
    }
  }
  return fcntl(fd, cmd, arg);
}

int r1_fsync(int fd) {
  char path[MAXPATHLEN];
  if (counted(fd, path)) plain_flushes++;
  return fsync(fd);
}

long r1_full_flushes(void) { return full_flushes; }
long r1_plain_flushes(void) { return plain_flushes; }
const char *r1_full_flush_path(long at) {
  return at >= 0 && at < full_flushes && at < LOGGED ? paths[at] : "";
}
void r1_fail_full_flushes(long count, const char *end) {
  strlcpy(failing, end, MAXPATHLEN);
  failing_left = count;
}

INTERPOSE(r1_fcntl, fcntl);
INTERPOSE(r1_fsync, fsync);
"#;

const DIR_KEY: &str = "R1_FLUSH_DIR";
const MARK_KEY: &str = "R1_FLUSH_MARK";
const GROUP_KEY: &str = "R1_FLUSH_GROUP";

type Count = unsafe extern "C" fn() -> libc::c_long;
type PathAt = unsafe extern "C" fn(libc::c_long) -> *const libc::c_char;
type FailNext = unsafe extern "C" fn(libc::c_long, *const libc::c_char);

/// The counter of the system's full-flush calls that the child's library
/// puts in front of `fcntl`: every `fcntl(fd, F_FULLFSYNC)` on a file whose
/// path holds the mark is counted with that path, and a plain `fsync` on such
/// a file is counted apart.
struct Flushes {
    total: Count,
    plain: Count,
    path_at: PathAt,
    fail_next: FailNext,
    mark: String,
}

fn symbol(name: &CStr) -> Option<*mut libc::c_void> {
    let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    (!found.is_null()).then_some(found)
}

impl Flushes {
    fn loaded(mark: String) -> Option<Self> {
        let total = symbol(c"r1_full_flushes")?;
        let plain = symbol(c"r1_plain_flushes")?;
        let path_at = symbol(c"r1_full_flush_path")?;
        let fail_next = symbol(c"r1_fail_full_flushes")?;
        unsafe {
            Some(Self {
                total: std::mem::transmute::<*mut libc::c_void, Count>(total),
                plain: std::mem::transmute::<*mut libc::c_void, Count>(plain),
                path_at: std::mem::transmute::<*mut libc::c_void, PathAt>(path_at),
                fail_next: std::mem::transmute::<*mut libc::c_void, FailNext>(fail_next),
                mark,
            })
        }
    }

    fn full(&self) -> usize {
        unsafe { (self.total)() as usize }
    }

    fn plain(&self) -> usize {
        unsafe { (self.plain)() as usize }
    }

    /// The paths of the full flushes from the `from`-th on, each from the
    /// part after the mark's directory: `.` for that directory itself.
    fn since(&self, from: usize) -> Vec<String> {
        (from..self.full())
            .map(|at| {
                let path = unsafe { CStr::from_ptr((self.path_at)(at as libc::c_long)) }
                    .to_string_lossy()
                    .into_owned();
                match path.split_once(&format!("{}/", self.mark)) {
                    Some((_, below)) => below.to_string(),
                    None if path.ends_with(&self.mark) => ".".to_string(),
                    None => path,
                }
            })
            .collect()
    }

    /// The next `count` full flushes of a file whose path ends in `end`
    /// answer EIO.
    fn fail_next(&self, count: usize, end: &str) {
        let end = CString::new(end).unwrap();
        unsafe { (self.fail_next)(count as libc::c_long, end.as_ptr()) }
    }
}

/// One step of the child: the full flushes that `act` made, by path, and
/// whether it made a plain fsync of a counted file, printed in one line.
fn step<T>(flushes: &Flushes, kind: &str, act: impl FnOnce() -> T) -> (T, Vec<String>, usize) {
    let (from, plain) = (flushes.full(), flushes.plain());
    let answer = act();
    let on = flushes.since(from);
    let plain = flushes.plain() - plain;
    println!(
        "FULLFLUSH kind={kind} calls={} plain_fsync={plain} on={}",
        on.len(),
        on.join(",")
    );
    (answer, on, plain)
}

fn count_of(on: &[String], name: &str) -> usize {
    on.iter().filter(|path| path.as_str() == name).count()
}

fn a_store(top: &Path, name: &str) -> (PathBuf, PathBuf, RootId) {
    let (store_dir, roots_dir) = (top.join(name).join("store"), top.join(name).join("roots"));
    std::fs::create_dir(top.join(name)).unwrap();
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    drop(EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap());
    (store_dir, roots_dir, root_id)
}

fn commit_group(top: &Path, flushes: &Flushes, problems: &mut Vec<String>) {
    let ((store_dir, roots_dir, root_id), _, _) =
        step(flushes, "create", || a_store(top, "commit"));
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    for round in 1..=2u8 {
        let (written, on, plain) = step(flushes, &format!("write_batch {round}"), || {
            store.write_segment(&mut Cursor::new(vec![round; 4096]))
        });
        let log = count_of(&on, "commit/store/store.db-wal");
        if written.is_err() || log == 0 || plain != 0 {
            problems.push(format!(
                "write {round} answered {:?} with {log} full flushes of the log and {plain} plain fsync: its commit does not reach the drive",
                written.map(drop)
            ));
        }
    }
}

fn helper_write(dir: &Dir, temp: &str, target: &str) -> io::Result<()> {
    durable::write_durably(
        &OsFs,
        dir,
        OsStr::new(temp),
        OsStr::new(target),
        |e| e,
        |_| Ok(()),
        |dir, partial| match dir.remove_file(partial) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
        |out| out.write_all(b"{\"version\":1}"),
    )
}

fn helper_group(top: &Path, flushes: &Flushes, problems: &mut Vec<String>) {
    let held = top.join("helper");
    std::fs::create_dir(&held).unwrap();
    let dir = Dir::open(&held).unwrap();
    for kind in ["delete-all.intent", "daemon-state.json"] {
        let temp = format!("{kind}.tmp");
        let (written, on, plain) = step(flushes, kind, || helper_write(&dir, &temp, kind));
        let want = vec![format!("helper/{temp}"), "helper".to_string()];
        if written.is_err() || on != want || plain != 0 {
            problems.push(format!(
                "the helper's write of {kind} answered {written:?} with full flushes on {on:?} and {plain} plain fsync, not on {want:?}"
            ));
        }
    }
    let (removed, on, _) = step(flushes, "removal of delete-all.intent", || {
        durable::remove_durably(&OsFs, &dir, OsStr::new("delete-all.intent"))
    });
    if removed.is_err() || on != ["helper"] {
        problems.push(format!(
            "the helper's removal answered {removed:?} with full flushes on {on:?}, not on the directory alone"
        ));
    }
    flushes.fail_next(1, "/daemon-state.json.tmp");
    std::fs::remove_file(held.join("daemon-state.json")).unwrap();
    let (refused, on, plain) = step(
        flushes,
        "daemon-state.json whose file's full flush fails",
        || helper_write(&dir, "daemon-state.json.tmp", "daemon-state.json"),
    );
    let published = held.join("daemon-state.json").exists();
    let temp_left = held.join("daemon-state.json.tmp").exists();
    println!(
        "FULLFLUSH refusal answer={refused:?} published={published} temp_left={temp_left} plain_fsync={plain}"
    );
    if refused.is_ok()
        || published
        || temp_left
        || plain != 0
        || on != ["helper/daemon-state.json.tmp"]
    {
        problems.push(format!(
            "a write whose full flush failed answered {refused:?}, published {published}, left its temporary name {temp_left}, made {plain} plain fsync and full flushes on {on:?}"
        ));
    }
    flushes.fail_next(1, "/helper");
    let (refused, on, _) = step(
        flushes,
        "delete-all.intent whose directory's full flush fails",
        || helper_write(&dir, "delete-all.intent.tmp", "delete-all.intent"),
    );
    if refused.is_ok() || on != ["helper/delete-all.intent.tmp", "helper"] {
        problems.push(format!(
            "a write whose directory's full flush failed answered {refused:?} with full flushes on {on:?}"
        ));
    }
}

/// Run by the parent tests of this module in a child process with the
/// counting library in front of `fcntl`: the group of steps that the parent
/// names, each step printed with its full flushes by path.
#[test]
fn full_flush_child_for_parent() {
    let (Ok(top), Ok(mark), Ok(group)) = (
        std::env::var(DIR_KEY),
        std::env::var(MARK_KEY),
        std::env::var(GROUP_KEY),
    ) else {
        println!("full_flush_child_for_parent: no parent asked for a child");
        return;
    };
    let flushes = Flushes::loaded(mark).expect("the counting library is not loaded");
    let top = PathBuf::from(top);
    let mut problems = Vec::new();
    match group.as_str() {
        "helper" => helper_group(&top, &flushes, &mut problems),
        "commit" => commit_group(&top, &flushes, &mut problems),
        other => panic!("no group {other}"),
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Builds the counting library and runs the child for `group`; panics with
/// the child's lines when the child fails.
fn run_child(group: &str) {
    let work = tempfile::tempdir().unwrap();
    let source = work.path().join("counter.c");
    let library = work.path().join("counter.dylib");
    std::fs::write(&source, COUNTER_C).unwrap();
    let built = Command::new("cc")
        .args(["-dynamiclib", "-o"])
        .arg(&library)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "cc did not build the counting library: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let top = work.path().join("flushes");
    std::fs::create_dir(&top).unwrap();
    let top = top.canonicalize().unwrap();
    let mark = format!(
        "{}/flushes",
        work.path().file_name().unwrap().to_string_lossy()
    );
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::hardening_tests::full_flush::full_flush_child_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(DIR_KEY, &top)
        .env(MARK_KEY, &mark)
        .env(GROUP_KEY, group)
        .env("DYLD_INSERT_LIBRARIES", &library)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout
        .lines()
        .filter_map(|line| line.find("FULLFLUSH").map(|at| &line[at..]))
    {
        println!("{line}");
    }
    let ran = stdout.contains("1 passed") || stdout.contains("1 failed");
    assert!(ran, "the child ran no test: {stdout}");
    assert!(
        out.status.success(),
        "the {group} child failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Ruling PR-30 PR9 (1): each write of the durable-write helper makes the
/// full-flush call itself, once for the file and once for its directory, and
/// a removal once for the directory; a write whose full flush fails answers
/// an error, publishes nothing and makes no plain fsync in its place.
#[test]
fn each_write_of_the_durable_helper_makes_the_full_flush_call_and_refuses_when_it_fails() {
    run_child("helper");
}

/// Ruling PR-30 PR9 (4): the commit of each write of the store reaches the
/// drive: its log is flushed with the full-flush call and with no plain fsync.
#[test]
fn each_commit_of_the_store_flushes_its_log_with_the_full_flush_call() {
    run_child("commit");
}
