use std::collections::HashMap;
use std::ffi::{c_int, c_void};
use std::path::Path;

use halvo_sqlcipher::rusqlite::Connection;
use zeroize::Zeroize;

pub(crate) const PAGE: usize = 4096;
const RESERVE: usize = 80;
const IV: usize = 16;
const SALT: usize = 16;
const BLOCK: usize = 16;
const WAL_HEADER: usize = 32;
const WAL_FRAME_HEADER: usize = 24;
const JOURNAL_HEADERS: [usize; 2] = [512, 4096];
const JOURNAL_RECORD_HEAD: usize = 4;
const JOURNAL_RECORD_TAIL: usize = 4;
const KEY_SCHEDULE_WORDS: usize = 8192;

extern "C" {
    fn rijndael_setup(key: *const u8, keylen: c_int, num_rounds: c_int, skey: *mut c_void)
        -> c_int;
    fn rijndael_ecb_decrypt(ct: *const u8, pt: *mut u8, skey: *const c_void) -> c_int;
}

struct Aes256(Vec<u64>);

impl Aes256 {
    fn new(key: &[u8; 32]) -> Self {
        let mut schedule = vec![0u64; KEY_SCHEDULE_WORDS];
        let status = unsafe { rijndael_setup(key.as_ptr(), 32, 0, schedule.as_mut_ptr().cast()) };
        assert_eq!(status, 0, "libtomcrypt refused the AES-256 key schedule");
        Self(schedule)
    }

    fn decrypt_page(&self, page: &[u8], is_page_one: bool) -> Vec<u8> {
        assert_eq!(page.len(), PAGE, "a page image is not {PAGE} bytes");
        let start = if is_page_one { SALT } else { 0 };
        let body_end = PAGE - RESERVE;
        let mut previous = [0u8; BLOCK];
        previous.copy_from_slice(&page[body_end..body_end + IV]);
        let mut plain = vec![0u8; PAGE];
        for (at, block) in (start..body_end)
            .step_by(BLOCK)
            .zip(page[start..body_end].chunks(BLOCK))
        {
            let mut out = [0u8; BLOCK];
            let status = unsafe {
                rijndael_ecb_decrypt(block.as_ptr(), out.as_mut_ptr(), self.0.as_ptr().cast())
            };
            assert_eq!(status, 0, "libtomcrypt failed to decrypt a block");
            for (byte, chained) in out.iter_mut().zip(previous) {
                *byte ^= chained;
            }
            previous.copy_from_slice(block);
            plain[at..at + BLOCK].copy_from_slice(&out);
        }
        plain
    }
}

impl Drop for Aes256 {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

fn require_sqlite_header(page_one: &[u8]) {
    let fields = &page_one[16..24];
    let page_size = u16::from_be_bytes([fields[0], fields[1]]) as usize;
    assert!(
        page_size == PAGE
            && matches!(fields[2], 1 | 2)
            && matches!(fields[3], 1 | 2)
            && fields[4..] == [RESERVE as u8, 64, 32, 32],
        "page 1 does not decrypt to a SQLite header, so the page reader would find nothing for the wrong reason"
    );
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

pub(crate) struct PageImage {
    pub(crate) file: &'static str,
    pub(crate) page_number: u32,
    pub(crate) plain: Vec<u8>,
}

impl Drop for PageImage {
    fn drop(&mut self) {
        self.plain.zeroize();
    }
}

pub(crate) fn database_pages(file: &[u8], key: &[u8; 32]) -> Vec<PageImage> {
    assert!(
        !file.is_empty() && file.len().is_multiple_of(PAGE),
        "store.db is not a whole number of {PAGE}-byte pages"
    );
    let aes = Aes256::new(key);
    let pages: Vec<PageImage> = file
        .chunks(PAGE)
        .enumerate()
        .map(|(index, page)| PageImage {
            file: "store.db",
            page_number: index as u32 + 1,
            plain: aes.decrypt_page(page, index == 0),
        })
        .collect();
    require_sqlite_header(&pages[0].plain);
    pages
}

pub(crate) fn wal_frame_slots(wal: &[u8], key: &[u8; 32]) -> Vec<PageImage> {
    let aes = Aes256::new(key);
    let mut slots = Vec::new();
    let mut at = WAL_HEADER;
    while at + WAL_FRAME_HEADER + PAGE <= wal.len() {
        let page_number = be32(wal, at);
        let page = &wal[at + WAL_FRAME_HEADER..at + WAL_FRAME_HEADER + PAGE];
        slots.push(PageImage {
            file: "store.db-wal",
            page_number,
            plain: aes.decrypt_page(page, page_number == 1),
        });
        at += WAL_FRAME_HEADER + PAGE;
    }
    slots
}

pub(crate) fn journal_records(journal: &[u8], key: &[u8; 32]) -> Vec<PageImage> {
    let aes = Aes256::new(key);
    let record = JOURNAL_RECORD_HEAD + PAGE + JOURNAL_RECORD_TAIL;
    let mut records = Vec::new();
    for header in JOURNAL_HEADERS {
        let mut at = header;
        while at + record <= journal.len() {
            let page_number = be32(journal, at);
            let page = &journal[at + JOURNAL_RECORD_HEAD..at + JOURNAL_RECORD_HEAD + PAGE];
            records.push(PageImage {
                file: "store.db-journal",
                page_number,
                plain: aes.decrypt_page(page, page_number == 1),
            });
            at += record;
        }
    }
    records
}

fn read_if_present(path: &Path) -> Vec<u8> {
    match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => panic!("{} could not be read: {e}", path.display()),
    }
}

pub(crate) fn store_pages(store_dir: &Path, key: &[u8; 32]) -> Vec<PageImage> {
    let mut pages = database_pages(&read_if_present(&store_dir.join("store.db")), key);
    pages.extend(wal_frame_slots(
        &read_if_present(&store_dir.join("store.db-wal")),
        key,
    ));
    pages.extend(journal_records(
        &read_if_present(&store_dir.join("store.db-journal")),
        key,
    ));
    pages
}

pub(crate) fn page_owners(conn: &Connection) -> HashMap<u32, String> {
    let mut statement = conn.prepare("SELECT name, pageno FROM dbstat").unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get::<_, u32>(1)?, row.get::<_, String>(0)?))
        })
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

pub(crate) fn places_of(pages: &[PageImage], needles: &[&[u8]]) -> Vec<Vec<(&'static str, u32)>> {
    const PREFIX: usize = 8;
    let mut by_prefix: HashMap<&[u8], Vec<usize>> = HashMap::new();
    for (index, needle) in needles.iter().enumerate() {
        assert!(
            needle.len() >= PREFIX,
            "a needle is shorter than {PREFIX} bytes"
        );
        by_prefix.entry(&needle[..PREFIX]).or_default().push(index);
    }
    let mut places = vec![Vec::new(); needles.len()];
    for page in pages {
        for at in 0..page.plain.len().saturating_sub(PREFIX - 1) {
            let Some(candidates) = by_prefix.get(&page.plain[at..at + PREFIX]) else {
                continue;
            };
            for &index in candidates {
                let needle = needles[index];
                let found = page.plain.get(at..at + needle.len()) == Some(needle);
                let place = (page.file, page.page_number);
                if found && places[index].last() != Some(&place) {
                    places[index].push(place);
                }
            }
        }
    }
    places
}
