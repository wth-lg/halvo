mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::*;
use halvo_coord::items::drive::{flush, log_text, store as store_events, Ledger};
use halvo_coord::ports::{Clock as CoordClock, Owner, PortError};
use halvo_coord::types::{
    Agent, AgentId, Body, Event, HoldSetCause, Limits, Name, Reply, ReplyRow, RunId, What,
    Workflow, WorkflowId,
};
use halvo_coord_store::{coordinator, Clock, CoordReply, MemoryStore};
use halvo_schema::coordinator::record::What as Row;
use halvo_schema::Instant;

const MS: i64 = 1_000_000;
const HOUR: i64 = 3_600;

/// One clock for the coordinator and the owner: the instant in nanoseconds and the zone in seconds east of UTC, each
/// of which the test sets.
#[derive(Clone)]
struct Shared {
    now: Rc<Cell<i64>>,
    zone: Rc<Cell<i64>>,
}

impl Shared {
    fn local_day(&self, ns: i64) -> u32 {
        u32::try_from((ns.div_euclid(NS) + self.zone.get()).div_euclid(DAY_S)).unwrap_or(0)
    }

    /// Sets the clock to `seconds` after local midnight of `day` in the zone that it reads.
    fn set_local(&self, day: u32, seconds: i64) {
        self.now.set(at(day, seconds, self.zone.get()));
    }
}

impl Clock for Shared {
    fn now(&self) -> Instant {
        Instant(self.now.get())
    }

    fn day(&self, at: Instant) -> u32 {
        self.local_day(at.0)
    }
}

/// The coordinator's ports over the handler and a store in memory on the shared clock: `log` and `history` send the
/// bodies `coord_log` and `coord_history` as halvo-coord writes them, and `log` answers busy while `busy` is set.
struct Pair {
    store: RefCell<MemoryStore>,
    clock: Shared,
    busy: Cell<bool>,
}

fn refused(name: halvo_schema::ErrorName) -> PortError {
    PortError::Refused(Name::parse(name.as_str()).expect("an error name"))
}

impl CoordClock for Pair {
    fn now(&self) -> Result<u64, PortError> {
        u64::try_from(self.clock.now.get() / MS).map_err(|_| PortError::NoService)
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        let ms = i64::try_from(at).map_err(|_| PortError::NoService)?;
        Ok(self.clock.local_day(ms * MS))
    }
}

impl Owner for Pair {
    type Config = halvo_coord::mem::Config;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        if self.busy.get() {
            return Err(PortError::Busy);
        }
        let first = events.first().ok_or(PortError::NoService)?;
        let body = log_text(&first.workflow, &first.agent, events);
        let request = request("coordinator_write", &body);
        match coordinator(&mut *self.store.borrow_mut(), &request) {
            Ok(CoordReply::Stored(s)) if s.stored as usize == events.len() => Ok(()),
            Ok(_) => Err(PortError::NoService),
            Err(name) => Err(refused(name)),
        }
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<(Vec<Event>, u32), PortError> {
        let n = if agent.as_str() == common::agent(1) {
            1
        } else {
            2
        };
        let after = (after > 0).then_some(after);
        let request = request("coordinator_write", &history_body(n, n, after));
        match coordinator(&mut *self.store.borrow_mut(), &request) {
            Ok(CoordReply::History(h)) => {
                let events = h
                    .records
                    .into_iter()
                    .map(|r| {
                        assert!(matches!(r.what, Row::HoldSet { .. }), "a hold_set row");
                        Event {
                            workflow: WorkflowId::parse(r.workflow.as_str()).unwrap(),
                            agent: AgentId::parse(r.agent.as_str()).unwrap(),
                            seq: r.seq,
                            day: r.day,
                            at: r.at.0,
                            what: hold(),
                        }
                    })
                    .collect();
                Ok((events, h.last))
            }
            Ok(_) => Err(PortError::NoService),
            Err(name) => Err(refused(name)),
        }
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        Err(PortError::NoService)
    }

