use std::ffi::{CStr, CString, OsStr};

use super::*;
use crate::store::durable::{self, Dir, OsFs};
use crate::store::RootReading;

const COUNTER_C: &str = r#"
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <sys/param.h>
#include <sys/stat.h>
#include <unistd.h>

#define INTERPOSE(replacement, original)                                      \
  __attribute__((used)) static struct {                                       \
    const void *r;                                                            \
    const void *o;                                                            \
  } interpose_##original __attribute__((section("__DATA,__interpose"))) = {   \
      (const void *)(unsigned long)&replacement,                              \
      (const void *)(unsigned long)&original}

#define LOGGED 4096
#define PLAIN_LOGGED 64
static long full_flushes = 0;
static long plain_flushes = 0;
static struct {
  long dev;
  long ino;
  int dir;
} plain_on[PLAIN_LOGGED];
static char paths[LOGGED][MAXPATHLEN];
static char failing[MAXPATHLEN];
static long failing_left = 0;
static int failing_errno = EIO;

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
        errno = failing_errno;
        return -1;
      }
    }
  }
  return fcntl(fd, cmd, arg);
}

int r1_fsync(int fd) {
  char path[MAXPATHLEN];
  if (counted(fd, path)) {
    if (plain_flushes < PLAIN_LOGGED) {
      struct stat st;
      if (fstat(fd, &st) == 0) {
        plain_on[plain_flushes].dev = (long)st.st_dev;
        plain_on[plain_flushes].ino = (long)st.st_ino;
        plain_on[plain_flushes].dir = S_ISDIR(st.st_mode) ? 1 : 0;
      } else {
        plain_on[plain_flushes].dir = -1;
      }
    }
    plain_flushes++;
  }
  return fsync(fd);
}

int r1_plain_flush_on(long at, long *dev, long *ino) {
  if (at < 0 || at >= plain_flushes) return -3;
  if (at >= PLAIN_LOGGED) return -2;
  *dev = plain_on[at].dev;
  *ino = plain_on[at].ino;
  return plain_on[at].dir;
}

long r1_full_flushes(void) { return full_flushes; }
long r1_plain_flushes(void) { return plain_flushes; }
const char *r1_full_flush_path(long at) {
  return at >= 0 && at < full_flushes && at < LOGGED ? paths[at] : "";
}
void r1_fail_full_flushes(long count, const char *end, int err) {
  strlcpy(failing, end, MAXPATHLEN);
  failing_left = count;
  failing_errno = err;
}

INTERPOSE(r1_fcntl, fcntl);
INTERPOSE(r1_fsync, fsync);
"#;

const DIR_KEY: &str = "R1_FLUSH_DIR";
const MARK_KEY: &str = "R1_FLUSH_MARK";
const GROUP_KEY: &str = "R1_FLUSH_GROUP";

type Count = unsafe extern "C" fn() -> libc::c_long;
type PathAt = unsafe extern "C" fn(libc::c_long) -> *const libc::c_char;
type FailNext = unsafe extern "C" fn(libc::c_long, *const libc::c_char, libc::c_int);
type PlainOn =
    unsafe extern "C" fn(libc::c_long, *mut libc::c_long, *mut libc::c_long) -> libc::c_int;

const PLAIN_ON_A_DIRECTORY: i32 = 1;
const PLAIN_NOT_LOGGED: i32 = -2;

