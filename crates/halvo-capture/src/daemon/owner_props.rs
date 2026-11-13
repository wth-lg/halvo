//! Properties of the owner on a model keyed on event identity: each emitted
//! event's id is recorded with the fate that the model expects for it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use halvo_control::model::StorageCap;
use proptest::prelude::*;
use uuid::Uuid;

use super::maintenance::{SystemClocks, SHRED_CHUNK};
use super::model_store::{
    foreground_now, frame, key, open_floor, title_of, Harness, ManualClocks, ModelStore,
    WriteScript,
};
use super::owner::{spawn, Answer, Command, OwnerParts, Report};
use super::port::WriteFault;
use crate::event::FloorEvent;
use crate::sink;

#[derive(Debug, Clone)]
enum Op {
    Emit(u8),
    Pass,
    OpenPause,
    ClosePause,
    Settle,
    FailNextWrite,
    Advance(u8),
    Restart,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        8 => (0u8..4).prop_map(Op::Emit),
        3 => Just(Op::Pass),
        1 => Just(Op::OpenPause),
        2 => Just(Op::ClosePause),
        1 => Just(Op::Settle),
        1 => Just(Op::FailNextWrite),
        1 => (1u8..90).prop_map(Op::Advance),
        1 => Just(Op::Restart),
    ]
}

fn event_of(kind: u8) -> FloorEvent {
    match kind {
        0 => foreground_now(),
        1 => key(),
        2 => frame(),
        _ => title_of(3, 3),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Persisted,
    Discarded,
}

/// What the model expects: the gate of the owner and the events that its sink
/// holds, received in the order of their emits.
struct Model {
    writing: bool,
    queue: VecDeque<Uuid>,
    fates: HashMap<Uuid, Fate>,
    emitted_in_pause: HashSet<Uuid>,
}

impl Model {
    fn new() -> Model {
        Model {
            writing: false,
            queue: VecDeque::new(),
            fates: HashMap::new(),
            emitted_in_pause: HashSet::new(),
        }
    }

    fn receive_one(&mut self) {
        if let Some(id) = self.queue.pop_front() {
            let fate = if self.writing {
                Fate::Persisted
            } else {
                Fate::Discarded
            };
            self.fates.insert(id, fate);
        }
    }

    fn receive_all(&mut self) {
        while !self.queue.is_empty() {
            self.receive_one();
        }
    }
}

fn run(ops: &[Op]) -> Result<(), TestCaseError> {
    let store = ModelStore::new();
    let clocks = ManualClocks::at(super::model_store::noon());
    let mut harness = Harness::with_clocks(store.clone(), open_floor(), 4096, clocks.clone());
    let mut model = Model::new();
    let mut paused = true;
    for op in ops {
        match op {
            Op::Emit(kind) => {
                let event = event_of(*kind);
                let id = event.id().uuid();
                if paused {
                    model.emitted_in_pause.insert(id);
                }
                harness.emit(event);
                model.queue.push_back(id);
            }
            Op::Pass => {
                harness.pass();
                model.receive_one();
            }
            Op::OpenPause => {
                harness.ask(Command::OpenPause);
                model.writing = false;
                paused = true;
                model.receive_one();
            }
            Op::ClosePause => {
                harness.ask(Command::ClosePause);
                model.receive_all();
                model.writing = true;
                paused = false;
                model.receive_one();
            }
            Op::Settle => {
                let Answer::Settled(snapshot) = harness.ask(Command::Settle) else {
                    return Err(TestCaseError::fail("no settle answer"));
                };
                model.receive_all();
                prop_assert!(
                    snapshot.is_conserved(),
                    "received is not persisted + discarded + lost + pending: {snapshot:?}"
                );
            }
            Op::FailNextWrite => {
                store.script_writes([WriteScript::Fail(WriteFault::Io(io::ErrorKind::Other))]);
            }
            Op::Advance(seconds) => clocks.advance(Duration::from_secs(u64::from(*seconds))),
            Op::Restart => {
                store.clear_write_scripts();
                let Answer::ShutDown(snapshot) = harness.ask(Command::Shutdown) else {
                    return Err(TestCaseError::fail("no shutdown answer"));
                };
                model.receive_all();
                prop_assert!(snapshot.is_conserved(), "{snapshot:?}");
                prop_assert_eq!(snapshot.events_pending, 0);
                harness = Harness::with_clocks(store.clone(), open_floor(), 4096, clocks.clone());
                model.writing = false;
                paused = true;
            }
        }
        let stored: HashSet<Uuid> = store.records().iter().map(|r| r.id()).collect();
        prop_assert!(
            stored.is_disjoint(&model.emitted_in_pause),
            "an event emitted inside a pause is readable"
        );
    }
    store.clear_write_scripts();
    let Answer::Settled(snapshot) = harness.ask(Command::Settle) else {
        return Err(TestCaseError::fail("no settle answer"));
    };
    model.receive_all();
    prop_assert!(snapshot.is_conserved(), "{snapshot:?}");
    prop_assert_eq!(snapshot.events_pending, 0);
    let records = store.records();
    let mut stored = HashSet::new();
    for record in &records {
        prop_assert!(stored.insert(record.id()), "a record was stored twice");
    }
    let expected: HashSet<Uuid> = model
        .fates
        .iter()
        .filter(|(_, fate)| **fate == Fate::Persisted)
        .map(|(id, _)| *id)
        .collect();
    prop_assert_eq!(stored, expected);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 96,
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Off)),
        ..ProptestConfig::default()
    })]

    #[test]
    fn no_event_of_a_pause_is_readable_and_every_event_has_one_fate(
        ops in proptest::collection::vec(op(), 1..80)
    ) {
        run(&ops)?;
    }
}

