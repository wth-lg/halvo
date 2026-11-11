use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub mod footprint;
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
    pub hits: [u64; 3],
}

impl Search {
    pub fn total(&self) -> u64 {
        self.hits.iter().sum()
    }

    fn add(&mut self, other: &Search) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.unreadable += other.unreadable;
        for (a, b) in self.hits.iter_mut().zip(other.hits) {
            *a += b;
        }
    }
}

pub fn search(roots: &[PathBuf], text: &str) -> Search {
    let needles: Vec<Vec<u8>> = ENCODINGS.iter().filter_map(|e| encode(text, e)).collect();
    let mut total = Search::default();
    for root in roots {
        total.add(&search_one(root, &needles));
    }
    total
}

fn search_one(root: &Path, needles: &[Vec<u8>]) -> Search {
    let mut s = Search::default();
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

pub fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
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
        assert_eq!(s.hits, [1, 1, 1]);
        assert_eq!(s.total(), 3);
        assert_eq!(search(&[root.join("missing")], word), Search::default());
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
    fn landing_reads_the_page() {
        assert_eq!(landing("/bill"), ("ok", "no"));
        assert_eq!(landing("/verify"), ("ok", "yes"));
        assert_eq!(landing("/signin"), ("miss", "no"));
        assert_eq!(landing("/"), ("miss", "no"));
    }
}