/// The counter of the system's full-flush calls that the child's library
/// puts in front of `fcntl`: every `fcntl(fd, F_FULLFSYNC)` on a file whose
/// path holds the mark is counted with that path, and a plain `fsync` on such
/// a file is counted apart.
struct Flushes {
    total: Count,
    plain: Count,
    path_at: PathAt,
    fail_next: FailNext,
    plain_on: PlainOn,
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
        let plain_on = symbol(c"r1_plain_flush_on")?;
        unsafe {
            Some(Self {
                total: std::mem::transmute::<*mut libc::c_void, Count>(total),
                plain: std::mem::transmute::<*mut libc::c_void, Count>(plain),
                path_at: std::mem::transmute::<*mut libc::c_void, PathAt>(path_at),
                fail_next: std::mem::transmute::<*mut libc::c_void, FailNext>(fail_next),
                plain_on: std::mem::transmute::<*mut libc::c_void, PlainOn>(plain_on),
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

    fn plain_since(&self, from: usize) -> Vec<(i32, i64, i64)> {
        (from..self.plain())
            .map(|at| {
                let (mut dev, mut ino): (i64, i64) = (0, 0);
                let kind = unsafe { (self.plain_on)(at as libc::c_long, &mut dev, &mut ino) };
                (kind, dev, ino)
            })
            .collect()
    }

    /// The next `count` full flushes of a file whose path ends in `end`
    /// answer EIO.
    fn fail_next(&self, count: usize, end: &str) {
        self.fail_next_with(count, end, libc::EIO);
    }

    /// The next `count` full flushes of a file whose path ends in `end`
    /// answer the error `errno`.
    fn fail_next_with(&self, count: usize, end: &str, errno: libc::c_int) {
        let end = CString::new(end).unwrap();
        unsafe { (self.fail_next)(count as libc::c_long, end.as_ptr(), errno) }
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
    let directory = {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(&store_dir).unwrap();
        (
            PLAIN_ON_A_DIRECTORY,
            metadata.dev() as i64,
            metadata.ino() as i64,
        )
    };
    for round in 1..=2u8 {
        let before = flushes.plain();
        let (written, on, plain) = step(flushes, &format!("write_batch {round}"), || {
            store.write_segment(&mut Cursor::new(vec![round; 4096]))
        });
        let log = count_of(&on, "commit/store/store.db-wal");
        let plain_on = flushes.plain_since(before);
        println!("FULLFLUSH kind=write_batch {round} plain_on={plain_on:?} store_directory={directory:?}");
        let the_new_logs_directory_sync = round == 1 && plain_on == [directory];
        if written.is_err() || log == 0 || (plain != 0 && !the_new_logs_directory_sync) {
            problems.push(format!(
                "write {round} answered {:?} with {log} full flushes of the log and {plain} plain fsync on {plain_on:?}, the store directory being {directory:?}: its commit does not reach the drive",
                written.map(drop)
            ));
        }
        let not_logged = plain_on
            .iter()
            .filter(|on| on.0 == PLAIN_NOT_LOGGED)
            .count();
        if not_logged > 0 {
            problems.push(format!(
                "write {round}: the log of plain fsyncs overflowed, {not_logged} of {plain} not logged"
            ));
        }
    }
}

fn checkpoint_group(top: &Path, flushes: &Flushes, problems: &mut Vec<String>) {
    let (store_dir, roots_dir, root_id) = a_store(top, "expiry");
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    store
        .write_segment_with_expiry(&mut Cursor::new(vec![7u8; 4096]), 0, 1)
        .unwrap();
    store
        .write_segment(&mut Cursor::new(vec![8u8; 4096]))
        .unwrap();
    let (shredded, on, _) = step(flushes, "expiry", || store.crypto_shred_expired());
    let database = on.iter().position(|path| path == "expiry/store/store.db");
    let removal = on
        .iter()
        .position(|path| path.starts_with("expiry/store/segments/") && !path.contains('.'));
    let log_left = wal_len(&store_dir);
    println!(
        "FULLFLUSH expiry answer={:?} database_full_flushes={} first_database_flush_at={database:?} first_removal_flush_at={removal:?} log_bytes_after={log_left}",
        shredded.as_ref().map(|report| report.segments_shredded),
        count_of(&on, "expiry/store/store.db")
    );
    let in_order =
        matches!((database, removal), (Some(checkpoint), Some(removal)) if checkpoint < removal);
    if !matches!(&shredded, Ok(report) if report.segments_shredded == 1)
        || !in_order
        || log_left != 0
    {
        problems.push(format!(
            "the expiry answered {:?}, flushed store.db at {database:?} and the segment's folder at {removal:?}, and left {log_left} bytes in the log: its truncating checkpoint does not reach the drive before the file's removal",
            shredded.map(|report| report.segments_shredded)
        ));
    }
}

fn reading_of(answer: &StoreResult<crate::store::DeleteReport>) -> Option<RootReading> {
    match answer {
        Ok(report) => Some(report.root),
        Err(StoreError::DeleteIncomplete(report)) => Some(report.root),
        Err(_) => None,
    }
}

fn root_group(top: &Path, flushes: &Flushes, problems: &mut Vec<String>) {
    for (at, case) in ["delete_all", "tail"].into_iter().enumerate() {
        let name = format!("root-{at}");
        let (store_dir, roots_dir, root_id) = a_store(top, &name);
        let root_file = format!("{name}/roots/{root_id}.root");
        let folder = format!("{name}/roots");
        let (answer, on, _) = step(flushes, case, || {
            if case == "tail" {
                tail(&store_dir, &roots_dir)
            } else {
                let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
                store.delete_all()
            }
        });
        let reading = reading_of(&answer);
        let zero_fill = count_of(&on, &root_file);
        let after = on
            .iter()
            .position(|path| *path == root_file)
            .map_or(0, |first| {
                on[first..].iter().filter(|path| **path == folder).count()
            });
        println!(
            "FULLFLUSH root step={case} answer_ok={} root={reading:?} zero_fill_full_flushes={zero_fill} roots_folder_full_flushes_after_it={after}",
            answer.is_ok()
        );
        if answer.is_err()
            || reading != Some(RootReading::Destroyed)
            || zero_fill != 1
            || after == 0
        {
            problems.push(format!(
                "{case}: answer ok {}, root {reading:?}, {zero_fill} full flushes of the zero-fill, {after} of the roots folder after it",
                answer.is_ok()
            ));
        }
    }
    for (at, case) in ["delete_all", "tail"].into_iter().enumerate() {
        let name = format!("root-failing-{at}");
        let (store_dir, roots_dir, root_id) = a_store(top, &name);
        let mut store = (case == "delete_all")
            .then(|| open_over_file_root(&store_dir, &roots_dir, root_id).unwrap());
        flushes.fail_next(1, &format!("{name}/roots"));
        let (first, _, _) = step(
            flushes,
            &format!("{case} whose roots folder's full flush fails"),
            || match store.as_mut() {
                Some(store) => store.delete_all(),
                None => tail(&store_dir, &roots_dir),
            },
        );
        let first_root = reading_of(&first);
        let first_listed = match &first {
            Ok(report) => Some(report.roots.listed),
            Err(StoreError::DeleteIncomplete(report)) => Some(report.roots.listed),
            Err(_) => None,
        };
        let again = match store.as_mut() {
            Some(store) => store.delete_all(),
            None => tail(&store_dir, &roots_dir),
        };
        drop(store);
        let after = tail(&store_dir, &roots_dir);
        println!(
            "FULLFLUSH root step={case} with a failed folder flush: first_ok={} first_root={first_root:?} first_listed={first_listed:?} again_ok={} again_root={:?} next_tail_ok={} next_tail_root={:?}",
            first.is_ok(),
            again.is_ok(),
            reading_of(&again),
            after.is_ok(),
            reading_of(&after)
        );
        if first.is_ok() || first_root != Some(RootReading::Left(Some(root_id))) {
            problems.push(format!(
                "{case}: a failed flush of the roots folder answered ok {} with the root {first_root:?}",
                first.is_ok()
            ));
        }
        let again_as_stated = match case {
            "delete_all" => {
                again.is_err() && reading_of(&again) == Some(RootReading::Left(Some(root_id)))
            }
            _ => again.is_ok() && reading_of(&again) == Some(RootReading::Unnamed),
        };
        if !again_as_stated {
            problems.push(format!(
                "{case}: the call after a failed flush of the roots folder answered ok {} with the root {:?}",
                again.is_ok(),
                reading_of(&again)
            ));
        }
        if after.is_err() || reading_of(&after) != Some(RootReading::Unnamed) {
            problems.push(format!(
                "{case}: the tail after a failed flush of the roots folder answered {} with the root {:?}",
                match &after {
                    Err(e) => e.to_string(),
                    Ok(_) => "Ok".to_string(),
                },
                reading_of(&after)
            ));
        }
    }
    let (store_dir, roots_dir, _) = a_store(top, "root-unnamed");
    std::fs::remove_file(KeyFile::path(&store_dir)).unwrap();
    flushes.fail_next(1, "root-unnamed/roots");
    let (first, _, _) = step(
        flushes,
        "a tail that names no root, whose roots folder's full flush fails",
        || tail(&store_dir, &roots_dir),
    );
    let next = tail(&store_dir, &roots_dir);
    println!(
        "FULLFLUSH unnamed sweep with a failed folder flush: first_ok={} first_root={:?} next_ok={} next_root={:?}",
        first.is_ok(),
        reading_of(&first),
        next.is_ok(),
        reading_of(&next)
    );
    if first.is_ok() || next.is_err() {
        problems.push(format!(
            "a tail that names no root answered ok {} at a failed flush of its roots folder and ok {} after it",
            first.is_ok(),
            next.is_ok()
        ));
    }
    let roots_dir = top.join("scrub-roots");
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let (made, on, _) = step(
        flushes,
        "a create_root whose folder flush after the link fails, scrubbed",
        || {
            crate::custody::file_root::make_root_seams::fail(Some(
                crate::custody::file_root::make_root_seams::SYNC_THE_DIRECTORY,
            ));
            custody.create_root()
        },
    );
    let scrubbed = count_of(&on, &format!("scrub-roots/{root_id}.root"));
    println!(
        "FULLFLUSH scrub of a root after a failed folder flush: answer={made:?} scrub_full_flushes={scrubbed} root_file_stands={}",
        roots_dir.join(format!("{root_id}.root")).exists()
    );
    if made.is_ok() || scrubbed != 1 || roots_dir.join(format!("{root_id}.root")).exists() {
        problems.push(format!(
            "a create_root whose folder flush failed answered {made:?}, flushed its scrubbed root file {scrubbed} times and left it standing {}",
            roots_dir.join(format!("{root_id}.root")).exists()
        ));
    }
    let roots_dir = top.join("custody-roots");
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    custody.create_root().unwrap();
    let (destroyed, on, _) = step(flushes, "the custody's destroy", || custody.destroy_root());
    let folder = count_of(&on, "custody-roots");
    if destroyed.is_err() || folder != 1 {
        problems.push(format!(
            "the custody's destroy answered {destroyed:?} with {folder} full flushes of its folder"
        ));
    }
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    custody.create_root().unwrap();
    flushes.fail_next(1, "custody-roots");
    let (first, on, _) = step(
        flushes,
        "the custody's destroy whose folder flush fails",
        || custody.destroy_root(),
    );
    let again = custody.destroy_root();
    println!(
        "FULLFLUSH custody destroy with a failed folder flush: first={first:?} again={again:?} root_file_full_flushes={}",
        count_of(&on, &format!("custody-roots/{root_id}.root"))
    );
    if first.is_ok() || again.is_err() {
        problems.push(format!(
            "the custody's destroy answered {first:?} at a failed flush of its folder and {again:?} after it"
        ));
    }
}

fn zero_fill_group(top: &Path, flushes: &Flushes, problems: &mut Vec<String>) {
    for (at, case) in ["delete_all", "tail", "the custody's destroy"]
        .into_iter()
        .enumerate()
    {
        let name = format!("zero-fill-{at}");
        let (store_dir, roots_dir, root_id) = a_store(top, &name);
        let root_file = roots_dir.join(format!("{root_id}.root"));
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        let mut store = (case == "delete_all")
            .then(|| open_over_file_root(&store_dir, &roots_dir, root_id).unwrap());
        let mut call = || -> Result<Option<RootReading>, String> {
            match case {
                "delete_all" => {
                    let answer = store.as_mut().unwrap().delete_all();
                    match answer {
                        Ok(report) => Ok(Some(report.root)),
                        Err(e) => Err(format!("{:?}", reading_of(&Err(e)))),
                    }
                }
                "tail" => {
                    let answer = tail(&store_dir, &roots_dir);
                    match answer {
                        Ok(report) => Ok(Some(report.root)),
                        Err(e) => Err(format!("{:?}", reading_of(&Err(e)))),
                    }
                }
                _ => custody
                    .destroy_root()
                    .map(|()| None)
                    .map_err(|e| format!("{e:?}")),
            }
        };
        flushes.fail_next(8, &format!("{name}/roots/{root_id}.root"));
        let (first, on, _) = step(
            flushes,
            &format!("{case} whose every full flush of the root file fails"),
            &mut call,
        );
        flushes.fail_next(0, "-");
        let stands_after_the_first = root_file.exists();
        let (again, _, _) = step(flushes, &format!("{case} again"), &mut call);
        let stands_after_again = root_file.exists();
        println!(
            "FULLFLUSH zero-fill never flushed: case={case} first={first:?} root_file_flushes={} root_file_stands={stands_after_the_first} again={again:?} root_file_stands_after_again={stands_after_again}",
            count_of(&on, &format!("{name}/roots/{root_id}.root"))
        );
        let first_left = match case {
            "the custody's destroy" => first.is_err(),
            _ => first == Err(format!("{:?}", Some(RootReading::Left(Some(root_id))))),
        };
        let again_done = match case {
            "delete_all" => again == Ok(Some(RootReading::Destroyed)),
            "tail" => again.is_ok(),
            _ => again == Ok(None),
        };
        if !first_left || !stands_after_the_first || !again_done || stands_after_again {
            problems.push(format!(
                "{case}: with every full flush of the root file failing it answered {first:?} and left the root file standing {stands_after_the_first}; with the flushes answered it answered {again:?} and left it standing {stands_after_again}"
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
    let at_its_name = std::fs::read(held.join("delete-all.intent")).ok();
    println!(
        "FULLFLUSH directory refusal answer={refused:?} the_new_content_at_its_name={}",
        at_its_name.as_deref() == Some(b"{\"version\":1}".as_slice())
    );
    if refused.is_ok()
        || on != ["helper/delete-all.intent.tmp", "helper"]
        || at_its_name.as_deref() != Some(b"{\"version\":1}".as_slice())
    {
        problems.push(format!(
            "a write whose directory's full flush failed answered {refused:?} with full flushes on {on:?}, and its name holds {at_its_name:?}"
        ));
    }
    flushes.fail_next_with(1, "/daemon-state.json.tmp", libc::EINTR);
    let (written, on, plain) = step(
        flushes,
        "daemon-state.json whose file's full flush a signal interrupts once",
        || helper_write(&dir, "daemon-state.json.tmp", "daemon-state.json"),
    );
    let want = [
        "helper/daemon-state.json.tmp",
        "helper/daemon-state.json.tmp",
        "helper",
    ];
    if written.is_err() || on != want || plain != 0 {
        problems.push(format!(
            "a write whose full flush a signal interrupted once answered {written:?} with full flushes on {on:?} and {plain} plain fsync, not on {want:?}"
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
        "checkpoint" => checkpoint_group(&top, &flushes, &mut problems),
        "root" => root_group(&top, &flushes, &mut problems),
        "zero-fill" => zero_fill_group(&top, &flushes, &mut problems),
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
    let stderr = String::from_utf8_lossy(&out.stderr);
    let told: Vec<&str> = stderr
        .lines()
        .chain(stdout.lines())
        .filter(|line| {
            !(line.starts_with("test ") || line.starts_with("running ") || line.is_empty())
        })
        .collect();
    let ran = stdout.contains("test result: ok. 1 passed")
        || stdout.contains("test result: FAILED. 0 passed; 1 failed");
    assert!(ran, "the child ran no test:\n{}", told.join("\n"));
    assert!(
        out.status.success(),
        "the {group} child failed:\n{}",
        told.join("\n")
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

/// Ruling PR-30 PR9 (4): the commit of each write of the store asks for the
/// full flush, which the open reads back as set: with every call answered,
/// its log is flushed with the full-flush call and with no plain fsync. Where
/// such a call fails the database library makes a plain fsync in its place
/// and answers Ok, so the commit is not known to be on the drive: a named
/// limit (ruling ST-50).
#[test]
fn each_commit_of_the_store_flushes_its_log_with_the_full_flush_call() {
    run_child("commit");
}

/// Ruling PR-30 PR9 (2): the truncating checkpoint after the key clear of an
/// expiry asks for the full flush of store.db, which the open reads back as
/// set, before the segment's file is removed, and leaves the log empty. With
/// that full flush answered, the checkpoint is the step that makes the expiry
/// last through a loss of power. Where such a call fails the database library
/// makes a plain fsync in its place and answers Ok, and the expiry still
/// removes the segment's file: a named limit (ruling ST-50).
#[test]
fn the_truncating_checkpoint_of_an_expiry_flushes_the_database_before_the_files_removal() {
    run_child("checkpoint");
}

/// Ruling PR-30 PR9 (3): the root step's zero-fill makes one full flush of
/// the root file, and the roots folder is flushed after it. When that flush
/// fails, the delete-all and the tail read the root left and answer that the
/// delete is not finished, the tail after it answers Ok, and the custody's
/// destroy answers the flush's error and then Ok.
#[test]
fn the_root_steps_zero_fill_and_its_roots_folder_are_flushed_and_a_failed_folder_flush_reads_left()
{
    run_child("root");
}

/// Ruling ST-50 Addition 1: the root step reads a root destroyed only when
/// its zero-fill's full flush was answered. With every full flush of the
/// root file failing, the handle's delete-all and the tail read the root left
/// and answer that the delete is not finished, the custody's destroy answers
/// an error, and the root file still stands at its name; with the flushes
/// answered the next call destroys it.
#[test]
fn a_root_whose_zero_fill_is_never_flushed_reads_left_and_stays_at_its_name() {
    run_child("zero-fill");
}
