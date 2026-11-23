use halvo_route::leave::{grant, Consent, LeaveCause, PartKind};
use halvo_route::release;
use halvo_route::tier::Tier;
use sha2::{Digest, Sha256};

const PINNED: [(&str, &str); 5] = [
    (
        "src/leave.rs",
        "991379b4cb1f90816d00fcd5b4ea4c754354d41cf360f9af2f14984ea9c61c3f",
    ),
    (
        "src/lib.rs",
        "caf3c97c58d3719747740f6f7196495310a24441b06a876fa0a5f9ba819b3031",
    ),
    (
        "src/ruling.rs",
        "58f146ea9865fe9c070fb15eded36eb4f22bb60a9fafe54025b888bfd462b80d",
    ),
    (
        "src/tier.rs",
        "64850efc01ea0c73b1fcca18542592d47ce52be979dc421bba59aaf1eba17365",
    ),
    (
        "Cargo.toml",
        "42ed4aa130d163c0ece33201d1056146aecd2f15032e94a43fd4ec35a8f0da8c",
    ),
];

#[test]
fn pinned_files_hold_their_hashes() {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut changed = Vec::new();
    for (name, pinned) in PINNED {
        let bytes = std::fs::read(crate_dir.join(name)).expect("a pinned file of the crate");
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if hash != pinned {
            changed.push(format!("{name} now hashes to {hash}"));
        }
    }
    assert!(
        changed.is_empty(),
        "a pinned file changed. A change of a pinned file updates its hash in PINNED of tests/no_grant_in_release.rs in the same commit, where the reviewer of that commit reads the change.\n{}",
        changed.join("\n")
    );
}

#[test]
fn the_manifest_holds_the_package_and_the_dev_dependencies_alone() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("the crate's manifest");
    let tables: Vec<&str> = manifest
        .lines()
        .filter(|line| line.trim_start().starts_with('['))
        .collect();
    assert_eq!(
        (manifest.lines().next(), tables),
        (Some("[package]"), vec!["[package]", "[dev-dependencies]"]),
        "halvo-route has no dependency outside the workspace (contracts.md T0.7): its manifest opens with [package] and holds no table but [package] and [dev-dependencies]"
    );
}

#[test]
fn no_grant_in_release() {
    let ruling = release::ruling();
    let mut lists: Vec<Vec<PartKind>> = PartKind::ALL.iter().map(|part| vec![*part]).collect();
    lists.push(Vec::new());
    lists.push(PartKind::ALL.to_vec());
    let mut calls = 0;
    for tier in Tier::ALL {
        let cause = if tier == Tier::OnDevice {
            LeaveCause::Local
        } else {
            LeaveCause::Ruling
        };
        for ceiling in Tier::ALL {
            for parts in &lists {
                for consent in Consent::ALL {
                    for origin in ["127.0.0.1:1", "example.invalid:443"] {
                        let answer = grant(&ruling, tier, ceiling, parts, origin, consent);
                        assert_eq!(
                            answer.err(),
                            Some(cause),
                            "{tier:?} {ceiling:?} {parts:?} {consent:?} {origin}"
                        );
                        calls += 1;
                    }
                }
            }
        }
    }
    assert_eq!(calls, 3 * 3 * 7 * 2 * 2);
}

const CRATE_MODULES: [&str; 12] = [
    "admit", "cap", "count", "leave", "line", "pick", "policy", "prices", "release", "ruling",
    "spend", "tier",
];

#[test]
fn lib_rs_is_the_forbid_line_and_the_module_lines() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("the crate's lib.rs");
    let modules: String = CRATE_MODULES
        .iter()
        .map(|module| format!("pub mod {module};\n"))
        .collect();
    assert_eq!(
        lib,
        format!("#![forbid(unsafe_code)]\n\n{modules}"),
        "lib.rs holds the forbid line and the lines of CRATE_MODULES alone"
    );
}

