use halvo_route::leave::{grant, Consent, LeaveCause, PartKind};
use halvo_route::release;
use halvo_route::tier::Tier;

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
