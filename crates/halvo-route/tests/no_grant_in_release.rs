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
