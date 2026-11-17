use halvo_agent_store::Host;
use halvo_learn_store::{Clock, LearnHost};

use crate::host::GateHost;
use crate::store::MemoryStore;

const LAST_DAY_OF_AN_INSTANT: u32 = 106_751;

#[test]
fn the_gate_host_gives_the_agent_store_s_handler_the_clock_s_day_on_every_day_that_an_instant_can_hold(
) {
    for day in [0, 1, 20_500, LAST_DAY_OF_AN_INSTANT] {
        let host = GateHost::new(MemoryStore::new(), day);
        assert_eq!(host.today(), day);
        assert_eq!(
            host.store.day(host.agent_now().at),
            day,
            "the agent store takes the day of a request from its instant, an i64 of nanoseconds"
        );
        assert_eq!(host.store.day(host.now().at), day);
    }
    let host = GateHost::new(MemoryStore::new(), 20_500);
    assert_eq!(
        host.now().at.0,
        20_500 * 86_400_000_000_000 + 43_200_000_000_000
    );
}

#[test]
fn after_the_last_day_of_an_instant_the_agent_store_s_day_stays_at_that_day() {
    for day in [LAST_DAY_OF_AN_INSTANT + 1, u32::MAX] {
        let host = GateHost::new(MemoryStore::new(), day);
        assert_eq!(host.today(), day);
        assert_eq!(
            host.store.day(host.agent_now().at),
            LAST_DAY_OF_AN_INSTANT,
            "an i64 of nanoseconds holds no instant after day {LAST_DAY_OF_AN_INSTANT}"
        );
    }
}
