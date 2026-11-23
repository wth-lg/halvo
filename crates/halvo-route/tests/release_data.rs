use halvo_route::cap::DefaultCap;
use halvo_route::policy::Policy;
use halvo_route::release;
use halvo_route::ruling::{check_hosts, Ruling, RulingLine};

const RULING: &str = include_str!("../data/ruling.txt");
const HOSTS: &str = include_str!("../data/hosts.tsv");
const POLICY: &str = include_str!("../data/policy.tsv");
const CAP: &str = include_str!("../data/cap.txt");

#[test]
fn release_data_parses() {
    let line = RULING
        .strip_suffix('\n')
        .expect("the ruling file ends with LF");
    let line = RulingLine::parse(line).expect("the ruling line parses");
    let rows = check_hosts(HOSTS).expect("the table of hosts parses");
    let ruling = release::ruling();
    let hosted = line == RulingLine::Hosted && rows > 0;
    assert_eq!(
        ruling != Ruling::None,
        hosted,
        "{line:?} with {rows} rows; a hosted line over a table with a row also reads none while the rate of a row's tier in data/prices.tsv has a source that begins with fixture: (or that file does not parse)"
    );
    assert_eq!(ruling.rows().len(), if hosted { rows } else { 0 });

    let policy = Policy::parse_for(POLICY, ruling.allowed())
        .expect("the policy parses for the tiers that the ruling allows");
    assert_eq!(release::policy(), &policy);

    let line = CAP.strip_suffix('\n').expect("the cap file ends with LF");
    let cap = DefaultCap::parse(line).expect("the cap line parses");
    assert_eq!(release::cap(), Some(cap));
}