    fn agents(&self) -> Result<Vec<Agent<Self::Config>>, PortError> {
        Err(PortError::NoService)
    }

    fn send(&self, _: &Body) -> Result<Reply, PortError> {
        Err(PortError::NoService)
    }

    fn replies(&self, _: &WorkflowId, _: &AgentId, _: &RunId) -> Result<Vec<ReplyRow>, PortError> {
        Err(PortError::NoService)
    }
}

fn hold() -> What {
    What::HoldSet {
        cause: HoldSetCause::Person,
    }
}

/// The ports on a clock in the zone `zone`, with workflow 1 of agent 1 and workflow 2 of agent 2 stored, and the
/// ledger of workflow 1 and agent 1.
fn pair(zone: i64) -> (Pair, Ledger) {
    let clock = Shared {
        now: Rc::new(Cell::new(0)),
        zone: Rc::new(Cell::new(zone)),
    };
    clock.set_local(TODAY, 12 * HOUR);
    let mut store = MemoryStore::new(Box::new(clock.clone()));
    store.put_workflow(workflow(1, Some(1)));
    store.put_workflow(workflow(2, Some(2)));
    let ledger = Ledger::new(
        WorkflowId::parse(&wf(1)).unwrap(),
        AgentId::parse(&agent(1)).unwrap(),
        0,
    );
    (
        Pair {
            store: RefCell::new(store),
            clock,
            busy: Cell::new(false),
        },
        ledger,
    )
}

/// Each stored row of workflow 1 and agent 1 as its `seq`, its `day` and the local day of its `at` by the clock's
/// zone at the read.
fn stored_days(ports: &Pair) -> Vec<(u32, u32, u32)> {
    let request = request("coordinator_write", &history_body(1, 1, None));
    let Ok(CoordReply::History(h)) = coordinator(&mut *ports.store.borrow_mut(), &request) else {
        panic!("no history");
    };
    h.records
        .iter()
        .map(|r| (r.seq.get(), r.day, ports.clock.local_day(r.at.0)))
        .collect()
}

fn nothing_refused(ledger: &mut Ledger, case: &str) {
    assert!(ledger.waiting().is_empty(), "{case}: rows wait");
    assert!(ledger.taken_off().is_empty(), "{case}: rows taken off");
    assert!(!ledger.refused(), "{case}: a refusal");
}

const ZONES: [i64; 6] = [
    0,
    -5 * HOUR,
    2 * HOUR,
    9 * HOUR + 1_800,
    14 * HOUR,
    -12 * HOUR,
];

#[test]
fn a_row_that_waits_over_local_midnight_and_the_row_after_it_are_each_stored_on_their_day() {
    for zone in ZONES {
        let (ports, mut ledger) = pair(zone);
        let limits = Limits::PRODUCT;
        ports.clock.set_local(TODAY, DAY_S - 1);
        ports.busy.set(true);
        assert_eq!(
            store_events(&ports, &mut ledger, &limits, vec![hold()]),
            Err(PortError::Busy),
            "zone {zone}"
        );
        assert_eq!(ledger.waiting().len(), 1, "zone {zone}");
        ports.clock.set_local(TODAY + 1, 1);
        ports.busy.set(false);
        assert_eq!(
            store_events(&ports, &mut ledger, &limits, vec![hold()]),
            Ok(()),
            "zone {zone}"
        );
        nothing_refused(&mut ledger, &format!("zone {zone}"));
        assert_eq!(
            stored_days(&ports),
            [(1, TODAY, TODAY), (2, TODAY + 1, TODAY + 1)],
            "zone {zone}"
        );
    }
}

