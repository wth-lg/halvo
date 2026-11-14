//! The listing and the reading of the crash row of `web-rows` (the coordinator's rulings CP-165 (2)
//! and CP-167 E1 to E4): which files changed under the places that a crash of the agent's browser
//! may write, and what the row's four readings give together.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A place that the listing reads: its name, its root, whether it lies inside the run's profile or
/// the browser's own temporary directory (both removed by the stop), and whether it is a report
/// place of the system, listed with no promise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub name: &'static str,
    pub root: PathBuf,
    pub inside_profile: bool,
    pub system: bool,
}

/// One file of a listing: its place, its path under the place's root, its size and its time of
/// change in nanoseconds since the Unix epoch, at the grain that the system gives (CP-186 T4 (c)).
/// A file written again at its own size inside one tick of that grain is no change.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct File {
    pub place: &'static str,
    pub rel: String,
    pub size: u64,
    pub modified: u64,
    /// Whether the entry is a symbolic link, whose size is the length of its target.
    pub link: bool,
    /// Whether the entry stood in the earlier listing and is gone from the later one; its size
    /// and time are the earlier listing's (CP-186 T3).
    pub removed: bool,
}

/// One listing: each file by place and path, with its size, its time of change and whether it is
/// a link.
pub type Listing = BTreeMap<(&'static str, String), (u64, u64, bool)>;

/// The name of the place of the system's temporary directory.
pub const SYSTEM_TEMP: &str = "system_temp";

/// The deepest level under a place's root that the listing reads.
pub const DEPTH: usize = 12;

/// Every file under each place, by place and path; a place that does not exist gives none, and a
/// link is listed and not followed.
pub fn snapshot(places: &[Place]) -> BTreeMap<(&'static str, String), (u64, u64, bool)> {
    let mut out = BTreeMap::new();
    for p in places {
        walk(p.name, &p.root, &p.root, 0, &mut out);
    }
    out
}

fn walk(
    place: &'static str,
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut BTreeMap<(&'static str, String), (u64, u64, bool)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if depth < DEPTH {
                walk(place, root, &path, depth + 1, out);
            }
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as u64);
        out.insert(
            (place, rel),
            (meta.len(), modified, meta.file_type().is_symlink()),
        );
    }
}

/// The files of `after` that are new against `before` or changed in size or time, and every file
/// of `before` that `after` lacks, marked as removed (CP-186 T3).
pub fn changed(before: &Listing, after: &Listing) -> Vec<File> {
    let file = |(place, rel): &(&'static str, String),
                (size, modified, link): &(u64, u64, bool),
                removed| File {
        place,
        rel: rel.clone(),
        size: *size,
        modified: *modified,
        link: *link,
        removed,
    };
    let mut out: Vec<File> = after
        .iter()
        .filter(|(k, v)| before.get(*k) != Some(*v))
        .map(|(k, v)| file(k, v, false))
        .collect();
    out.extend(
        before
            .iter()
            .filter(|(k, _)| !after.contains_key(*k))
            .map(|(k, v)| file(k, v, true)),
    );
    out
}

/// The listed place, other than the run's own profile and temporary directory, under which the
/// row's scratch directory `scratch` lies, if any (CP-186 S6): the row's profiles would then be
/// listed twice and their files read as left outside. Both paths are read alike (`real_path`).
pub fn scratch_under_place(scratch: &Path, places: &[Place]) -> Option<&'static str> {
    let real = real_path;
    let scratch = real(scratch);
    places
        .iter()
        .filter(|p| !p.inside_profile)
        .find(|p| scratch.starts_with(real(&p.root)))
        .map(|p| p.name)
}

