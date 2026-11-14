use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub mod footprint;
pub mod product;
pub mod screen;

pub const MIB: u64 = 1_048_576;
pub const ENCODINGS: [&str; 3] = ["utf8", "utf16le", "utf16be"];
pub const LEAVE_OUT_ALL: &str = "all";

pub fn bound_bytes(median: u64) -> u64 {
    let units = (5 * median as u128).div_ceil(33_554_432);
    (units * 8 * MIB as u128) as u64
}

pub fn encode(text: &str, encoding: &str) -> Option<Vec<u8>> {
    match encoding {
        "utf8" => Some(text.as_bytes().to_vec()),
        "utf16le" => Some(text.encode_utf16().flat_map(u16::to_le_bytes).collect()),
        "utf16be" => Some(text.encode_utf16().flat_map(u16::to_be_bytes).collect()),
        _ => None,
    }
}

pub fn occurrences(haystack: &[u8], needle: &[u8]) -> u64 {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut n = 0;
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            n += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    n
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Search {
    pub files: u64,
    pub bytes: u64,
    pub unreadable: u64,
    pub skipped: u64,
    pub hits: Vec<u64>,
}

impl Search {
    pub fn total(&self) -> u64 {
        self.hits.iter().sum()
    }

    pub fn add(&mut self, other: &Search) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.unreadable += other.unreadable;
        self.skipped += other.skipped;
        if self.hits.len() < other.hits.len() {
            self.hits.resize(other.hits.len(), 0);
        }
        for (a, b) in self.hits.iter_mut().zip(&other.hits) {
            *a += b;
        }
    }
}

pub fn marker_needles(text: &str) -> Vec<Vec<u8>> {
    ENCODINGS.iter().filter_map(|e| encode(text, e)).collect()
}

pub fn search(roots: &[PathBuf], text: &str) -> Search {
    search_with(roots, &marker_needles(text), None)
}

pub fn search_with(
    roots: &[PathBuf],
    needles: &[Vec<u8>],
    since: Option<std::time::SystemTime>,
) -> Search {
    let mut total = Search {
        hits: vec![0; needles.len()],
        ..Search::default()
    };
    for root in roots {
        total.add(&search_one(root, needles, since));
    }
    total
}

pub fn outside_roots(home: &Path, temp: &Path) -> Vec<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let support = home.join("Library").join("Application Support");
    let user_dir = temp
        .components()
        .collect::<PathBuf>()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let cache = user_dir.join("C");
    vec![
        support.join("Google").join("Chrome").join("Crashpad"),
        support.join("Microsoft Edge").join("Crashpad"),
        cache.join("com.google.Chrome.helper"),
        cache.join("com.microsoft.edgemac.helper"),
    ]
}

fn search_one(root: &Path, needles: &[Vec<u8>], since: Option<std::time::SystemTime>) -> Search {
    let mut s = Search {
        hits: vec![0; needles.len()],
        ..Search::default()
    };
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                s.unreadable += 1;
                continue;
            }
        };
        if meta.is_dir() {
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    for e in entries {
                        match e {
                            Ok(e) => stack.push(e.path()),
                            Err(_) => s.unreadable += 1,
                        }
                    }
                }
                Err(_) => s.unreadable += 1,
            }
        } else if meta.is_file() {
            if let (Some(t), Ok(m)) = (since, meta.modified()) {
                if m < t {
                    continue;
                }
            }
            match std::fs::read(&path) {
                Ok(data) => {
                    s.files += 1;
                    s.bytes += data.len() as u64;
                    for (i, n) in needles.iter().enumerate() {
                        s.hits[i] += occurrences(&data, n);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => s.unreadable += 1,
            }
        } else {
            s.skipped += 1;
        }
    }
    s
}

