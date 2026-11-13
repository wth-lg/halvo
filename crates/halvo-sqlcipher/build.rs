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

const MEND_LINE: &str =
    "  /* The pPg->nFree field is now set incorrectly. The caller will fix it. */";

const MEND: &str = " if( pPg->pBt->btsFlags & BTS_FAST_SECURE ){ memset(pCellptr, 0, (size_t)(pData - pCellptr)); }";

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

fn mended_source(source: &[u8]) -> Vec<u8> {
    let (at, line) = lines(source)
        .find(|(_, line)| *line == MEND_LINE.as_bytes())
        .expect("the source holds the line of the mend");
    let end = at + line.len();
    let mut copy = Vec::with_capacity(source.len() + MEND.len());
    copy.extend_from_slice(&source[..end]);
    copy.extend_from_slice(MEND.as_bytes());
    copy.extend_from_slice(&source[end..]);
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
    std::fs::write(&copy, mended_source(&source))
        .unwrap_or_else(|error| panic!("halvo-sqlcipher: write {}: {error}", copy.display()));

    let mut sqlcipher = libtomcrypt_build(source_dir);
    sqlcipher.include(source_dir).file(&copy).file(ENTROPY_SHIM);
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