#[test]
fn a_loopback_ruling_names_no_origin_off_the_machine() {
    let hosted = [Tier::LowCostHosted, Tier::Frontier];
    for port in [0, 1, 80, 443, 4040, 65535] {
        let origin = format!("http://127.0.0.1:{port}");
        let ruling =
            halvo_route::ruling::Ruling::loopback_for_tests(port, "model-1.5:x", "adapter_a");
        let rows: Vec<(Tier, &str, &str, &str)> = ruling
            .rows()
            .iter()
            .map(|row| (row.tier(), row.origin(), row.model(), row.adapter()))
            .collect();
        assert_eq!(
            rows,
            hosted.map(|tier| (tier, origin.as_str(), "model-1.5:x", "adapter_a")),
            "{port}"
        );
        assert_eq!(ruling.allowed(), &Tier::ALL);
        for tier in hosted {
            let made = grant(
                &ruling,
                tier,
                Tier::Frontier,
                &[PartKind::Text],
                &origin,
                Consent::Recorded,
            )
            .expect("a grant for the loopback origin");
            assert_eq!(made.row().origin(), origin);
            for asked in [
                format!("http://localhost:{port}"),
                format!("https://127.0.0.1:{port}"),
                format!("http://127.0.0.1:{}", u32::from(port) + 1),
                format!("http://0.0.0.0:{port}"),
                "http://127.0.0.1".to_owned(),
                "https://example.invalid".to_owned(),
            ] {
                let answer = grant(
                    &ruling,
                    tier,
                    Tier::Frontier,
                    &[PartKind::Text],
                    &asked,
                    Consent::Recorded,
                );
                assert_eq!(answer.err(), Some(LeaveCause::Origin), "{asked}");
            }
        }
    }
    let long = "m".repeat(65);
    for (model, adapter) in [
        ("", "a"),
        ("m", ""),
        ("m m", "a"),
        ("m", "a\tb"),
        ("m/x", "a"),
        ("m", "a\n"),
        (long.as_str(), "a"),
        ("m", "\u{e9}"),
    ] {
        assert_eq!(
            halvo_route::ruling::Ruling::loopback_for_tests(4040, model, adapter),
            halvo_route::ruling::Ruling::None,
            "{model:?} {adapter:?}"
        );
    }
}

/// The name of the public maker of a loopback ruling, written in parts so that this text is no
/// use of it.
const LOOPBACK_MAKER: &str = concat!("loopback", "_for", "_tests");

/// The Rust files that may name the maker of a loopback ruling, each with the number of times that
/// the name stands in it as a word, in code, comments and strings alike, and why. The maker's own
/// file and test files alone: no file that a build for a person compiles as product code but the
/// maker's own.
const LOOPBACK_MAKER_USES: [(&str, usize, &str); 3] = [
    (
        "crates/halvo-model/src/loopback.rs",
        1,
        "the door's stand-in form, compiled under its cargo feature loopback alone: the coordinator's log line 16582",
    ),
    (
        "crates/halvo-route/src/ruling.rs",
        2,
        "the maker's own file: its definition and the comment of Ruling that names the two makers",
    ),
    (
        "crates/halvo-route/tests/no_grant_in_release.rs",
        2,
        "the test a_loopback_ruling_names_no_origin_off_the_machine",
    ),
];