pub fn entry_names(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

pub fn new_entries(
    dir: &Path,
    before: &BTreeSet<String>,
    after: &BTreeSet<String>,
    skip: &Path,
) -> Vec<PathBuf> {
    after
        .difference(before)
        .map(|name| dir.join(name))
        .filter(|p| !same_path(p, skip))
        .collect()
}

/// Whether neither `profile` nor `<profile>-tmp` exists, read without following a link; any other
/// error of the read than not-found counts as existing.
pub fn profile_gone(profile: &Path) -> bool {
    let mut temp = profile.as_os_str().to_owned();
    temp.push("-tmp");
    [profile, Path::new(&temp)].iter().all(|p| {
        matches!(std::fs::symlink_metadata(p), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    })
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Whether `line` is a `run`, `notrun` or `task` line of a run that ends `cause=browser_start`.
pub fn browser_did_not_start(line: &str) -> bool {
    let of_a_run = line.starts_with("run candidate=")
        || line.starts_with("notrun candidate=")
        || line.starts_with("task ");
    of_a_run && line.ends_with(" cause=browser_start")
}

pub fn landing(path: &str) -> (&'static str, &'static str) {
    match path {
        "/bill" => ("ok", "no"),
        "/verify" => ("ok", "yes"),
        _ => ("miss", "no"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_follows_the_rule_of_the_workflow_run() {
        assert_eq!(bound_bytes(214_748_364), 256 * MIB);
        assert_eq!(bound_bytes(214_748_365), 264 * MIB);
        assert_eq!(bound_bytes(0), 0);
        assert_eq!(bound_bytes(1), 8 * MIB);
        assert_eq!(bound_bytes(800 * MIB), 1000 * MIB);
    }

    #[test]
    fn encodings_give_each_byte_order() {
        assert_eq!(encode("Ab", "utf8").unwrap(), b"Ab".to_vec());
        assert_eq!(encode("Ab", "utf16le").unwrap(), vec![0x41, 0, 0x62, 0]);
        assert_eq!(encode("Ab", "utf16be").unwrap(), vec![0, 0x41, 0, 0x62]);
        assert_eq!(encode("Ab", "latin1"), None);
    }

    #[test]
    fn occurrences_count_without_overlap() {
        assert_eq!(occurrences(b"xabcabcx", b"abc"), 2);
        assert_eq!(occurrences(b"aaaa", b"aa"), 2);
        assert_eq!(occurrences(b"ab", b"abc"), 0);
        assert_eq!(occurrences(b"abc", b""), 0);
    }

    #[test]
    fn profile_gone_reads_both_paths_and_follows_no_link() {
        let root = std::env::temp_dir().join(format!("r3-profile-gone-{}", std::process::id()));
        std::fs::create_dir_all(root.join("both")).unwrap();
        std::fs::create_dir_all(root.join("both-tmp")).unwrap();
        std::fs::create_dir_all(root.join("tmp-only-tmp")).unwrap();
        assert!(profile_gone(&root.join("none")));
        assert!(!profile_gone(&root.join("both")));
        assert!(!profile_gone(&root.join("tmp-only")));
        #[cfg(unix)]
        {
            std::fs::write(root.join("afile"), b"x").unwrap();
            assert!(!profile_gone(&root.join("afile").join("under-a-file")));
            let link = root.join("link");
            if std::fs::symlink_metadata(&link).is_err() {
                std::os::unix::fs::symlink(root.join("no-target"), &link).unwrap();
            }
            assert!(!profile_gone(&link));
        }
    }

    #[test]
    fn search_reads_nested_files_in_each_encoding() {
        let root = std::env::temp_dir().join(format!("r3-profile-search-{}", std::process::id()));
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        let word = "needle-for-the-search-test";
        std::fs::write(root.join("one"), encode(word, "utf8").unwrap()).unwrap();
        let mut two = b"head".to_vec();
        two.extend(encode(word, "utf16le").unwrap());
        two.extend(encode(word, "utf16be").unwrap());
        std::fs::write(nested.join("two"), two).unwrap();
        std::fs::write(nested.join("three"), b"nothing here").unwrap();
        let s = search(std::slice::from_ref(&root), word);
        assert_eq!(s.files, 3);
        assert_eq!(s.hits, vec![1, 1, 1]);
        assert_eq!(s.total(), 3);
        assert_eq!(search(&[root.join("missing")], word).total(), 0);
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("one"), root.join("link")).unwrap();
        #[cfg(unix)]
        assert_eq!(search(std::slice::from_ref(&root), word).skipped, 1);
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        assert_eq!(
            search_with(
                std::slice::from_ref(&root),
                &marker_needles(word),
                Some(later)
            )
            .files,
            0
        );
    }

    #[test]
    fn new_entries_leave_out_the_old_ones_and_the_skipped_one() {
        let dir = Path::new("/t");
        let before: BTreeSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let after: BTreeSet<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            new_entries(dir, &before, &after, Path::new("/t/d")),
            vec![PathBuf::from("/t/c")]
        );
    }

    #[test]
    fn a_browser_that_did_not_start_is_read_from_the_lines_of_a_run() {
        assert!(browser_did_not_start(
            "run candidate=cdp machine=mac browser=chrome step=signin_form i=1 result=miss ms=0 version=- cause=browser_start"
        ));
        assert!(browser_did_not_start(
            "notrun candidate=cdp machine=mac browser=chrome step=signin_form cause=browser_start"
        ));
        assert!(browser_did_not_start(
            "task machine=mac browser=chrome result=miss ms=40 removed=no cause=browser_start"
        ));
        assert!(!browser_did_not_start(
            "run candidate=cdp machine=mac browser=chrome step=signin_form i=1 result=miss ms=9 version=x cause=timeout"
        ));
        assert!(!browser_did_not_start(
            "notrun item=4 machine=mac browser=chrome cause=browser_start"
        ));
    }

    #[test]
    fn landing_reads_the_page() {
        assert_eq!(landing("/bill"), ("ok", "no"));
        assert_eq!(landing("/verify"), ("ok", "yes"));
        assert_eq!(landing("/signin"), ("miss", "no"));
        assert_eq!(landing("/"), ("miss", "no"));
    }
}
