//! Properties of the owner on a model keyed on event identity: each emitted
//! event's id is recorded with the fate that the model expects for it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::sync::{mpsc, Arc, Mutex};
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
    HoldEmit(u8),
    Release,
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
        1 => (0u8..4).prop_map(Op::HoldEmit),
        1 => Just(Op::Release),
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

/// What the model expects: the gate of the owner, the count of resumes, and
/// the events that its sink holds, each with the count of resumes when its
/// emit began, received in the order in which their sends landed.
struct Model {
    writing: bool,
    resumes: u64,
    queue: VecDeque<(Uuid, u64)>,
    fates: HashMap<Uuid, Fate>,
    emitted_in_pause: HashSet<Uuid>,
}

impl Model {
    fn new() -> Model {
        Model {
            writing: false,
            resumes: 0,
            queue: VecDeque::new(),
            fates: HashMap::new(),
            emitted_in_pause: HashSet::new(),
        }
    }

    fn receive_one(&mut self) {
        if let Some((id, begun)) = self.queue.pop_front() {
            let fate = if self.writing && begun == self.resumes {
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

/// An emit held by the sink's seam after it read its resume mark: its id, the
/// count of resumes that it read, the sender that lets it go and its thread.
type Held = (
    Uuid,
    u64,
    mpsc::Sender<()>,
    thread::JoinHandle<sink::Emitted>,
);

fn hold(harness: &Harness, event: FloorEvent, begun: u64) -> Held {
    let id = event.id().uuid();
    let (marked_tx, marked_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let sink = harness.sink.clone();
    let thread = thread::spawn(move || {
        sink.emit_held(event, move || {
            marked_tx.send(()).unwrap();
            go_rx.recv().unwrap();
        })
    });
    marked_rx.recv().unwrap();
    (id, begun, go_tx, thread)
}

fn release(model: &mut Model, held: Held) {
    let (id, begun, go, thread) = held;
    go.send(()).unwrap();
    assert_eq!(thread.join().unwrap(), sink::Emitted::Queued);
    model.queue.push_back((id, begun));
}

fn run(ops: &[Op]) -> Result<(), TestCaseError> {
    let store = ModelStore::new();
    let clocks = ManualClocks::at(super::model_store::noon());
    let mut harness = Harness::with_clocks(store.clone(), open_floor(), 4096, clocks.clone());
    let mut model = Model::new();
    let mut paused = true;
    let mut held: VecDeque<Held> = VecDeque::new();
    for op in ops {
        match op {
            Op::Emit(kind) => {
                let event = event_of(*kind);
                let id = event.id().uuid();
                if paused {
                    model.emitted_in_pause.insert(id);
                }
                harness.emit(event);
                model.queue.push_back((id, model.resumes));
            }
            Op::HoldEmit(kind) => {
                let event = event_of(*kind);
                if paused {
                    model.emitted_in_pause.insert(event.id().uuid());
                }
                held.push_back(hold(&harness, event, model.resumes));
            }
            Op::Release => {
                if let Some(next) = held.pop_front() {
                    release(&mut model, next);
                }
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
                model.resumes += 1;
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
                while let Some(next) = held.pop_front() {
                    release(&mut model, next);
                }
                store.clear_write_scripts();
                let Answer::ShutDown(snapshot) = harness.ask(Command::Shutdown) else {
                    return Err(TestCaseError::fail("no shutdown answer"));
                };
                model.receive_all();
                prop_assert!(snapshot.is_conserved(), "{snapshot:?}");
                prop_assert_eq!(snapshot.events_pending, 0);
                harness = Harness::with_clocks(store.clone(), open_floor(), 4096, clocks.clone());
                model.writing = false;
                model.resumes = 0;
                paused = true;
            }
        }
        let stored: HashSet<Uuid> = store.records().iter().map(|r| r.id()).collect();
        prop_assert!(
            stored.is_disjoint(&model.emitted_in_pause),
            "an event emitted inside a pause is readable"
        );
    }
    while let Some(next) = held.pop_front() {
        release(&mut model, next);
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

fn stored_ids(store: &ModelStore) -> HashSet<Uuid> {
    store.records().iter().map(|r| r.id()).collect()
}

/// One trial: the owner is paused; the sink holds `fill` queued events and a
/// keystroke whose emit started inside the pause is blocked on the full sink.
/// ClosePause is then asked. Returns whether that keystroke is stored.
fn blocked_keystroke_trial(capacity: usize, fill: usize, blocked: usize) -> (bool, String) {
    let store = ModelStore::new();
    let mut h = Harness::over(store.clone(), open_floor(), capacity);
    // The owner starts discarding: this is the pause.
    for _ in 0..fill {
        h.emit(key());
    }
    let mut ids = Vec::new();
    let mut threads = Vec::new();
    let (marked_tx, marked_rx) = mpsc::channel();
    for _ in 0..blocked {
        let event = key();
        ids.push(event.id().uuid());
        let sink = h.sink.clone();
        let marked = marked_tx.clone();
        threads.push(thread::spawn(move || {
            sink.emit_held(event, move || marked.send(()).unwrap())
        }));
    }
    for _ in 0..blocked {
        marked_rx.recv().unwrap();
    }
    let blocked_now = threads.iter().filter(|t| !t.is_finished()).count();
    let answer = h.ask(Command::ClosePause);
    for t in threads {
        assert_eq!(t.join().unwrap(), sink::Emitted::Queued);
    }
    h.passes(4);
    let settled = h.ask(Command::Settle);
    let stored = stored_ids(&store);
    let kept = ids.iter().filter(|id| stored.contains(id)).count();
    (
        kept > 0,
        format!("blocked_at_close={blocked_now} answer={answer:?} kept_of_blocked={kept} settle={settled:?}"),
    )
}

/// Q1 and lens 1: a keystroke emitted inside the pause and blocked in its
/// emit on a full sink while ClosePause drains is stored.
#[test]
fn a_keystroke_blocked_in_its_emit_through_a_resume_is_discarded() {
    let trials = 200;
    let mut kept = 0;
    let mut example = String::new();
    for _ in 0..trials {
        let (k, text) = blocked_keystroke_trial(1, 1, 1);
        if k {
            kept += 1;
            if example.is_empty() {
                example = text;
            }
        }
    }
    println!("BLOCKED capacity=1 trials={trials} kept={kept} example: {example}");
    assert_eq!(
        kept, 0,
        "a keystroke emitted inside the pause was stored in {kept} of {trials} trials"
    );
}

/// The same at the product's sink capacity of 2,048 with five blocked
/// emitters, as five sources would be.
#[test]
fn the_same_at_capacity_2048_with_five_blocked_sources() {
    let trials = 40;
    let mut kept = 0;
    let mut example = String::new();
    for _ in 0..trials {
        let (k, text) = blocked_keystroke_trial(2048, 2048, 5);
        if k {
            kept += 1;
            if example.is_empty() {
                example = text;
            }
        }
    }
    println!("BLOCKED capacity=2048 blocked=5 trials={trials} kept={kept} example: {example}");
    assert_eq!(
        kept, 0,
        "a keystroke emitted inside the pause was stored in {kept} of {trials} trials"
    );
}

#[test]
fn an_emit_held_through_a_resume_is_discarded_by_the_model_and_the_owner() {
    run(&[
        Op::HoldEmit(1),
        Op::ClosePause,
        Op::Release,
        Op::Pass,
        Op::HoldEmit(0),
        Op::OpenPause,
        Op::ClosePause,
        Op::Release,
        Op::Emit(3),
        Op::Settle,
    ])
    .unwrap();
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
