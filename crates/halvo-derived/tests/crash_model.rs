use std::collections::{HashMap, VecDeque};
use std::fmt;

const ORDINALS: u8 = 5;
const TOP: u8 = ORDINALS - 1;

#[derive(Clone, Copy, Debug)]
struct Episode {
    summary: u8,
    segments: u8,
}

const fn episode(summary: u8, segments: u8) -> Episode {
    Episode { summary, segments }
}

const BATCHES: [&[Episode]; 3] = [
    &[episode(0, 0b00001), episode(1, 0b10010)],
    &[episode(1, 0b10010), episode(2, 0b00101)],
    &[episode(3, 0b01000), episode(4, 0b11100)],
];
const RECORDS_PER_BATCH: usize = 2;
const READS_PER_STORE: u8 = 4;
const HANDLER_SEGMENTS: [u8; 3] = [0b10001, 0b00100, 0b10000];
const HANDLER_COMMITS_PER_STORE: u8 = 2;
const PURGES: [Purge; 15] = [
    Purge::Ordinals(0b00000),
    Purge::Ordinals(0b11111),
    Purge::Ordinals(0b10100),
    Purge::Below(0),
    Purge::Ordinals(0b00001),
    Purge::Ordinals(0b00010),
    Purge::Ordinals(0b00100),
    Purge::Ordinals(0b01000),
    Purge::Ordinals(0b10000),
    Purge::Ordinals(0b00011),
    Purge::Ordinals(0b11000),
    Purge::Below(1),
    Purge::Below(2),
    Purge::Below(3),
    Purge::Below(TOP),
];
const PURGES_PER_STORE: u8 = 3;
const DELETE_ALLS: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Purge {
    Ordinals(u8),
    Below(u8),
}

fn below(n: u8) -> u8 {
    ((1u16 << n) - 1) as u8
}

