use std::path::{Path, PathBuf};

const LIBTOMCRYPT_SWITCHES: &[&str] = &[
    "LTC_NOTHING",
    "LTC_RIJNDAEL",
    "LTC_CBC_MODE",
    "LTC_SHA1",
    "LTC_SHA256",
    "LTC_SHA512",
    "LTC_HASH_HELPERS",
    "LTC_HMAC",
    "LTC_FORTUNA",
    "LTC_PKCS_5",
    "LTC_RNG_GET_BYTES",
    "LTC_PRNG_ENABLE_LTC_RNG",
    "LTC_NO_TEST",
    "LTC_NO_ASM",
    "LTC_NO_MATH",
    "LTC_CLEAN_STACK",
];

const SQLITE_SWITCHES: &[(&str, Option<&str>)] = &[
    ("SQLITE_CORE", None),
    ("SQLITE_HAS_CODEC", None),
    ("SQLCIPHER_CRYPTO_LIBTOMCRYPT", None),
    ("SQLITE_EXTRA_INIT", Some("halvo_sqlcipher_extra_init")),
    ("SQLITE_EXTRA_SHUTDOWN", Some("sqlcipher_extra_shutdown")),
    ("SQLITE_TEMP_STORE", Some("2")),
    ("SQLITE_THREADSAFE", Some("1")),
    ("SQLITE_DEFAULT_FOREIGN_KEYS", Some("1")),
    ("SQLITE_ENABLE_API_ARMOR", None),
    ("SQLITE_ENABLE_COLUMN_METADATA", None),
    ("SQLITE_ENABLE_DBSTAT_VTAB", None),
    ("SQLITE_ENABLE_FTS3", None),
    ("SQLITE_ENABLE_FTS3_PARENTHESIS", None),
    ("SQLITE_ENABLE_FTS5", None),
    ("SQLITE_ENABLE_JSON1", None),
    ("SQLITE_ENABLE_RTREE", None),
    ("SQLITE_ENABLE_STAT4", None),
    ("SQLITE_SOUNDEX", None),
    ("SQLITE_USE_URI", None),
    ("HAVE_STDINT_H", Some("1")),
    ("HAVE_USLEEP", Some("1")),
    ("HAVE_ISNAN", None),
];

const ENTROPY_SHIM: &str = "csrc/entropy.c";

const PINNED: &str = "sqlcipher-src 419.0.6 (SQLCipher 4.19.0 on SQLite 3.53.4)";

const PINNED_SHA256: &str = "838b2ed8a586b8b77f03e82c5a25b97e1a55e524ebac0de05354193865703072";

const FIX: &str = "read rebuildPage, editPage, pageInsertArray, pageFreeArray, balance_quick and \
    balance_nonroot from its divider loop again in the new source, with every row of the table of \
    docs/adr/0013, move or delete the mends, and run the tests page_gap of halvo-sqlcipher, the \
    structural check of every page among them, on the source with and without the mends";

const SHA256_ROUND: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

struct Mend {
    function: &'static str,
    line: &'static str,
    statement: &'static str,
}

const MENDS: [Mend; 2] = [
    Mend {
        function: "static int rebuildPage(",
        line: "  /* The pPg->nFree field is now set incorrectly. The caller will fix it. */",
        statement: " if( pPg->pBt->btsFlags & BTS_FAST_SECURE ){ memset(pCellptr, 0, (size_t)(pData - pCellptr)); }",
    },
    Mend {
        function: "static int editPage(",
        line: "  pPg->nCell = (u16)nNew;",
        statement: " if( (pPg->pBt->btsFlags & BTS_FAST_SECURE) && pData>pBegin ){ memset(pBegin, 0, (size_t)(pData - pBegin)); }",
    },
];

const MARK: &str = "int halvo_sqlcipher_page_gap_mend(void){ return 2; }\n";

fn libtomcrypt_build(source_dir: &Path) -> cc::Build {
    let mut build = cc::Build::new();
    build
        .include(source_dir.join(sqlcipher_src::LIBTOMCRYPT_INCLUDE_DIR))
        .define("ARGTYPE", "4")
        .warnings(false);
    for switch in LIBTOMCRYPT_SWITCHES {
        build.define(switch, None);
    }
    build
}

fn lines(text: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    let mut start = 0;
    text.split(|byte| *byte == b'\n').map(move |line| {
        let at = start;
        start += line.len() + 1;
        (at, line)
    })
}

