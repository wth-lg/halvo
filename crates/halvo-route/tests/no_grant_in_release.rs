use halvo_route::leave::{grant, Consent, LeaveCause, PartKind};
use halvo_route::release;
use halvo_route::tier::Tier;
use sha2::{Digest, Sha256};

const PINNED: [(&str, &str); 5] = [
    (
        "src/leave.rs",
        "ff956fb7ccdb24692ae6c855ad27e082ba051d13d911e598a238991b5d59c0e9",
    ),
    (
        "src/lib.rs",
        "82333428818c311344054ce975e49ee207a52c3dc4fef562e38f586892130036",
    ),
    (
        "src/ruling.rs",
        "507e4e9563c0b2c94695ba74484f6a42f667f99e99e38dc2f12aba79ad3ebb21",
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

const CRATE_MODULES: [&str; 11] = [
    "admit", "cap", "leave", "line", "pick", "policy", "prices", "release", "ruling", "spend",
    "tier",
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