fn ordinals(set: u8) -> impl Iterator<Item = u8> {
    (0..ORDINALS).filter(move |o| set & (1 << o) != 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Kind {
    Evidence(u8),
    Step,
    Handler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Row {
    kind: Kind,
    capture: u8,
    segments: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Store {
    bound: Option<u8>,
    position: Option<u8>,
    mark: u8,
    purged: u8,
    summaries: u8,
    window: Vec<Row>,
}

impl Store {
    fn new() -> Store {
        Store {
            bound: None,
            position: None,
            mark: 0,
            purged: 0,
            summaries: 0,
            window: Vec::new(),
        }
    }

    fn names_purged(&self, mark: u8, segments: u8) -> bool {
        for segment in ordinals(segments) {
            if segment < mark {
                return true;
            }
            if self.purged & (1 << segment) != 0 {
                return true;
            }
        }
        false
    }

    fn raise_mark(&mut self, floor: u8) {
        let mut mark = self.mark.max(floor);
        self.purged &= !below(mark);
        while mark < TOP && self.purged & (1 << mark) != 0 {
            self.purged &= !below(mark + 1);
            mark += 1;
        }
        self.mark = mark;
    }

    fn remove_naming(&mut self, set: u8) {
        self.window.retain(|row| row.segments & set == 0);
    }

    fn insert(&mut self, row: Row) {
        self.window.push(row);
        self.window.sort();
    }

    fn purge(&mut self, set: u8) {
        let mark = self.mark;
        for ordinal in ordinals(set) {
            self.remove_naming(1 << ordinal);
            if ordinal >= mark {
                self.purged |= 1 << ordinal;
            }
        }
        self.raise_mark(mark);
    }

    fn purge_below(&mut self, first_live: u8) {
        self.remove_naming(below(first_live));
        self.raise_mark(first_live);
    }

    fn typed_commit(&mut self, kind: Kind, capture: u8, segments: u8) -> bool {
        let mark = self.mark;
        if self.names_purged(mark, segments) {
            return true;
        }
        self.insert(Row {
            kind,
            capture,
            segments,
        });
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Read {
    capture: u8,
    batch: u8,
}

impl Read {
    fn episodes(self) -> &'static [Episode] {
        BATCHES[usize::from(self.batch)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Next {
    Mark,
    Summary(u8),
    Evidence(u8),
    Position,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Txn {
    read: Read,
    working: Store,
    mark: u8,
    next: Next,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Work {
    Idle,
    Batch(Txn),
    Proposals(Read),
    Steps(Read),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Deleting {
    No,
    Started,
    DerivedGone,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct State {
    store: Option<Store>,
    capture: u8,
    shredded: u8,
    deleting: Deleting,
    owner: bool,
    read: Option<Read>,
    work: Work,
    reads: u8,
    purges: u8,
    handled: u8,
    deletes: u8,
    stepped: u8,
    stepped_twice: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Start,
    Read(u8),
    Commit,
    Refused,
    Repeated,
    MarkRead,
    SummaryWrite(u8),
    SummarySkip(u8),
    EvidenceWrite(u8),
    EvidenceDrop(u8),
    PositionAndCommit,
    Proposals,
    StepCommit,
    StepDrop,
    HandlerCommit(u8),
    HandlerDrop(u8),
    Purge(Purge),
    DeleteAllStart,
    DeleteDerived,
    DeleteAllEnd,
    Crash,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Purge(Purge::Ordinals(set)) => write!(f, "purge(ordinals {set:05b})"),
            Action::Purge(Purge::Below(n)) => write!(f, "purge_below({n})"),
            other => write!(f, "{other:?}"),
        }
    }
}

struct Step {
    action: Action,
    state: State,
    dropped: Option<u8>,
}

impl State {
    fn initial() -> State {
        State {
            store: None,
            capture: 0,
            shredded: 0,
            deleting: Deleting::No,
            owner: false,
            read: None,
            work: Work::Idle,
            reads: 0,
            purges: 0,
            handled: 0,
            deletes: 0,
            stepped: 0,
            stepped_twice: false,
        }
    }

    fn store(&mut self) -> &mut Store {
        self.store
            .as_mut()
            .expect("an owner that runs has opened its store")
    }

    fn violation(&self, reads: &mut usize) -> Option<&'static str> {
        *reads += 1;
        if self.stepped_twice {
            return Some("the steps of one batch ran twice");
        }
        let store = self.store.as_ref()?;
        for row in &store.window {
            if row.capture != self.capture {
                return Some(
                    "a window row names a segment of a capture store that delete-all destroyed",
                );
            }
            if row.segments & self.shredded != 0 {
                return Some("a window row names a shredded ordinal");
            }
        }
        for ordinal in ordinals(self.shredded) {
            if ordinal >= store.mark && store.purged & (1 << ordinal) == 0 {
                return Some(
                    "the store keeps neither its mark nor its set over a shredded ordinal",
                );
            }
        }
        if (below(store.mark) | store.purged) & !self.shredded != 0 {
            return Some("the store holds an ordinal as purged that no committed purge named");
        }
        if store.purged & below(store.mark) != 0 {
            return Some("the set of purged ordinals holds an ordinal below the mark");
        }
        if store.mark < TOP && store.purged & (1 << store.mark) != 0 {
            return Some("the mark has not risen over a purged ordinal that follows it");
        }
        None
    }

    fn after_episode(episode: u8, read: Read) -> Next {
        if usize::from(episode) + 1 < read.episodes().len() {
            Next::Summary(episode + 1)
        } else {
            Next::Position
        }
    }

    fn batch_step(&self, mut txn: Txn) -> Step {
        let mut next = self.clone();
        let mut dropped = None;
        let action = match txn.next {
            Next::Mark => {
                txn.mark = txn.working.mark;
                txn.next = Next::Summary(0);
                Action::MarkRead
            }
            Next::Summary(e) => {
                let episode = txn.read.episodes()[usize::from(e)];
                if txn.working.summaries & (1 << episode.summary) != 0 {
                    txn.next = State::after_episode(e, txn.read);
                    Action::SummarySkip(e)
                } else {
                    txn.working.summaries |= 1 << episode.summary;
                    txn.next = Next::Evidence(e);
                    Action::SummaryWrite(e)
                }
            }
            Next::Evidence(e) => {
                let episode = txn.read.episodes()[usize::from(e)];
                txn.next = State::after_episode(e, txn.read);
                if txn.working.names_purged(txn.mark, episode.segments) {
                    dropped = Some(episode.segments);
                    Action::EvidenceDrop(e)
                } else {
                    txn.working.insert(Row {
                        kind: Kind::Evidence(episode.summary),
                        capture: txn.read.capture,
                        segments: episode.segments,
                    });
                    Action::EvidenceWrite(e)
                }
            }
            Next::Position => {
                txn.working.bound = Some(txn.read.capture);
                txn.working.position = Some(txn.read.batch);
                next.store = Some(txn.working.clone());
                next.work = Work::Proposals(txn.read);
                return Step {
                    action: Action::PositionAndCommit,
                    state: next,
                    dropped,
                };
            }
        };
        next.work = Work::Batch(txn);
        Step {
            action,
            state: next,
            dropped,
        }
    }

    fn steps(&self) -> Vec<Step> {
        let mut out = Vec::new();
        let mut push = |action: Action, state: State| {
            out.push(Step {
                action,
                state,
                dropped: None,
            })
        };
        let live = self.deleting == Deleting::No;
        {
            let mut next = self.clone();
            next.owner = false;
            next.read = None;
            next.work = Work::Idle;
            push(Action::Crash, next);
        }
        if !self.owner && live {
            let mut next = self.clone();
            next.owner = true;
            next.store.get_or_insert_with(Store::new);
            push(Action::Start, next);
        }
        match self.deleting {
            Deleting::Started => {
                let mut next = self.clone();
                next.store = None;
                next.deleting = Deleting::DerivedGone;
                push(Action::DeleteDerived, next);
            }
            Deleting::DerivedGone => {
                let mut next = self.clone();
                next.capture += 1;
                next.shredded = 0;
                next.reads = 0;
                next.purges = 0;
                next.handled = 0;
                next.stepped = 0;
                next.deleting = Deleting::No;
                push(Action::DeleteAllEnd, next);
            }
            Deleting::No => {}
        }
        if live && self.deletes < DELETE_ALLS {
            let mut next = self.clone();
            next.deleting = Deleting::Started;
            next.deletes += 1;
            next.owner = false;
            next.read = None;
            next.work = Work::Idle;
            push(Action::DeleteAllStart, next);
        }
        if !(live && self.owner) {
            return out;
        }
        match &self.work {
            Work::Batch(txn) => {
                let step = self.batch_step(txn.clone());
                out.push(step);
                return out;
            }
            Work::Proposals(read) => {
                let mut next = self.clone();
                next.work = Work::Steps(*read);
                push(Action::Proposals, next);
                return out;
            }
            Work::Steps(read) => {
                let mut next = self.clone();
                next.work = Work::Idle;
                let segments = read.episodes()[read.episodes().len() - 1].segments;
                next.stepped_twice |= next.stepped & (1 << read.batch) != 0;
                next.stepped |= 1 << read.batch;
                let dropped = next
                    .store()
                    .typed_commit(Kind::Step, read.capture, segments);
                out.push(Step {
                    action: if dropped {
                        Action::StepDrop
                    } else {
                        Action::StepCommit
                    },
                    state: next,
                    dropped: dropped.then_some(segments),
                });
                return out;
            }
            Work::Idle => {}
        }
        let store = self
            .store
            .as_ref()
            .expect("an owner that runs has opened its store");
        match self.read {
            None if self.reads < READS_PER_STORE => {
                let first = store.position.map_or(0, |p| p + 1);
                let resend = store.position;
                for batch in [resend, Some(first)].into_iter().flatten() {
                    if usize::from(batch) < BATCHES.len() {
                        let mut next = self.clone();
                        next.reads += 1;
                        next.read = Some(Read {
                            capture: self.capture,
                            batch,
                        });
                        push(Action::Read(batch), next);
                    }
                }
            }
            None => {}
            Some(read) => {
                let mut next = self.clone();
                next.read = None;
                if store.bound.is_some_and(|bound| bound != read.capture) {
                    push(Action::Refused, next);
                } else if store.position.is_some_and(|p| read.batch <= p) {
                    push(Action::Repeated, next);
                } else {
                    next.work = Work::Batch(Txn {
                        read,
                        working: store.clone(),
                        mark: 0,
                        next: Next::Mark,
                    });
                    push(Action::Commit, next);
                }
            }
        }
        if self.purges < PURGES_PER_STORE {
            for purge in PURGES {
                let mut next = self.clone();
                next.purges += 1;
                match purge {
                    Purge::Ordinals(set) => {
                        next.store().purge(set);
                        next.shredded |= set;
                    }
                    Purge::Below(first_live) => {
                        next.store().purge_below(first_live);
                        next.shredded |= below(first_live);
                    }
                }
                push(Action::Purge(purge), next);
            }
        }
        if self.handled < HANDLER_COMMITS_PER_STORE {
            for segments in HANDLER_SEGMENTS {
                let mut next = self.clone();
                next.handled += 1;
                let capture = next.capture;
                let dropped = next.store().typed_commit(Kind::Handler, capture, segments);
                out.push(Step {
                    action: if dropped {
                        Action::HandlerDrop(segments)
                    } else {
                        Action::HandlerCommit(segments)
                    },
                    state: next,
                    dropped: dropped.then_some(segments),
                });
            }
        }
        out
    }
}

#[derive(Default)]
struct Walk {
    states: usize,
    invariant_reads: usize,
    transitions: usize,
    crashes: usize,
    purges_between_read_and_commit: usize,
    delete_alls_between_read_and_commit: usize,
    dropped_for_a_shredded_ordinal: usize,
    repeated: usize,
    skipped: usize,
    top_purged_at_the_mark: usize,
    second_store_states: usize,
    states_with_evidence: usize,
    states_with_step_rows: usize,
    states_with_handler_rows: usize,
    purges_that_removed_rows: usize,
    top_only_dropped: usize,
    deepest: usize,
}

fn fail(
    parent: &[Option<(usize, Action)>],
    at: usize,
    last: Option<Action>,
    fault: &str,
    state: &State,
) -> ! {
    let mut trace: Vec<Action> = last.into_iter().collect();
    let mut cursor = at;
    while let Some((from, action)) = parent[cursor] {
        trace.push(action);
        cursor = from;
    }
    trace.reverse();
    let mut text = format!(
        "crash model: violation: {fault}\ntrace of {} steps:\n",
        trace.len()
    );
    for (n, action) in trace.iter().enumerate() {
        text.push_str(&format!("  {:2}. {action}\n", n + 1));
    }
    text.push_str(&format!("state: {state:?}"));
    panic!("{text}");
}

fn walk() -> Walk {
    let mut walk = Walk::default();
    let mut index: HashMap<State, usize> = HashMap::new();
    let mut parent: Vec<Option<(usize, Action)>> = Vec::new();
    let mut depth: Vec<usize> = Vec::new();
    let mut queue = VecDeque::new();
    let first = State::initial();
    index.insert(first.clone(), 0);
    parent.push(None);
    depth.push(0);
    queue.push_back((first, 0));
    while let Some((state, at)) = queue.pop_front() {
        walk.states += 1;
        walk.deepest = walk.deepest.max(depth[at]);
        if state.capture > 0 {
            walk.second_store_states += 1;
        }
        if let Some(store) = &state.store {
            let holds = |kind: Kind| store.window.iter().any(|row| row.kind == kind);
            if store
                .window
                .iter()
                .any(|row| matches!(row.kind, Kind::Evidence(_)))
            {
                walk.states_with_evidence += 1;
            }
            if holds(Kind::Step) {
                walk.states_with_step_rows += 1;
            }
            if holds(Kind::Handler) {
                walk.states_with_handler_rows += 1;
            }
        }
        if let Some(fault) = state.violation(&mut walk.invariant_reads) {
            fail(&parent, at, None, fault, &state);
        }
        for step in state.steps() {
            walk.transitions += 1;
            match step.action {
                Action::Crash => walk.crashes += 1,
                Action::Purge(_) if state.read.is_some() => {
                    walk.purges_between_read_and_commit += 1
                }
                Action::DeleteAllStart if state.read.is_some() => {
                    walk.delete_alls_between_read_and_commit += 1
                }
                Action::Repeated => walk.repeated += 1,
                Action::SummarySkip(_) => walk.skipped += 1,
                Action::Purge(Purge::Ordinals(set))
                    if set & (1 << TOP) != 0
                        && state.store.as_ref().is_some_and(|store| store.mark == TOP) =>
                {
                    walk.top_purged_at_the_mark += 1
                }
                _ => {}
            }
            if let Some(segments) = step.dropped {
                if segments & state.shredded == 0 {
                    fail(
                        &parent,
                        at,
                        Some(step.action),
                        "a record that names no shredded ordinal was dropped",
                        &step.state,
                    );
                }
                walk.dropped_for_a_shredded_ordinal += 1;
                if segments == 1 << TOP {
                    walk.top_only_dropped += 1;
                }
            }
            if let (Action::Purge(_), Some(before), Some(after)) =
                (step.action, &state.store, &step.state.store)
            {
                if after.window.len() < before.window.len() {
                    walk.purges_that_removed_rows += 1;
                }
            }
            if !index.contains_key(&step.state) {
                let n = parent.len();
                index.insert(step.state.clone(), n);
                parent.push(Some((at, step.action)));
                depth.push(depth[at] + 1);
                queue.push_back((step.state, n));
            }
        }
    }
    walk
}

#[test]
fn no_window_row_names_a_shredded_ordinal_at_any_step_or_crash_of_commit_purge_and_delete_all() {
    let walk = walk();
    println!(
        "crash model: {} states, the invariant read at each of them ({} reads); {} transitions, {} of them a crash; \
         deepest trace {} steps",
        walk.states, walk.invariant_reads, walk.transitions, walk.crashes, walk.deepest
    );
    println!(
        "crash model bounds: ordinals {ORDINALS} (the ordinal {TOP} stands for 2^63 - 1); batches {} of {} records \
         each, {READS_PER_STORE} reads per capture store; purges {PURGES_PER_STORE} per capture store from {} kinds; \
         handler commits {HANDLER_COMMITS_PER_STORE} per capture store; delete-all {DELETE_ALLS}; crash points: \
         every state",
        BATCHES.len(),
        RECORDS_PER_BATCH,
        PURGES.len()
    );
    println!(
        "crash model coverage: {} purges between a batch's read and its commit, {} delete-alls between a batch's \
         read and its commit, {} records dropped for a purged ordinal, {} batches sent again and answered \
         repeated, {} episodes skipped for a stored summary, {} purges of the ordinal {TOP} with the mark at it, {} states after a \
         delete-all made the capture store again",
        walk.purges_between_read_and_commit,
        walk.delete_alls_between_read_and_commit,
        walk.dropped_for_a_shredded_ordinal,
        walk.repeated,
        walk.skipped,
        walk.top_purged_at_the_mark,
        walk.second_store_states
    );
    println!(
        "crash model rows: {} states hold an evidence row, {} a step's row, {} a handler's row; {} purges removed a \
         row; {} records that name only the ordinal {TOP} dropped",
        walk.states_with_evidence,
        walk.states_with_step_rows,
        walk.states_with_handler_rows,
        walk.purges_that_removed_rows,
        walk.top_only_dropped
    );
    assert!(BATCHES.iter().all(|batch| batch.len() == RECORDS_PER_BATCH));
    assert_eq!(walk.invariant_reads, walk.states);
    assert!(walk.crashes > 0);
    assert!(walk.purges_between_read_and_commit > 0);
    assert!(walk.delete_alls_between_read_and_commit > 0);
    assert!(walk.dropped_for_a_shredded_ordinal > 0);
    assert!(walk.repeated > 0);
    assert!(walk.skipped > 0);
    assert!(walk.top_purged_at_the_mark > 0);
    assert!(walk.second_store_states > 0);
    assert!(walk.states_with_evidence > 0);
    assert!(walk.states_with_step_rows > 0);
    assert!(walk.states_with_handler_rows > 0);
    assert!(walk.purges_that_removed_rows > 0);
    assert!(walk.top_only_dropped > 0);
}