/// Every file under the directory `dir` of the tree at `base` whose name ends in `.rs` in any
/// letter case, by its path from `base`, in order: the walk of `rust_files` of
/// crates/halvo-capture/tests/source_rules.rs, copied. It enters no directory whose name begins
/// with a dot, none named `node_modules` or `target`, and none that holds a file `CACHEDIR.TAG`. A
/// directory that cannot be listed, and an entry that cannot be read, is a fault that names it.
fn rust_files(base: &std::path::Path, dir: &str, out: &mut Vec<String>, faults: &mut Vec<String>) {
    let entries = match std::fs::read_dir(base.join(dir)) {
        Ok(entries) => entries,
        Err(e) => {
            faults.push(format!("the directory '{dir}' cannot be listed: {e}"));
            return;
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        let read = entry.and_then(|entry| {
            let kind = entry.file_type()?;
            Ok((entry.file_name().to_string_lossy().into_owned(), kind))
        });
        match read {
            Ok(one) => found.push(one),
            Err(e) => faults.push(format!("an entry of '{dir}' cannot be read: {e}")),
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, kind) in found {
        let path = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        if kind.is_dir() {
            let skipped = name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || base.join(&path).join("CACHEDIR.TAG").exists();
            if !skipped {
                rust_files(base, &path, out, faults);
            }
        } else if name.to_ascii_lowercase().ends_with(".rs") {
            out.push(path);
        }
    }
}

/// How many times the maker's name stands in `text` as a word: a run of ASCII letters, digits and
/// underscores.
fn loopback_maker_words(text: &str) -> usize {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| *word == LOOPBACK_MAKER)
        .count()
}

/// The Rust files under `base` that name the maker, each with its count, and the faults of the
/// walk and of the reading.
fn loopback_maker_reading(base: &std::path::Path) -> (Vec<(String, usize)>, Vec<String>) {
    let (mut files, mut faults) = (Vec::new(), Vec::new());
    rust_files(base, "", &mut files, &mut faults);
    let mut counts = Vec::new();
    for file in files {
        match std::fs::read_to_string(base.join(&file)) {
            Ok(text) => match loopback_maker_words(&text) {
                0 => {}
                n => counts.push((file, n)),
            },
            Err(e) => faults.push(format!("{file} cannot be read as UTF-8 text: {e}")),
        }
    }
    (counts, faults)
}

/// One refusal for each file whose count differs from its entry of `listed`, a file that is not
/// listed counting as allowed none, and a listed file that is gone as found none.
fn loopback_maker_refusals(
    base: &std::path::Path,
    counts: &[(String, usize)],
    listed: &[(&str, usize, &str)],
) -> Vec<String> {
    let mut files: Vec<&str> = counts
        .iter()
        .map(|(file, _)| file.as_str())
        .chain(listed.iter().map(|(file, _, _)| *file))
        .collect();
    files.sort();
    files.dedup();
    files
        .into_iter()
        .filter_map(|file| {
            let found = counts
                .iter()
                .find(|(f, _)| f == file)
                .map_or(0, |(_, n)| *n);
            let (allowed, why) = listed
                .iter()
                .find(|(f, _, _)| *f == file)
                .map_or((0, "the file is not on the list"), |(_, n, why)| (*n, *why));
            (found != allowed).then(|| {
                let lines = std::fs::read_to_string(base.join(file)).map_or_else(
                    |e| format!("none, as the file cannot be read: {e}"),
                    |text| {
                        let at: Vec<String> = text
                            .lines()
                            .enumerate()
                            .filter(|(_, line)| loopback_maker_words(line) > 0)
                            .map(|(at, _)| (at + 1).to_string())
                            .collect();
                        if at.is_empty() {
                            "none".to_owned()
                        } else {
                            at.join(", ")
                        }
                    },
                );
                format!(
                    "{file}: uses of the name of the maker of a loopback ruling ({LOOPBACK_MAKER}) as a word: {found}, where LOOPBACK_MAKER_USES allows {allowed} ({why}); its lines that name it: {lines}; product code takes its ruling from halvo_route::release::ruling(), a comment of a product file says it in other words, and a new test file goes on LOOPBACK_MAKER_USES with its count and why"
                )
            })
        })
        .collect()
}

#[test]
fn the_loopback_maker_stands_only_in_its_own_file_and_listed_test_files() {
    for (text, want) in [
        (
            format!("let r = Ruling::{LOOPBACK_MAKER}(1, \"m\", \"a\");"),
            1,
        ),
        (
            format!("use halvo_route::ruling::Ruling as R;\nR::r#{LOOPBACK_MAKER}(1, m, a)"),
            1,
        ),
        (
            format!("/// Ruling::{LOOPBACK_MAKER}\n// {LOOPBACK_MAKER}"),
            2,
        ),
        (format!("const S: &str = \"{LOOPBACK_MAKER}\";"), 1),
        (
            format!("fn {LOOPBACK_MAKER}s() {{}}\nfn my_{LOOPBACK_MAKER}() {{}}"),
            0,
        ),
        (concat!("loopback", "_for", "_test").to_owned(), 0),
    ] {
        assert_eq!(loopback_maker_words(&text), want, "{text}");
    }

    let tree = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "loopback-maker-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let plant = |path: &str, bytes: &[u8]| {
        let file = tree.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a planted directory");
        std::fs::write(file, bytes).expect("a planted file");
    };
    let call = format!("fn f() {{ let r = Ruling::{LOOPBACK_MAKER}(1, \"m\", \"a\"); }}\n");
    let product = "crates/a/src/lib.rs";
    let listed = "crates/a/tests/stand_in.rs";
    let upper = "tools/x/src/Upper.RS";
    plant(
        product,
        format!("// Ruling::{LOOPBACK_MAKER} is for tests\n{call}").as_bytes(),
    );
    plant(listed, call.as_bytes());
    plant(upper, call.as_bytes());
    plant(
        "crates/a/src/near.rs",
        format!("fn {LOOPBACK_MAKER}s() {{}}\n").as_bytes(),
    );
    plant(
        "out/CACHEDIR.TAG",
        b"Signature: 8a477f597d28d172789f06886806bc55",
    );
    for skipped in [
        "out/debug/made.rs",
        "target/debug/held.rs",
        ".claude/worktrees/w/src/lib.rs",
        "ui/node_modules/pkg/lib.rs",
        "docs/notes.md",
    ] {
        plant(skipped, call.as_bytes());
    }
    plant("crates/a/src/bytes.rs", b"fn f() {}\n\xff\n");
    let (counts, faults) = loopback_maker_reading(&tree);
    assert_eq!(faults.len(), 1, "{faults:?}");
    assert!(
        faults[0].starts_with("crates/a/src/bytes.rs cannot be read as UTF-8 text: "),
        "{faults:?}"
    );
    assert_eq!(
        counts,
        [
            (product.to_owned(), 2),
            (listed.to_owned(), 1),
            (upper.to_owned(), 1)
        ]
    );
    let refusals = loopback_maker_refusals(
        &tree,
        &counts,
        &[
            (listed, 1, "a test file of the planted tree"),
            (upper, 2, "a count above the tree's"),
            ("crates/gone.rs", 1, "a file that is gone"),
        ],
    );
    let refused: Vec<&str> = refusals
        .iter()
        .map(|refusal| refusal.split(':').next().expect("a file"))
        .collect();
    assert_eq!(refused, ["crates/a/src/lib.rs", "crates/gone.rs", upper]);
    assert_eq!(
        refusals[0],
        format!("{product}: uses of the name of the maker of a loopback ruling ({LOOPBACK_MAKER}) as a word: 2, where LOOPBACK_MAKER_USES allows 0 (the file is not on the list); its lines that name it: 1, 2; product code takes its ruling from halvo_route::release::ruling(), a comment of a product file says it in other words, and a new test file goes on LOOPBACK_MAKER_USES with its count and why")
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let (counts, faults) = loopback_maker_reading(&root);
    println!(
        "files that name the maker of a loopback ruling: {}, listed files: {}",
        counts.len(),
        LOOPBACK_MAKER_USES.len()
    );
    assert_eq!(faults, Vec::<String>::new());
    assert_eq!(
        loopback_maker_refusals(&root, &counts, &LOOPBACK_MAKER_USES),
        Vec::<String>::new()
    );
}