#[test]
fn rows_stamped_on_each_side_of_local_midnight_are_stored_on_the_day_of_their_instant() {
    for zone in ZONES {
        let (ports, mut ledger) = pair(zone);
        let limits = Limits::PRODUCT;
        let mut want = Vec::new();
        for (n, (day, seconds)) in [
            (TODAY, DAY_S - 2),
            (TODAY, DAY_S - 1),
            (TODAY + 1, 0),
            (TODAY + 1, 1),
        ]
        .into_iter()
        .enumerate()
        {
            ports.clock.set_local(day, seconds);
            assert_eq!(
                store_events(&ports, &mut ledger, &limits, vec![hold()]),
                Ok(()),
                "zone {zone} row {n}"
            );
            want.push((n as u32 + 1, day, day));
        }
        nothing_refused(&mut ledger, &format!("zone {zone}"));
        assert_eq!(stored_days(&ports), want, "zone {zone}");
    }
}

#[test]
fn a_change_of_zone_either_way_between_the_stamp_and_the_send_refuses_no_row() {
    for from in ZONES {
        for to in ZONES {
            for stamped in [HOUR / 2, DAY_S - HOUR / 2, DAY_S - 1, 0] {
                let case = format!("zone {from} to {to}, stamped at {stamped}");
                let (ports, mut ledger) = pair(from);
                let limits = Limits::PRODUCT;
                ports.clock.set_local(TODAY, stamped);
                ports.busy.set(true);
                assert_eq!(
                    store_events(&ports, &mut ledger, &limits, vec![hold(), hold()]),
                    Err(PortError::Busy),
                    "{case}"
                );
                ports.clock.zone.set(to);
                ports.clock.now.set(ports.clock.now.get() + 60 * NS);
                ports.busy.set(false);
                assert_eq!(flush(&ports, &mut ledger, &limits), Ok(()), "{case}");
                nothing_refused(&mut ledger, &case);
                let days = stored_days(&ports);
                assert_eq!(days.len(), 2, "{case}");
                for (seq, day, local) in days {
                    assert_eq!(day, local, "{case}: seq {seq}");
                }
            }
        }
    }
}

#[test]
fn after_the_widest_change_of_zone_both_bodies_are_answered_and_the_seq_goes_on() {
    for (from, to) in [(14 * HOUR, -12 * HOUR), (-12 * HOUR, 14 * HOUR)] {
        let case = format!("zone {from} to {to}");
        let (ports, mut ledger) = pair(from);
        let limits = Limits::PRODUCT;
        for (day, seconds) in [(TODAY - 1, DAY_S - 1), (TODAY, 1)] {
            ports.clock.set_local(day, seconds);
            assert_eq!(
                store_events(&ports, &mut ledger, &limits, vec![hold()]),
                Ok(()),
                "{case}"
            );
        }
        ports.clock.zone.set(to);
        ports.clock.now.set(ports.clock.now.get() + NS);
        let today = ports.clock.local_day(ports.clock.now.get());
        let ahead = if from > to { 2 } else { -1 };
        assert_eq!(
            i64::from(TODAY) - i64::from(today),
            ahead,
            "{case}: the newest row lies {ahead} days after the clock's day"
        );
        let mut fresh = Ledger::new(
            WorkflowId::parse(&wf(1)).unwrap(),
            AgentId::parse(&agent(1)).unwrap(),
            0,
        );
        let (events, last) = ports
            .history(&AgentId::parse(&agent(1)).unwrap(), 0)
            .expect("the history after the change");
        assert_eq!((events.len(), last), (2, 2), "{case}");
        fresh.follow(last);
        assert_eq!(
            store_events(&ports, &mut fresh, &limits, vec![hold()]),
            Ok(()),
            "{case}"
        );
        nothing_refused(&mut fresh, &case);
        let days = stored_days(&ports);
        assert_eq!(
            days.iter().map(|d| d.0).collect::<Vec<_>>(),
            [1, 2, 3],
            "{case}"
        );
        assert_eq!(days[2].1, days[2].2, "{case}");
        assert!(
            days[2].1.abs_diff(days[1].1) <= 2,
            "{case}: {} and {}",
            days[2].1,
            days[1].1
        );
    }
}