/// `p` read through its links: its deepest ancestor that exists, through `canonicalize`, with the
/// rest of `p` joined to it, so that a path that exists and one that does not yet are read in one
/// form (on Windows `canonicalize` gives the `\\?\` form; run 43503444269).
fn real_path(p: &Path) -> PathBuf {
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    let mut at = p;
    loop {
        if let Ok(found) = std::fs::canonicalize(at) {
            return rest.iter().rev().fold(found, |acc, part| acc.join(part));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                at = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// The removals of a reading (CP-186 T3), in three counts. `deciding`: a file that stood in
/// `origin`, the listing taken before the row's first reading, removed at a place that is neither
/// inside the profile, nor a report place of the system, nor the system's temporary directory.
/// `own`: such a removal of a file that `origin` lacks, which an earlier reading of the row left
/// and which decides nothing. `temp`: a removal in the system's temporary directory, which decides
/// nothing, because other programs of the machine remove their own files there during a reading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Removals {
    pub deciding: usize,
    pub own: usize,
    pub temp: usize,
}

pub fn removals(changed: &[File], places: &[Place], origin: &Listing) -> Removals {
    let mut r = Removals::default();
    for f in changed.iter().filter(|f| f.removed) {
        let Some(p) = places.iter().find(|p| p.name == f.place) else {
            continue;
        };
        if p.inside_profile || p.system {
            continue;
        }
        if p.name == SYSTEM_TEMP {
            r.temp += 1;
        } else if origin.contains_key(&(f.place, f.rel.clone())) {
            r.deciding += 1;
        } else {
            r.own += 1;
        }
    }
    r
}

/// A file's name for the comparison of two readings: its path, with the random part of the
/// browser's singleton directory in the system's temporary directory taken out
/// (`.com.google.Chrome.XXXXXX`, `.com.microsoft.edgemac.XXXXXX`), which each start of the browser
/// names anew.
pub fn name_key(rel: &str) -> String {
    rel.split('/')
        .map(|part| {
            for prefix in [
                ".com.google.Chrome.",
                "com.google.Chrome.",
                ".com.microsoft.edgemac.",
                "com.microsoft.edgemac.",
            ] {
                if let Some(rest) = part.strip_prefix(prefix) {
                    if rest.len() == 6 && rest.bytes().all(|b| b.is_ascii_alphanumeric()) {
                        return format!("{prefix}*");
                    }
                }
            }
            part.to_string()
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether a changed file is a crash's dump or report: a minidump (`.dmp`), a report of the
/// system (`.ips`, `.crash`, `.wer`, `.hdmp`, `.mdmp`), or a file under a Crashpad database's
/// `pending`, `completed` or `new` directory.
pub fn is_dump(f: &File) -> bool {
    let rel = f.rel.to_ascii_lowercase();
    [".dmp", ".ips", ".crash", ".wer", ".hdmp", ".mdmp"]
        .iter()
        .any(|x| rel.ends_with(x))
        || ["pending/", "completed/", "new/"]
            .iter()
            .any(|d| rel.starts_with(d) || rel.contains(&format!("/{d}")))
}

/// What one reading left outside the profile and the temporary directory, which the stop removes;
/// a removal is no file left.
pub fn left_outside(changed: &[File], places: &[Place]) -> Vec<File> {
    changed
        .iter()
        .filter(|f| !f.removed)
        .filter(|f| {
            places
                .iter()
                .find(|p| p.name == f.place)
                .is_some_and(|p| !p.inside_profile)
        })
        .cloned()
        .collect()
}

/// The files of `with` that `without` does not hold by place, name (`name_key`) and size: what a
/// crash left beyond a start with no crash (CP-165 (2) (b)). A symbolic link is matched by its
/// name alone, because its size is the length of its target, which a start draws anew (the
/// singleton's cookie).
pub fn beyond(with: &[File], without: &[File]) -> Vec<File> {
    with.iter()
        .filter(|f| {
            !without.iter().any(|b| {
                b.place == f.place
                    && name_key(&b.rel) == name_key(&f.rel)
                    && (b.size == f.size || (b.link && f.link))
            })
        })
        .cloned()
        .collect()
}

/// The facts of one reading that decide beside its files (CP-186 T2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// Everything that the reading changed under the places of its listing.
    pub changed: Vec<File>,
    /// The dumps that the reading left inside its profile: the sign that its crash was taken.
    pub dumps_inside_profile: usize,
    /// The crash handler's word when the listing was taken: `gone` once no handler of the reading
    /// ran, `stop_wait` when one still ran at the wait's end.
    pub handler: String,
    /// Whether the stop removed the profile and the temporary directory.
    pub removed: bool,
}

/// The row's result of one system and browser, and its cause when it is not `ok`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub result: &'static str,
    pub cause: &'static str,
}

impl Verdict {
    fn of(result: &'static str, cause: &'static str) -> Verdict {
        Verdict { result, cause }
    }
}

/// Whether `f` lies at a place that is no report place of the system: a file at a report place of
/// the system (`Place::system`) is recorded with no promise and decides nothing (CP-165 (2) (d)
/// and (g)).
fn deciding(places: &[Place], f: &File) -> bool {
    !places.iter().any(|p| p.name == f.place && p.system)
}

/// The row's result of one system and browser (CP-165 (2) (b), CP-186 T2), read in this order:
/// (a) `notrun` (`control_no_dump`) when the control's changed files hold no dump at a place that
/// is neither a report place of the system nor the system's temporary directory, where another
/// program may leave a dump during the control (CP-189 G8), for then nothing showed that the
/// browser's crash was taken;
/// (b) `miss` when the crash with the means left a file outside the profile, at a deciding place,
/// that the start with no crash did not leave (`beyond`), when anything was sent after the crash
/// (`sent`), or when the stop did not remove the profile and the temporary directory in the
/// reading with the means or in the start with no crash (`not_removed`); (c) `notrun`
/// (`no_dump_inside`) when the reading with the means shows no dump inside its profile, the sign
/// that its crash was taken (`Executor::crash` drops the command's answer), and also `miss`
/// (`removed_outside`) when the reading with the means removed a file that stood before the
/// row's first reading (`removals`, `deciding`); (d) `notrun`
/// (`handler`) when its crash handler's word is not `gone`: it still ran when the listing was
/// taken, or the table or the arguments of the browser's process were unread (`handler_state`);
/// (e) `ok`.
pub fn result(
    places: &[Place],
    origin: &Listing,
    baseline: &Facts,
    means: &Facts,
    control: &[File],
    sent_after_crash: usize,
) -> Verdict {
    if !control
        .iter()
        .any(|f| !f.removed && is_dump(f) && deciding(places, f) && f.place != SYSTEM_TEMP)
    {
        return Verdict::of("notrun", "control_no_dump");
    }
    let left_means = left_outside(&means.changed, places);
    let left_baseline = left_outside(&baseline.changed, places);
    if beyond(&left_means, &left_baseline)
        .iter()
        .any(|f| deciding(places, f))
    {
        return Verdict::of("miss", "beyond");
    }
    if sent_after_crash > 0 {
        return Verdict::of("miss", "sent");
    }
    if !means.removed || !baseline.removed {
        return Verdict::of("miss", "not_removed");
    }
    if removals(&means.changed, places, origin).deciding > 0 {
        return Verdict::of("miss", "removed_outside");
    }
    if means.dumps_inside_profile == 0 {
        return Verdict::of("notrun", "no_dump_inside");
    }
    if means.handler != "gone" {
        return Verdict::of("notrun", "handler");
    }
    Verdict::of("ok", "-")
}

/// The word of a reading's crash handler (CP-186 T2 (d), CP-189 G8), read from `table` for the
/// processes that began at or after `since_us`: `runs` when one is a crash handler (its program's
/// file name holds `crashpad_handler`, or an argument holds `--type=crashpad-handler`); `unread`
/// when the table could not be read, or when the arguments of a process of the browser's
/// `program` could not be read, for then nothing shows whether it is a handler (CP-145: an unread
/// table is never an empty one); `gone` otherwise. The row reads every word but `gone` as notrun.
pub fn handler_state(
    table: Option<&[crate::procs::Proc]>,
    since_us: u64,
    program: &Path,
    args: &dyn Fn(u32) -> Option<Vec<String>>,
) -> &'static str {
    let Some(table) = table else {
        return "unread";
    };
    let program = program.to_string_lossy();
    let mut unread = false;
    for p in table.iter().filter(|p| p.started_us >= since_us) {
        let named = p
            .exe
            .as_deref()
            .and_then(|e| e.file_name())
            .is_some_and(|n| n.to_string_lossy().contains("crashpad_handler"));
        let a = args(p.pid);
        if named
            || a.as_ref()
                .is_some_and(|a| a.iter().any(|x| x.contains("--type=crashpad-handler")))
        {
            return "runs";
        }
        let of_browser = p.exe.as_deref().is_some_and(|e| {
            crate::checks::same_path_on(&e.to_string_lossy(), &program, cfg!(windows))
        });
        if of_browser && a.is_none() {
            unread = true;
        }
    }
    if unread {
        "unread"
    } else {
        "gone"
    }
}

/// The places of a run on this system, with the run's `profile` and `temp` and the account's
/// directories read from the system (macOS: `HOME` and the per-user directories of `confstr`;
/// Windows: `LOCALAPPDATA`, `USERPROFILE`, `TEMP` and `ProgramData` of this program's own
/// environment).
pub fn places(profile: &Path, temp: &Path) -> Vec<Place> {
    let mut out = vec![
        place("profile", profile.to_path_buf(), true, false),
        place("temp", temp.to_path_buf(), true, false),
    ];
    out.extend(sys::account_places());
    out
}

fn place(name: &'static str, root: PathBuf, inside_profile: bool, system: bool) -> Place {
    Place {
        name,
        root,
        inside_profile,
        system,
    }
}

/// The crash place of a browser of `kind` (`chrome` or `edge`) on this system, where its Crashpad
/// database lies for the account's own browser.
pub fn crash_place(kind: &str) -> Option<PathBuf> {
    sys::crash_place(kind)
}

/// Whether uploads are on in the Crashpad database at `place`, read from its `settings.dat`
/// (Crashpad's `Settings`). It reads bytes 0 to 3, the magic number `'CPds'`, and bytes 8 to 11,
/// the options, whose first bit is uploads enabled, in the byte order that the magic shows: the
/// bytes `sdPC` are the magic written little-endian, the bytes `CPds` big-endian (CP-186 S5).
/// Bytes 4 to 7, the version, are not read. `None` when the file is absent or not of that form.
/// Reads the file, changes nothing.
pub fn send_setting(place: &Path) -> Option<bool> {
    let bytes = std::fs::read(place.join("settings.dat")).ok()?;
    if bytes.len() < 12 {
        return None;
    }
    let raw = [bytes[8], bytes[9], bytes[10], bytes[11]];
    let options = match &bytes[0..4] {
        b"sdPC" => u32::from_le_bytes(raw),
        b"CPds" => u32::from_be_bytes(raw),
        _ => return None,
    };
    Some(options & 1 == 1)
}

#[cfg(target_os = "macos")]
mod sys {
    use super::{place, Place};
    use std::path::PathBuf;

    fn confstr(name: libc::c_int) -> Option<PathBuf> {
        let mut buf = vec![0u8; 1024];
        // SAFETY: the buffer holds `buf.len()` bytes, which the call is told.
        let n = unsafe { libc::confstr(name, buf.as_mut_ptr().cast(), buf.len()) };
        if n == 0 || n > buf.len() {
            return None;
        }
        buf.truncate(n - 1);
        Some(PathBuf::from(String::from_utf8_lossy(&buf).into_owned()))
    }

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    pub(super) fn crash_place(kind: &str) -> Option<PathBuf> {
        let support = home()?.join("Library").join("Application Support");
        Some(match kind {
            "edge" => support.join("Microsoft Edge").join("Crashpad"),
            _ => support.join("Google").join("Chrome").join("Crashpad"),
        })
    }

    pub(super) fn account_places() -> Vec<Place> {
        let mut out = Vec::new();
        if let Some(c) = crash_place("chrome") {
            out.push(place("chrome_crashpad", c, false, false));
        }
        if let Some(c) = crash_place("edge") {
            out.push(place("edge_crashpad", c, false, false));
        }
        if let Some(cache) = confstr(libc::_CS_DARWIN_USER_CACHE_DIR) {
            out.push(place(
                "chrome_helper_cache",
                cache.join("com.google.Chrome.helper"),
                false,
                false,
            ));
            out.push(place(
                "edge_helper_cache",
                cache.join("com.microsoft.edgemac.helper"),
                false,
                false,
            ));
        }
        if let Some(user) = confstr(libc::_CS_DARWIN_USER_DIR) {
            out.push(place(
                "chrome_code_sign_clone",
                user.join("X").join("com.google.Chrome.code_sign_clone"),
                false,
                false,
            ));
        }
        if let Some(h) = home() {
            out.push(place(
                "edge_widgets",
                h.join("Library")
                    .join("Group Containers")
                    .join("UBF8T346G9.com.microsoft.edgemac.widgets"),
                false,
                false,
            ));
            out.push(place(
                "user_reports",
                h.join("Library").join("Logs").join("DiagnosticReports"),
                false,
                true,
            ));
        }
        out.push(place(
            "system_reports",
            PathBuf::from("/Library/Logs/DiagnosticReports"),
            false,
            true,
        ));
        if let Some(t) = confstr(libc::_CS_DARWIN_USER_TEMP_DIR) {
            out.push(place("system_temp", t, false, false));
        }
        out
    }
}

#[cfg(windows)]
mod sys {
    use super::{place, Place};
    use std::path::PathBuf;

    fn var(name: &str) -> Option<PathBuf> {
        std::env::var_os(name).map(PathBuf::from)
    }

    pub(super) fn crash_place(kind: &str) -> Option<PathBuf> {
        let local = var("LOCALAPPDATA")?;
        Some(match kind {
            "edge" => local
                .join("Microsoft")
                .join("Edge")
                .join("User Data")
                .join("Crashpad"),
            _ => local
                .join("Google")
                .join("Chrome")
                .join("User Data")
                .join("Crashpad"),
        })
    }

    pub(super) fn account_places() -> Vec<Place> {
        let mut out = Vec::new();
        if let Some(c) = crash_place("chrome") {
            out.push(place("chrome_crashpad", c, false, false));
        }
        if let Some(c) = crash_place("edge") {
            out.push(place("edge_crashpad", c, false, false));
        }
        if let Some(profile) = var("USERPROFILE") {
            out.push(place(
                "cryptnet_cache",
                profile
                    .join("AppData")
                    .join("LocalLow")
                    .join("Microsoft")
                    .join("CryptnetUrlCache")
                    .join("MetaData"),
                false,
                false,
            ));
        }
        if let Some(local) = var("LOCALAPPDATA") {
            out.push(place(
                "token_broker_cache",
                local.join("Microsoft").join("TokenBroker").join("Cache"),
                false,
                false,
            ));
            out.push(place(
                "user_crash_dumps",
                local.join("CrashDumps"),
                false,
                true,
            ));
            out.push(place(
                "user_reports",
                local.join("Microsoft").join("Windows").join("WER"),
                false,
                true,
            ));
        }
        if let Some(data) = var("ProgramData") {
            out.push(place(
                "system_reports",
                data.join("Microsoft").join("Windows").join("WER"),
                false,
                true,
            ));
        }
        if let Some(t) = var("TEMP") {
            out.push(place("system_temp", t, false, false));
        }
        out
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod sys {
    use super::Place;
    use std::path::PathBuf;
    pub(super) fn crash_place(_kind: &str) -> Option<PathBuf> {
        None
    }
    pub(super) fn account_places() -> Vec<Place> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> (PathBuf, Vec<Place>) {
        let dir = crate::child::scratch(name);
        let places = vec![
            place("profile", dir.join("profile"), true, false),
            place("crashpad", dir.join("Crashpad"), false, false),
            place("reports", dir.join("reports"), false, true),
        ];
        for p in &places {
            std::fs::create_dir_all(&p.root).unwrap();
        }
        std::fs::write(dir.join("Crashpad").join("settings.dat"), b"old").unwrap();
        (dir, places)
    }

    /// CP-167 E3: a new file deep under a named place is in the listing of what changed.
    #[test]
    fn a_new_file_under_a_named_place_is_listed() {
        let (dir, places) = tree("listing-new");
        let before = snapshot(&places);
        let deep = dir.join("Crashpad").join("pending").join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("x.dmp"), b"dump").unwrap();
        let got = changed(&before, &snapshot(&places));
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].place, "crashpad");
        assert_eq!(got[0].rel, "pending/a/b/x.dmp");
        assert!(is_dump(&got[0]));
    }

    /// CP-167 E3: a file of a dump's size that appears is reported with its size, and a file that
    /// changed in size is reported too; a file that did not change is not.
    #[test]
    fn a_file_of_a_dump_s_size_is_reported_with_its_size() {
        let (dir, places) = tree("listing-size");
        std::fs::write(dir.join("reports").join("kept.txt"), b"same").unwrap();
        let before = snapshot(&places);
        std::fs::write(
            dir.join("reports").join("Chrome.ips"),
            vec![7u8; 300 * 1024],
        )
        .unwrap();
        std::fs::write(dir.join("Crashpad").join("settings.dat"), b"longer").unwrap();
        let got = changed(&before, &snapshot(&places));
        let sizes: Vec<(&str, u64)> = got.iter().map(|f| (f.rel.as_str(), f.size)).collect();
        assert!(sizes.contains(&("Chrome.ips", 300 * 1024)), "{sizes:?}");
        assert!(sizes.contains(&("settings.dat", 6)), "{sizes:?}");
        assert!(!sizes.iter().any(|(r, _)| *r == "kept.txt"), "{sizes:?}");
    }

    fn row_places() -> Vec<Place> {
        vec![
            place("profile", PathBuf::from("/p"), true, false),
            place("crashpad", PathBuf::from("/c"), false, false),
            place("reports", PathBuf::from("/r"), false, true),
        ]
    }

    fn file(place: &'static str, rel: &str, size: u64) -> File {
        File {
            place,
            rel: rel.into(),
            size,
            modified: 1,
            link: false,
            removed: false,
        }
    }

    fn link(place: &'static str, rel: &str, size: u64) -> File {
        File {
            link: true,
            ..file(place, rel, size)
        }
    }

    /// Run 43461032070: the browser's singleton directory in the system's temporary directory has
    /// a random name at each start and holds links whose size is a random cookie's length; the
    /// same links under another random name and of another size are no file beyond the start with
    /// no crash, and a regular file of another size or a new name is.
    #[test]
    fn the_singleton_s_random_names_and_link_sizes_are_not_beyond() {
        let baseline = vec![
            link("temp", ".com.google.Chrome.Ab12Cd/SingletonCookie", 19),
            link("temp", ".com.google.Chrome.Ab12Cd/SingletonSocket", 0),
        ];
        let crash = vec![
            link("temp", ".com.google.Chrome.Zx98Yw/SingletonCookie", 20),
            link("temp", ".com.google.Chrome.Zx98Yw/SingletonSocket", 0),
        ];
        assert!(beyond(&crash, &baseline).is_empty());
        let file_of_other_size = vec![file("temp", ".com.google.Chrome.Zx98Yw/x", 3)];
        let base_file = vec![file("temp", ".com.google.Chrome.Ab12Cd/x", 2)];
        assert_eq!(beyond(&file_of_other_size, &base_file).len(), 1);
        let other_name = vec![link("temp", ".com.google.Chrome.Zx98Yw/Other", 20)];
        assert_eq!(beyond(&other_name, &baseline).len(), 1);
        assert_eq!(
            name_key("a/com.microsoft.edgemac.Q1w2E3/b"),
            "a/com.microsoft.edgemac.*/b"
        );
        assert_eq!(
            name_key("a/com.google.Chrome.toolong1/b"),
            "a/com.google.Chrome.toolong1/b"
        );
    }

    /// The facts of a reading whose crash was taken: `changed` beside one dump inside the
    /// profile, its handler gone, both directories removed.
    fn taken(changed: Vec<File>) -> Facts {
        Facts {
            changed,
            dumps_inside_profile: 1,
            handler: "gone".into(),
            removed: true,
        }
    }

    /// The facts of a start with no crash: its browser still runs at the listing.
    fn start(changed: Vec<File>) -> Facts {
        Facts {
            handler: "stop_wait".into(),
            dumps_inside_profile: 0,
            ..taken(changed)
        }
    }

    fn res(places: &[Place], b: &Facts, m: &Facts, c: &[File], sent: usize) -> &'static str {
        result(places, &Listing::new(), b, m, c, sent).result
    }

    /// CP-186 T4 (c), with the review's case crash/07: a file under a listed place that is written
    /// again at its own size with another time of change is in the listing.
    #[test]
    fn t4_a_file_written_again_at_its_size_with_another_time_is_listed() {
        let (dir, places) = tree("listing-time");
        let path = dir.join("Crashpad").join("settings.dat");
        let before = snapshot(&places);
        let then = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, b"new").unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_modified(then + std::time::Duration::from_millis(1_500))
            .unwrap();
        drop(f);
        let got = changed(&before, &snapshot(&places));
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!((got[0].rel.as_str(), got[0].size), ("settings.dat", 3));
        assert!(!got[0].removed);
        // Two times inside one second: the listing keeps the system's grain and does not cut the
        // time to seconds.
        let second = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let set = |at: std::time::SystemTime| {
            std::fs::write(&path, b"one").unwrap();
            let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            f.set_modified(at).unwrap();
        };
        set(second + std::time::Duration::from_millis(100));
        let early = snapshot(&places);
        set(second + std::time::Duration::from_millis(400));
        let got = changed(&early, &snapshot(&places));
        assert_eq!(got.len(), 1, "{got:?}");
    }

    /// CP-186 T4 (d), with the review's case crash/08: a dump by each ending and under each of the
    /// three directories of a Crashpad database, as the first part of the path and deeper, and the
    /// files that are no dump.
    #[test]
    fn t4_is_dump_by_ending_and_by_directory() {
        let dump = |rel: &str| is_dump(&file("c", rel, 1));
        for rel in [
            "x.dmp",
            "a/x.DMP",
            "x.ips",
            "x.crash",
            "x.wer",
            "x.hdmp",
            "x.mdmp",
            "pending/x",
            "completed/x",
            "new/x",
            "db/pending/x",
            "db/completed/x",
            "db/new/x",
        ] {
            assert!(dump(rel), "{rel}");
        }
        for rel in [
            "settings.dat",
            "x.dmpx",
            "x.ips.txt",
            "dmp",
            "xpending/x",
            "notcompleted/x",
            "renew/x",
            "db/xpending/x",
            "pending",
        ] {
            assert!(!dump(rel), "{rel}");
        }
    }

    /// CP-186 S6, with the review's case crash/04: a scratch directory under a listed place other
    /// than the run's profile is named; one beside the places is not.
    #[test]
    fn s6_a_scratch_directory_under_a_listed_place_is_named() {
        let base = crate::child::scratch("scratch-under");
        let places = vec![
            place("profile", base.join("row").join("p-means"), true, false),
            place(SYSTEM_TEMP, base.clone(), false, false),
            place("crashpad", base.join("elsewhere"), false, false),
        ];
        assert_eq!(
            scratch_under_place(&base.join("row"), &places),
            Some(SYSTEM_TEMP)
        );
        let beside = crate::child::scratch("scratch-beside");
        assert_eq!(scratch_under_place(&beside, &places), None);
    }

    /// CP-186 T3, the review's case crash/03: a file that stood before and is gone after is in the
    /// listing, marked as removed.
    #[test]
    fn t3_a_file_removed_outside_the_profile_is_in_the_listing() {
        let mut before = Listing::new();
        before.insert(
            ("account_db", "completed/older-report.dmp".to_string()),
            (4096, 10, false),
        );
        let got = changed(&before, &Listing::new());
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].removed);
        assert_eq!(got[0].rel, "completed/older-report.dmp");
    }

    /// CP-186 T3: a removal at the account's crash place of a file that stood before the row reads
    /// miss; the same for a file that an earlier reading of the row left decides nothing; a
    /// removal in the system's temporary directory is counted and decides nothing.
    #[test]
    fn t3_a_removal_outside_the_profile_of_a_file_that_stood_before_reads_miss() {
        let places = vec![
            place("profile", PathBuf::from("/p"), true, false),
            place("crashpad", PathBuf::from("/c"), false, false),
            place(SYSTEM_TEMP, PathBuf::from("/t"), false, false),
            place("reports", PathBuf::from("/r"), false, true),
        ];
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let mut origin = Listing::new();
        origin.insert(("crashpad", "settings.dat".to_string()), (40, 1, false));
        let gone = |place: &'static str, rel: &str| File {
            removed: true,
            ..file(place, rel, 40)
        };
        let before_row = taken(vec![gone("crashpad", "settings.dat")]);
        assert_eq!(
            result(&places, &origin, &start(vec![]), &before_row, &control, 0),
            Verdict::of("miss", "removed_outside")
        );
        assert_eq!(
            removals(&before_row.changed, &places, &origin),
            Removals {
                deciding: 1,
                own: 0,
                temp: 0
            }
        );
        let own = taken(vec![gone("crashpad", "completed/earlier-reading.dmp")]);
        assert_eq!(
            result(&places, &origin, &start(vec![]), &own, &control, 0),
            Verdict::of("ok", "-")
        );
        assert_eq!(removals(&own.changed, &places, &origin).own, 1);
        let temp = taken(vec![gone(SYSTEM_TEMP, "other-program.lock")]);
        assert_eq!(
            result(&places, &origin, &start(vec![]), &temp, &control, 0),
            Verdict::of("ok", "-")
        );
        assert_eq!(removals(&temp.changed, &places, &origin).temp, 1);
        let gone_dump = vec![gone("crashpad", "completed/abc.dmp")];
        assert_eq!(
            result(
                &places,
                &origin,
                &start(vec![]),
                &taken(vec![]),
                &gone_dump,
                0
            )
            .cause,
            "control_no_dump"
        );
    }

    /// CP-167 E3: a row whose control left no dump reads notrun, never ok, also when the crash
    /// with the means left nothing.
    #[test]
    fn a_control_with_no_dump_reads_notrun() {
        let places = row_places();
        let (b, m) = (start(vec![]), taken(vec![]));
        let control = vec![file("crashpad", "settings.dat", 40)];
        assert_eq!(
            result(&places, &Listing::new(), &b, &m, &control, 0),
            Verdict::of("notrun", "control_no_dump")
        );
        assert_eq!(res(&places, &b, &m, &[], 0), "notrun");
        let control = vec![file("crashpad", "pending/abc.dmp", 4096)];
        assert_eq!(res(&places, &b, &m, &control, 0), "ok");
        let inside = vec![file("profile", "Crashpad/reports/abc.dmp", 4096)];
        assert_eq!(res(&places, &b, &m, &inside, 0), "ok");
    }

    /// CP-186 T2 (a), the review's case crash/02: a control whose only dump is a report of the
    /// system reads notrun.
    #[test]
    fn t2_a_control_whose_only_dump_is_a_system_report_reads_notrun() {
        let places = row_places();
        let control = vec![file("reports", "OtherApp-2026-11-13-120000.ips", 9000)];
        assert_eq!(
            result(
                &places,
                &Listing::new(),
                &start(vec![]),
                &taken(vec![]),
                &control,
                0
            ),
            Verdict::of("notrun", "control_no_dump")
        );
    }

    /// CP-186 T2 (c), the review's case crash/01: a reading with the means that shows no dump
    /// inside its profile reads notrun, for nothing shows that its crash was taken.
    #[test]
    fn t2_a_means_reading_with_no_dump_inside_reads_notrun() {
        let places = row_places();
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let means = Facts {
            dumps_inside_profile: 0,
            ..taken(vec![])
        };
        assert_eq!(
            result(
                &places,
                &Listing::new(),
                &start(vec![]),
                &means,
                &control,
                0
            ),
            Verdict::of("notrun", "no_dump_inside")
        );
    }

    /// CP-186 T2 (d): a reading with the means whose crash handler still ran when the listing was
    /// taken reads notrun; the start with no crash, whose browser still runs, decides nothing by
    /// its handler.
    #[test]
    fn t2_a_handler_that_still_runs_reads_notrun() {
        let places = row_places();
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let means = Facts {
            handler: "stop_wait".into(),
            ..taken(vec![])
        };
        assert_eq!(
            result(
                &places,
                &Listing::new(),
                &start(vec![]),
                &means,
                &control,
                0
            ),
            Verdict::of("notrun", "handler")
        );
        assert_eq!(
            res(&places, &start(vec![]), &taken(vec![]), &control, 0),
            "ok"
        );
    }

    /// CP-186 T2 (b): a profile that the stop left, in the reading with the means or in the start
    /// with no crash, reads miss.
    #[test]
    fn t2_a_profile_that_the_stop_left_reads_miss() {
        let places = row_places();
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let means = Facts {
            removed: false,
            ..taken(vec![])
        };
        assert_eq!(
            result(
                &places,
                &Listing::new(),
                &start(vec![]),
                &means,
                &control,
                0
            ),
            Verdict::of("miss", "not_removed")
        );
        let base = Facts {
            removed: false,
            ..start(vec![])
        };
        assert_eq!(
            result(&places, &Listing::new(), &base, &taken(vec![]), &control, 0),
            Verdict::of("miss", "not_removed")
        );
    }

    /// CP-186 T2 (b) before (c): a file beyond the start with no crash together with no dump
    /// inside the profile reads miss, not notrun.
    #[test]
    fn t2_a_file_beyond_with_no_dump_inside_reads_miss() {
        let places = row_places();
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let means = Facts {
            dumps_inside_profile: 0,
            ..taken(vec![file("crashpad", "completed/x.dmp", 9)])
        };
        assert_eq!(
            result(
                &places,
                &Listing::new(),
                &start(vec![]),
                &means,
                &control,
                0
            ),
            Verdict::of("miss", "beyond")
        );
    }

    /// CP-186 T2: the four cells of run 43484944398, written out as lists, read ok.
    #[test]
    fn t2_the_four_cells_of_run_9_read_ok() {
        let mac_places = |own: &'static str| {
            vec![
                place("profile", PathBuf::from("/p"), true, false),
                place(own, PathBuf::from("/crashpad"), false, false),
                place("chrome_helper_cache", PathBuf::from("/h"), false, false),
                place("edge_helper_cache", PathBuf::from("/eh"), false, false),
                place("system_temp", PathBuf::from("/t"), false, false),
                place("user_reports", PathBuf::from("/r"), false, true),
            ]
        };
        let start_left = |cache: &'static str| {
            vec![
                file(cache, "a/cache.db", 2048),
                file(cache, "a/cache.db-wal", 2048),
                link(
                    "system_temp",
                    ".com.google.Chrome.Ab12Cd/SingletonCookie",
                    19,
                ),
                link(
                    "system_temp",
                    ".com.google.Chrome.Ab12Cd/SingletonSocket",
                    0,
                ),
            ]
        };
        let means_left = |cache: &'static str| {
            let mut v = vec![
                file(cache, "a/cache.db", 2048),
                file(cache, "a/cache.db-wal", 2048),
                link(
                    "system_temp",
                    ".com.google.Chrome.Zx98Yw/SingletonCookie",
                    20,
                ),
                link(
                    "system_temp",
                    ".com.google.Chrome.Zx98Yw/SingletonSocket",
                    0,
                ),
            ];
            v.push(file("profile", "Crashpad/completed/m.dmp", 400_000));
            v
        };
        let chrome_mac = mac_places("chrome_crashpad");
        let control = vec![file("chrome_crashpad", "completed/c.dmp", 1_378_480)];
        assert_eq!(
            result(
                &chrome_mac,
                &Listing::new(),
                &start(start_left("chrome_helper_cache")),
                &taken(means_left("chrome_helper_cache")),
                &control,
                0
            ),
            Verdict::of("ok", "-")
        );
        let edge_mac = mac_places("edge_crashpad");
        let mut edge_means = means_left("edge_helper_cache");
        edge_means.push(file(
            "user_reports",
            "Microsoft Edge-2026-11-13.ips",
            98_801,
        ));
        let edge_control = vec![
            file("edge_crashpad", "completed/c.dmp", 1_318_227),
            file("user_reports", "Microsoft Edge-2026-11-13.ips", 98_801),
        ];
        assert_eq!(
            result(
                &edge_mac,
                &Listing::new(),
                &start(start_left("edge_helper_cache")),
                &taken(edge_means),
                &edge_control,
                0
            ),
            Verdict::of("ok", "-")
        );
        let win = vec![
            place("profile", PathBuf::from("C:/p"), true, false),
            place("chrome_crashpad", PathBuf::from("C:/c"), false, false),
            place("edge_crashpad", PathBuf::from("C:/e"), false, false),
            place("system_temp", PathBuf::from("C:/t"), false, false),
            place("system_reports", PathBuf::from("C:/r"), false, true),
        ];
        for size in [1_539_184, 7_031_487] {
            let control = vec![file("profile", "Crashpad/reports/c.dmp", size)];
            let means = taken(vec![file("profile", "Crashpad/reports/m.dmp", size)]);
            assert_eq!(
                result(&win, &Listing::new(), &start(vec![]), &means, &control, 0),
                Verdict::of("ok", "-")
            );
        }
    }

    /// CP-167 E3: a row whose crash with the means left a file that the start with no crash did
    /// not leave reads miss, by name and by size; one that left only what the start with no crash
    /// left reads ok; anything sent after the crash reads miss.
    #[test]
    fn a_crash_with_the_means_that_left_a_file_reads_miss() {
        let places = row_places();
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let baseline = start(vec![file("crashpad", "settings.dat", 40)]);
        let same = taken(vec![file("crashpad", "settings.dat", 40)]);
        assert_eq!(res(&places, &baseline, &same, &control, 0), "ok");
        let extra = taken(vec![
            file("crashpad", "settings.dat", 40),
            file("crashpad", "new/x.dmp", 9),
        ]);
        assert_eq!(res(&places, &baseline, &extra, &control, 0), "miss");
        let resized = taken(vec![file("crashpad", "settings.dat", 41)]);
        assert_eq!(res(&places, &baseline, &resized, &control, 0), "miss");
        assert_eq!(
            result(&places, &Listing::new(), &baseline, &same, &control, 1),
            Verdict::of("miss", "sent")
        );
        let reported = taken(vec![
            file("crashpad", "settings.dat", 40),
            file("reports", "Chrome.ips", 9),
        ]);
        assert_eq!(res(&places, &baseline, &reported, &control, 0), "ok");
    }

    #[test]
    fn what_is_left_outside_drops_the_profile_s_files() {
        let places = vec![
            place("profile", PathBuf::from("/p"), true, false),
            place("crashpad", PathBuf::from("/c"), false, false),
        ];
        let files = vec![
            file("profile", "Crashpad/x.dmp", 1),
            file("crashpad", "y", 2),
        ];
        let left = left_outside(&files, &places);
        assert_eq!(left, vec![file("crashpad", "y", 2)]);
    }

    #[test]
    fn the_send_setting_is_read_from_settings_dat_and_nothing_else_is_taken() {
        let dir = crate::child::scratch("send-setting");
        std::fs::write(dir.join("settings.dat"), b"sdPC\x01\0\0\0\x01\0\0\0").unwrap();
        assert_eq!(send_setting(&dir), Some(true));
        std::fs::write(dir.join("settings.dat"), b"sdPC\x01\0\0\0\0\0\0\0").unwrap();
        assert_eq!(send_setting(&dir), Some(false));
        // CP-186 S5, the review's case crash/06: the magic's bytes in big-endian order, the
        // options in that order too.
        let mut data = b"CPds".to_vec();
        data.extend(1u32.to_be_bytes());
        data.extend(1u32.to_be_bytes());
        std::fs::write(dir.join("settings.dat"), &data).unwrap();
        assert_eq!(send_setting(&dir), Some(true));
        data[11] = 0;
        std::fs::write(dir.join("settings.dat"), &data).unwrap();
        assert_eq!(send_setting(&dir), Some(false));
        let mut le_options = b"CPds".to_vec();
        le_options.extend(1u32.to_be_bytes());
        le_options.extend(1u32.to_le_bytes());
        std::fs::write(dir.join("settings.dat"), &le_options).unwrap();
        assert_eq!(send_setting(&dir), Some(false));
        std::fs::write(dir.join("settings.dat"), b"other").unwrap();
        assert_eq!(send_setting(&dir), None);
    }

    /// CP-189 G8, the review's point 3: an unread process table, or unread arguments of a process
    /// of the browser's program that began in the reading, reads `handler=unread`, which the row
    /// reads notrun (CP-145: an unread table is never an empty one).
    #[test]
    fn g8_an_unread_table_or_unread_arguments_of_the_browser_read_handler_unread() {
        use crate::procs::Proc;
        let program = Path::new("/B.app/Contents/MacOS/b");
        let p = |pid: u32, exe: &str, started_us: u64| Proc {
            pid,
            ppid: 1,
            started_us,
            pgid: pid,
            exe: Some(PathBuf::from(exe)),
        };
        let none = |_: u32| -> Option<Vec<String>> { None };
        let plain = |_: u32| Some(vec!["--flag".to_string()]);
        let by_arg = |_: u32| Some(vec!["--type=crashpad-handler".to_string()]);
        assert_eq!(handler_state(None, 10, program, &plain), "unread");
        let browser = [p(5, "/B.app/Contents/MacOS/b", 20)];
        assert_eq!(handler_state(Some(&browser), 10, program, &none), "unread");
        assert_eq!(handler_state(Some(&browser), 10, program, &plain), "gone");
        assert_eq!(handler_state(Some(&browser), 10, program, &by_arg), "runs");
        let other = [p(6, "/usr/bin/other", 20)];
        assert_eq!(handler_state(Some(&other), 10, program, &none), "gone");
        let older = [p(5, "/B.app/Contents/MacOS/b", 5)];
        assert_eq!(handler_state(Some(&older), 10, program, &none), "gone");
        let handler = [
            p(5, "/B.app/Contents/MacOS/b", 20),
            p(
                7,
                "/B.app/Contents/Frameworks/x/chrome_crashpad_handler",
                20,
            ),
        ];
        assert_eq!(handler_state(Some(&handler), 10, program, &none), "runs");
        let means = Facts {
            handler: "unread".into(),
            ..taken(vec![])
        };
        assert_eq!(
            result(
                &row_places(),
                &Listing::new(),
                &start(vec![]),
                &means,
                &[file("crashpad", "completed/abc.dmp", 4096)],
                0
            ),
            Verdict::of("notrun", "handler")
        );
    }

    /// CP-189 G8, T2's order: what was sent after the crash reads `miss` (`sent`) before a profile
    /// that the stop left, a removal outside the profile, no dump inside and a handler that ran.
    #[test]
    fn g8_t2_sent_reads_before_what_follows_it() {
        let places = vec![
            place("profile", PathBuf::from("/p"), true, false),
            place("crashpad", PathBuf::from("/c"), false, false),
        ];
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let mut origin = Listing::new();
        origin.insert(("crashpad", "settings.dat".to_string()), (40, 1, false));
        let means = Facts {
            removed: false,
            dumps_inside_profile: 0,
            handler: "stop_wait".into(),
            ..taken(vec![File {
                removed: true,
                ..file("crashpad", "settings.dat", 40)
            }])
        };
        assert_eq!(
            result(&places, &origin, &start(vec![]), &means, &control, 1),
            Verdict::of("miss", "sent")
        );
    }

    /// CP-189 G8, T2's order: a profile that the stop left reads `miss` (`not_removed`) before a
    /// removal outside the profile, no dump inside and a handler that ran.
    #[test]
    fn g8_t2_not_removed_reads_before_what_follows_it() {
        let places = vec![
            place("profile", PathBuf::from("/p"), true, false),
            place("crashpad", PathBuf::from("/c"), false, false),
        ];
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let mut origin = Listing::new();
        origin.insert(("crashpad", "settings.dat".to_string()), (40, 1, false));
        let means = Facts {
            removed: false,
            dumps_inside_profile: 0,
            handler: "stop_wait".into(),
            ..taken(vec![File {
                removed: true,
                ..file("crashpad", "settings.dat", 40)
            }])
        };
        assert_eq!(
            result(&places, &origin, &start(vec![]), &means, &control, 0),
            Verdict::of("miss", "not_removed")
        );
    }

    /// CP-189 G8, T2's order: a removal outside the profile of a file that stood before the row
    /// reads `miss` (`removed_outside`) before no dump inside and a handler that ran.
    #[test]
    fn g8_t2_removed_outside_reads_before_what_follows_it() {
        let places = vec![
            place("profile", PathBuf::from("/p"), true, false),
            place("crashpad", PathBuf::from("/c"), false, false),
        ];
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let mut origin = Listing::new();
        origin.insert(("crashpad", "settings.dat".to_string()), (40, 1, false));
        let means = Facts {
            dumps_inside_profile: 0,
            handler: "stop_wait".into(),
            ..taken(vec![File {
                removed: true,
                ..file("crashpad", "settings.dat", 40)
            }])
        };
        assert_eq!(
            result(&places, &origin, &start(vec![]), &means, &control, 0),
            Verdict::of("miss", "removed_outside")
        );
    }

    /// CP-189 G8, T3's first exemption: a removal at the account's crash place of a file that an
    /// earlier reading of the row left, and that the listing before the row lacks, is the row's
    /// own and decides nothing; it is counted as own.
    #[test]
    fn g8_t3_a_removal_of_what_an_earlier_reading_left_decides_nothing() {
        let places = vec![
            place("profile", PathBuf::from("/p"), true, false),
            place("crashpad", PathBuf::from("/c"), false, false),
        ];
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let origin = Listing::new();
        let means = taken(vec![File {
            removed: true,
            ..file("crashpad", "completed/earlier-reading.dmp", 4096)
        }]);
        assert_eq!(
            removals(&means.changed, &places, &origin),
            Removals {
                deciding: 0,
                own: 1,
                temp: 0
            }
        );
        assert_eq!(
            result(&places, &origin, &start(vec![]), &means, &control, 0),
            Verdict::of("ok", "-")
        );
    }

    /// CP-189 G8, T3's second exemption: a removal in the system's temporary directory of a file
    /// that stood before the row decides nothing; it is counted as temp.
    #[test]
    fn g8_t3_a_removal_in_the_system_temp_decides_nothing() {
        let places = vec![
            place("profile", PathBuf::from("/p"), true, false),
            place("crashpad", PathBuf::from("/c"), false, false),
            place(SYSTEM_TEMP, PathBuf::from("/t"), false, false),
        ];
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let mut origin = Listing::new();
        origin.insert(
            (SYSTEM_TEMP, "other-program.lock".to_string()),
            (4, 1, false),
        );
        let means = taken(vec![File {
            removed: true,
            ..file(SYSTEM_TEMP, "other-program.lock", 4)
        }]);
        assert_eq!(
            removals(&means.changed, &places, &origin),
            Removals {
                deciding: 0,
                own: 0,
                temp: 1
            }
        );
        assert_eq!(
            result(&places, &origin, &start(vec![]), &means, &control, 0),
            Verdict::of("ok", "-")
        );
    }

    /// CP-189 G8: a dump that another program left in the system's temporary directory during the
    /// control is not the control's, so a control with no other dump reads notrun.
    #[test]
    fn g8_a_dump_in_the_system_temp_during_the_control_is_not_the_control_s() {
        let places = vec![
            place("profile", PathBuf::from("/p"), true, false),
            place("crashpad", PathBuf::from("/c"), false, false),
            place(SYSTEM_TEMP, PathBuf::from("/t"), false, false),
        ];
        let control = vec![file(SYSTEM_TEMP, "other-program.dmp", 4096)];
        assert_eq!(
            result(
                &places,
                &Listing::new(),
                &start(vec![]),
                &taken(vec![]),
                &control,
                0
            ),
            Verdict::of("notrun", "control_no_dump")
        );
        let control = vec![
            file(SYSTEM_TEMP, "other-program.dmp", 4096),
            file("crashpad", "completed/abc.dmp", 4096),
        ];
        assert_eq!(
            result(
                &places,
                &Listing::new(),
                &start(vec![]),
                &taken(vec![]),
                &control,
                0
            ),
            Verdict::of("ok", "-")
        );
    }
}