fn sha256_hex(data: &[u8]) -> String {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut words = [0u32; 64];
        for (word, bytes) in words.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        for at in 16..64 {
            let (back15, back2) = (words[at - 15], words[at - 2]);
            let s0 = back15.rotate_right(7) ^ back15.rotate_right(18) ^ (back15 >> 3);
            let s1 = back2.rotate_right(17) ^ back2.rotate_right(19) ^ (back2 >> 10);
            words[at] = words[at - 16]
                .wrapping_add(s0)
                .wrapping_add(words[at - 7])
                .wrapping_add(s1);
        }
        let mut v = state;
        for (round, word) in SHA256_ROUND.iter().zip(words) {
            let [a, b, c, d, e, f, g, h] = v;
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(*round)
                .wrapping_add(word);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            v = [
                t1.wrapping_add(s0.wrapping_add(majority)),
                a,
                b,
                c,
                d.wrapping_add(t1),
                e,
                f,
                g,
            ];
        }
        for (word, add) in state.iter_mut().zip(v) {
            *word = word.wrapping_add(add);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

fn stop(found: String) -> ! {
    panic!(
        "halvo-sqlcipher: the mends of rebuildPage and editPage were read in {PINNED}, sqlcipher.c with sha256 \
         {PINNED_SHA256}; {found}. Fix: {FIX}."
    )
}

fn occurrences(line: &[u8], text: &str) -> usize {
    line.windows(text.len())
        .filter(|window| *window == text.as_bytes())
        .count()
}

fn place(source: &[u8], mend: &Mend) -> (usize, &'static str) {
    let Mend {
        function,
        line,
        statement,
    } = mend;
    let found: Vec<(usize, (usize, &[u8]))> = lines(source)
        .enumerate()
        .filter(|(_, (_, text))| *text == line.as_bytes())
        .collect();
    let [(line_number, (at, text))] = found[..] else {
        stop(format!(
            "looked for the line `{line}` once, found it {} times",
            found.len()
        ))
    };
    let opening: Vec<usize> = lines(source)
        .enumerate()
        .flat_map(|(number, (_, text))| std::iter::repeat_n(number, occurrences(text, function)))
        .collect();
    let [function_number] = opening[..] else {
        stop(format!(
            "looked for the text `{function}` once, found it {} times",
            opening.len()
        ))
    };
    if function_number >= line_number {
        stop(format!(
            "looked for the text `{function}` before the line `{line}`, line {}, found it on line {}",
            line_number + 1,
            function_number + 1
        ));
    }
    if let Some((end, _)) = lines(source)
        .enumerate()
        .take(line_number)
        .skip(function_number + 1)
        .find(|(_, (_, text))| text.first() == Some(&b'}'))
    {
        stop(format!(
            "looked for no function end between the text `{function}` on line {} and the line \
             `{line}`, line {}, found one on line {}",
            function_number + 1,
            line_number + 1,
            end + 1
        ));
    }
    (at + text.len(), statement)
}

fn mended_source(source: &[u8], name: &Path) -> Vec<u8> {
    let hash = sha256_hex(source);
    if hash != PINNED_SHA256 {
        stop(format!(
            "looked for that sha256 in {}, found {hash}",
            name.display()
        ));
    }
    let mut ends = Vec::new();
    for mend in &MENDS {
        ends.push(place(source, mend));
    }
    ends.sort_unstable_by_key(|(end, _)| *end);
    let mut copy = Vec::with_capacity(source.len() + 256);
    let mut from = 0;
    for (end, statement) in ends {
        copy.extend_from_slice(&source[from..end]);
        copy.extend_from_slice(statement.as_bytes());
        from = end;
    }
    copy.extend_from_slice(&source[from..]);
    copy.extend_from_slice(MARK.as_bytes());
    copy
}

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("cargo sets CARGO_CFG_TARGET_OS");
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let source_dir = sqlcipher_src::source_dir();
    let pinned = source_dir.join(sqlcipher_src::SOURCE_FILE);
    let source = std::fs::read(&pinned)
        .unwrap_or_else(|error| panic!("halvo-sqlcipher: read {}: {error}", pinned.display()));
    let copy = out_dir.join(sqlcipher_src::SOURCE_FILE);
    std::fs::write(&copy, mended_source(&source, &pinned))
        .unwrap_or_else(|error| panic!("halvo-sqlcipher: write {}: {error}", copy.display()));

    let mut sqlcipher = libtomcrypt_build(source_dir);
    sqlcipher.file(&copy).file(ENTROPY_SHIM);
    for (name, value) in SQLITE_SWITCHES {
        sqlcipher.define(name, *value);
    }
    if target_os != "windows" {
        sqlcipher.define("HAVE_LOCALTIME_R", None);
    }
    sqlcipher.compile("sqlcipher");

    libtomcrypt_build(source_dir)
        .define("LTC_SOURCE", None)
        .files(
            sqlcipher_src::LIBTOMCRYPT_SOURCES
                .iter()
                .map(|relative| source_dir.join(relative)),
        )
        .compile("sqlcipher_libtomcrypt");

    if target_os == "windows" {
        println!("cargo:rustc-link-lib=bcrypt");
        println!("cargo:rustc-link-lib=advapi32");
    }
    println!(
        "cargo:rustc-env=HALVO_SQLCIPHER_PINNED_SOURCE={}",
        pinned.display()
    );
    println!("cargo:rerun-if-changed={}", pinned.display());
    println!("cargo:rerun-if-changed={ENTROPY_SHIM}");
    println!("cargo:rerun-if-changed=build.rs");
}
