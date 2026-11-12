use std::path::Path;

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

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("cargo sets CARGO_CFG_TARGET_OS");
    let source_dir = sqlcipher_src::source_dir();

    let mut sqlcipher = libtomcrypt_build(source_dir);
    sqlcipher
        .file(source_dir.join(sqlcipher_src::SOURCE_FILE))
        .file(ENTROPY_SHIM);
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
    println!("cargo:rerun-if-changed={ENTROPY_SHIM}");
    println!("cargo:rerun-if-changed=build.rs");
}
