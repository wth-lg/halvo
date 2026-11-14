use halvo_learn_store::{Clock, LearnHost};

use crate::host::GateHost;
use crate::store::MemoryStore;

#[test]
fn the_gate_host_gives_the_agent_store_s_handler_the_clock_s_day() {
    for day in [1, 20_500, u32::MAX] {
        let host = GateHost::new(MemoryStore::new(), day);
        assert_eq!(host.today(), day);
        assert_eq!(host.agent_now().day, day);
        assert_eq!(host.now().day, day);
    }
    let host = GateHost::new(MemoryStore::new(), 20_500);
    assert_eq!(
        host.now().at.0,
        20_500 * 86_400_000_000_000 + 43_200_000_000_000
    );
}
