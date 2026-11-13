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
/// change in seconds since the Unix epoch.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct File {
    pub place: &'static str,
    pub rel: String,
    pub size: u64,
    pub modified: u64,
    /// Whether the entry is a symbolic link, whose size is the length of its target.
    pub link: bool,
}

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
            .map_or(0, |d| d.as_secs());
        out.insert(
            (place, rel),
            (meta.len(), modified, meta.file_type().is_symlink()),
        );
    }
}

/// The files of `after` that are new against `before` or changed in size or time.
pub fn changed(
    before: &BTreeMap<(&'static str, String), (u64, u64, bool)>,
    after: &BTreeMap<(&'static str, String), (u64, u64, bool)>,
) -> Vec<File> {
    after
        .iter()
        .filter(|(k, v)| before.get(*k) != Some(*v))
        .map(|((place, rel), (size, modified, link))| File {
            place,
            rel: rel.clone(),
            size: *size,
            modified: *modified,
            link: *link,
        })
        .collect()
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

/// What one reading left outside the profile and the temporary directory, which the stop removes.
pub fn left_outside(changed: &[File], places: &[Place]) -> Vec<File> {
    changed
        .iter()
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

/// The row's result of one system and browser (CP-165 (2) (b)): `notrun` when the control left no
/// dump at any place of the listing, the profile among them (`control` is everything that changed
/// in the control's reading), for then nothing showed that the crash was taken as one; `miss` when the crash with the
/// means left a file outside the profile that the start with no crash did not leave, or when
/// anything was sent after the crash; `ok` otherwise. A file in a report place of the system
/// (`Place::system`) is recorded with no promise and decides nothing (CP-165 (2) (d) and (g)).
pub fn result(
    places: &[Place],
    baseline: &[File],
    means: &[File],
    control: &[File],
    sent_after_crash: usize,
) -> &'static str {
    if !control.iter().any(is_dump) {
        return "notrun";
    }
    let deciding = |f: &&File| !places.iter().any(|p| p.name == f.place && p.system);
    if beyond(means, baseline).iter().any(|f| deciding(&f)) || sent_after_crash > 0 {
        return "miss";
    }
    "ok"
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
/// (Crashpad's `Settings`: a magic number `CPds`, in either byte order, a version, then the
/// options, whose first bit is uploads enabled). `None` when the file is absent or not of that form. Reads the file, changes
/// nothing.
pub fn send_setting(place: &Path) -> Option<bool> {
    let bytes = std::fs::read(place.join("settings.dat")).ok()?;
    if bytes.len() < 12 || (&bytes[0..4] != b"CPds" && &bytes[0..4] != b"sdPC") {
        return None;
    }
    let options = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
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

    /// CP-167 E3: a row whose control left no dump reads notrun, never ok, also when the crash
    /// with the means left nothing.
    #[test]
    fn a_control_with_no_dump_reads_notrun() {
        let places = row_places();
        let control = vec![file("crashpad", "settings.dat", 40)];
        assert_eq!(result(&places, &[], &[], &control, 0), "notrun");
        assert_eq!(result(&places, &[], &[], &[], 0), "notrun");
        let control = vec![file("crashpad", "pending/abc.dmp", 4096)];
        assert_eq!(result(&places, &[], &[], &control, 0), "ok");
        let inside = vec![file("profile", "Crashpad/reports/abc.dmp", 4096)];
        assert_eq!(result(&places, &[], &[], &inside, 0), "ok");
    }

    /// CP-167 E3: a row whose crash with the means left a file that the start with no crash did
    /// not leave reads miss, by name and by size; one that left only what the start with no crash
    /// left reads ok; anything sent after the crash reads miss.
    #[test]
    fn a_crash_with_the_means_that_left_a_file_reads_miss() {
        let places = row_places();
        let control = vec![file("crashpad", "completed/abc.dmp", 4096)];
        let baseline = vec![file("crashpad", "settings.dat", 40)];
        let same = vec![file("crashpad", "settings.dat", 40)];
        assert_eq!(result(&places, &baseline, &same, &control, 0), "ok");
        let extra = vec![
            file("crashpad", "settings.dat", 40),
            file("crashpad", "new/x.dmp", 9),
        ];
        assert_eq!(result(&places, &baseline, &extra, &control, 0), "miss");
        let resized = vec![file("crashpad", "settings.dat", 41)];
        assert_eq!(result(&places, &baseline, &resized, &control, 0), "miss");
        assert_eq!(result(&places, &baseline, &same, &control, 1), "miss");
        let reported = vec![
            file("crashpad", "settings.dat", 40),
            file("reports", "Chrome.ips", 9),
        ];
        assert_eq!(result(&places, &baseline, &reported, &control, 0), "ok");
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
        let mut data = b"CPds".to_vec();
        data.extend(1u32.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        std::fs::write(dir.join("settings.dat"), &data).unwrap();
        assert_eq!(send_setting(&dir), Some(true));
        data[8] = 0;
        std::fs::write(dir.join("settings.dat"), &data).unwrap();
        assert_eq!(send_setting(&dir), Some(false));
        std::fs::write(dir.join("settings.dat"), b"other").unwrap();
        assert_eq!(send_setting(&dir), None);
    }
}
