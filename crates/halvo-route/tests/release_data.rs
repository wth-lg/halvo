use halvo_route::cap::DefaultCap;
use halvo_route::policy::Policy;
use halvo_route::release;
use halvo_route::ruling::Ruling;

const RULING: &str = include_str!("../data/ruling.txt");
const POLICY: &str = include_str!("../data/policy.tsv");
const CAP: &str = include_str!("../data/cap.txt");

#[test]
fn release_data_parses() {
    let line = RULING
        .strip_suffix('\n')
        .expect("the ruling file ends with LF");
    let ruling = Ruling::parse(line).expect("the ruling line parses");
    assert_eq!(release::ruling(), ruling);

    let policy = Policy::parse_for(POLICY, ruling.allowed())
        .expect("the policy parses for the tiers that the ruling allows");
    assert_eq!(release::policy(), &policy);

    let line = CAP.strip_suffix('\n').expect("the cap file ends with LF");
    let cap = DefaultCap::parse(line).expect("the cap line parses");
    assert_eq!(release::cap(), Some(cap));
}