#[test]
fn an_event_queued_through_a_resume_is_discarded() {
    run(&[Op::Emit(0), Op::ClosePause, Op::Restart]).unwrap();
    run(&[
        Op::ClosePause,
        Op::OpenPause,
        Op::Emit(1),
        Op::Emit(2),
        Op::ClosePause,
        Op::Settle,
    ])
    .unwrap();
}

#[test]
fn a_resume_keeps_the_first_event_after_it() {
    run(&[
        Op::ClosePause,
        Op::Emit(1),
        Op::Pass,
        Op::OpenPause,
        Op::Emit(1),
        Op::ClosePause,
        Op::Emit(0),
        Op::Pass,
    ])
    .unwrap();
}

#[test]
fn a_restart_starts_numbers_again_and_keeps_identities_apart() {
    run(&[
        Op::ClosePause,
        Op::Emit(0),
        Op::Emit(1),
        Op::Settle,
        Op::Restart,
        Op::ClosePause,
        Op::Emit(0),
        Op::Emit(3),
        Op::FailNextWrite,
        Op::Settle,
        Op::Advance(2),
        Op::Pass,
    ])
    .unwrap();
}

#[test]
fn keystrokes_outside_the_pause_survive_a_frame_flood_once_each() {
    let store = ModelStore::new();
    let (sink, writer) = sink::channel(2048);
    let owner = spawn(OwnerParts {
        writer,
        store: store.clone(),
        clocks: SystemClocks::new(),
        floor: open_floor(),
        cap: StorageCap::DEFAULT,
        shred_chunk: SHRED_CHUNK,
    })
    .unwrap();
    let ids = Arc::new(Mutex::new(Vec::<(Uuid, bool)>::new()));
    let flood = |keys: usize, frames: usize, outside: bool| {
        let mut threads = Vec::new();
        for _ in 0..2 {
            let (key_sink, ids) = (sink.clone(), ids.clone());
            threads.push(thread::spawn(move || {
                for _ in 0..keys / 2 {
                    let event = key();
                    let id = event.id().uuid();
                    key_sink.emit_event(event);
                    ids.lock().unwrap().push((id, outside));
                }
            }));
            let frame_sink = sink.clone();
            threads.push(thread::spawn(move || {
                for _ in 0..frames / 2 {
                    frame_sink.emit_event(frame());
                }
            }));
        }
        threads
    };
    let join = |threads: Vec<thread::JoinHandle<()>>| {
        for thread in threads {
            thread.join().unwrap();
        }
    };
    owner.handle.call(Command::ClosePause).unwrap();
    join(flood(5_000, 50_000, true));
    owner.handle.call(Command::Settle).unwrap();
    owner.handle.call(Command::OpenPause).unwrap();
    join(flood(100, 1_000, false));
    owner.handle.call(Command::Settle).unwrap();
    owner.handle.call(Command::ClosePause).unwrap();
    store.script_writes([WriteScript::Fail(WriteFault::Io(io::ErrorKind::Other))]);
    let running = flood(5_000, 49_000, true);
    loop {
        match owner.reports.recv_timeout(Duration::from_secs(60)) {
            Ok(Report::DrainFailing { .. }) => break,
            Ok(_) => {}
            Err(_) => panic!("the injected write fault was not reported"),
        }
    }
    owner.handle.call(Command::Settle).unwrap();
    join(running);
    let mut last = None;
    for _ in 0..100 {
        let Ok(Answer::Settled(snapshot)) = owner.handle.call(Command::Settle) else {
            panic!("no settle answer");
        };
        last = Some(snapshot);
        if snapshot.events_pending == 0 {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let snapshot = last.unwrap();
    assert_eq!(snapshot.events_pending, 0, "the drain did not recover");
    assert!(snapshot.is_conserved());
    assert_eq!(snapshot.events_lost, 0);
    owner.handle.call(Command::Shutdown).unwrap();
    owner.join().unwrap();

    let records = store.records();
    let mut seen = HashSet::new();
    for record in &records {
        assert!(seen.insert(record.id()), "a record is stored twice");
    }
    let mut keys_outside = 0;
    for (id, outside) in ids.lock().unwrap().iter() {
        if *outside {
            keys_outside += 1;
            assert!(
                seen.contains(id),
                "a keystroke received outside the pause is missing"
            );
        } else {
            assert!(
                !seen.contains(id),
                "a keystroke emitted inside the pause was stored"
            );
        }
    }
    assert_eq!(keys_outside, 10_000);
    let stored_keys = records.iter().filter(|r| r.kind() == "keystroke").count();
    assert_eq!(stored_keys, 10_000);
    assert!(store
        .write_calls()
        .iter()
        .any(|(_, answer)| answer.is_err()));
}
