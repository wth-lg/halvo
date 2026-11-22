//! The module agents (contracts I10.1, I8.1): the page's commands of
//! `halvo_coord::types::commands::LIST`, which the handler of halvo-coord-app answers for the
//! page of the window main alone, through the `Agents` that the app manages.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use halvo_control::client::Deadlines;
use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::commands::LIST;
use halvo_coord::types::{AgentId, Cause, ItemId, RunId, Snapshot};
use halvo_coord::words;
use halvo_coord_app::{Agents, Core, Reply};
use serde_json::Value;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Manager, Runtime};

use super::control::{Control, Places};
use super::names::{Names, AGENTS};
use super::Registration;
use crate::poller::{Polled, Views};
use halvo_control::paths::DataDir;

pub struct Module;

impl Registration for Module {
    fn names(&self) -> &'static Names {
        &AGENTS
    }
}

/// The handler of halvo-coord-app, but once the tray's Quit has begun: from then on every command
/// of `LIST` gets the fixed reply of `NoService` and reaches no call of the Core, while the quit
/// hook runs and after it has returned.
pub fn handler<R: Runtime>(invoke: Invoke<R>) -> bool {
    let quitting = crate::app::quitting(invoke.message.webview().app_handle());
    match LIST.iter().find(|c| c.name == invoke.message.command()) {
        Some(command) if quitting => {
            invoke.resolver.resolve(NoService.call(command.name, None));
            true
        }
        _ => halvo_coord_app::handler::<R>()(invoke),
    }
}

/// The app's hold on the agents' runs (the coordinator's lines 12397 and 12465 of the capture
/// run): it stands while a delete of everything runs at any door, behind the end of such a
/// delete until the view of a poll made after that end is drawn (`Owed`), and while the view's
/// store reads deleting or error locked, and is lifted once none of these stands. The core's
/// `hold_runs` refuses every start while it stands, the person's, the queue's and a tick's, and
/// stops the runs at work; calls to the core go one at a time, each with what is wanted at its
/// turn.
#[derive(Debug, Default)]
pub struct RunsHold {
    wanted: Mutex<Wanted>,
    applied: Mutex<bool>,
}

/// What the hold wants at a moment: it stands while a delete runs, while the newest view drawn
/// bars, or while a reading is owed. `late` is the moment at which the answer of a delete passed
/// its deadline, until a reading clears it.
#[derive(Debug, Default)]
struct Wanted {
    deletes: usize,
    barred: bool,
    owed: Owed,
    late: Option<Instant>,
}

impl Wanted {
    fn held(&self) -> bool {
        self.deletes > 0 || self.barred || self.owed != Owed::Nothing
    }

    /// Whether `poll` is a reading at `now`: a poll that answered always is; a poll that passed
    /// its deadline is not while a late answer stands and less than `Deadlines::V1.delete_all`
    /// has passed since it.
    fn reads(&self, poll: Polled, now: Instant) -> bool {
        !poll.passed_its_deadline
            || self
                .late
                .is_none_or(|late| now.saturating_duration_since(late) >= Deadlines::V1.delete_all)
    }
}

/// The reading that the end of a delete of everything owes the hold, in three steps.
///
/// 1. The end: `delete_ends`, from the drop of a door's guard (`HeldForADelete` at the service's
///    door and the offline door, the guard of Remove Halvo's list), which comes after the
///    delete's answer and before the door takes that answer into the views; from then on the
///    hold stands while the reading is owed.
/// 2. The fence: the first draw after that end sets it at the next call number of the app's
///    places, read under the hold's lock, and wakes the poller. Every poll at or past the fence
///    was made after the end, and so after the answer. No view that a draw reads before a poll
///    at or past the fence lifts the hold: the view of a poll made before the end, Connecting
///    behind a delete's answer, and the view of before drawn again behind a refusal.
/// 3. The reading: the first draw whose views hold the view of a poll at or past the fence
///    clears it, and the hold lifts there unless that view bars.
///
/// Behind a late answer the reading waits longer. At the service's door, a delete whose own
/// answer passed its deadline is marked (`answer_passed_its_deadline`, from the door's guard
/// before its drop): the service may still delete, and it answers no status while it does. Then
/// a poll that passes its deadline is no reading, and the first poll at or past the fence that
/// answers, with a status or another error, is the reading and clears the mark. Once
/// `Deadlines::V1.delete_all` (120 s, the wait for a delete's answer) has passed since the mark,
/// a poll that passes its deadline is a reading again. The offline door and Remove Halvo's list
/// take no answer of the service and are never marked.
///
/// It cannot stand for good while the poller runs: the door draws behind its answer and each
/// round of the poller draws, so the fence comes at the first of these draws; the woken
/// poller's next poll is made after it, and its answer is taken as a view, a status or any
/// error, no service among them. Two answers are not taken. One is a poll made before a newer
/// answer of a mutation, and the poller's next poll follows it. The other is a deadline while a
/// delete of the app is in flight, and that delete's own end owes a new reading. Behind a late
/// answer, a poll that passes its deadline is a reading again once the bound has passed. Where
/// no poller runs, as in the launch form `--delete-all` and in a test app, nothing clears it,
/// and at a quit the process may end first: there the hold stands until the process ends.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Owed {
    /// No reading is owed.
    #[default]
    Nothing,
    /// A delete ended, and no draw has set the fence since.
    Unfenced,
    /// The view of a poll whose call has this number or a later one.
    From(u64),
}

impl RunsHold {
    fn wanted(&self) -> MutexGuard<'_, Wanted> {
        self.wanted.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A delete of everything begins: the runs are held, and the call returns once the core has
    /// stopped them, within its grace.
    pub fn delete_begins(&self, core: Option<&dyn Core>) {
        self.wanted().deletes += 1;
        self.apply(core);
    }

    /// The answer of a delete of everything that runs passed its deadline at `at`, while the
    /// service may still delete: from its end on, a poll that passes its deadline is no reading
    /// until a poll answers or `Deadlines::V1.delete_all` has passed since `at` (`Owed`).
    pub fn answer_passed_its_deadline(&self, at: Instant) {
        self.wanted().late = Some(at);
    }

    /// A delete of everything ended: a reading is owed (`Owed`), and the hold stands on until
    /// it is drawn, and longer while another delete runs or the view bars.
    pub fn delete_ends(&self, core: Option<&dyn Core>) {
        {
            let mut wanted = self.wanted();
            wanted.deletes = wanted.deletes.saturating_sub(1);
            wanted.owed = Owed::Unfenced;
        }
        self.apply(core);
    }

    /// Whether `view` bars the agents' runs: its store reads deleting or error locked.
    pub fn bars(view: &crate::view_state::StatusView) -> bool {
        match view {
            crate::view_state::StatusView::Running(running) => {
                running.store.state == halvo_control::proto::StoreState::Deleting
                    || running.store.error == Some(halvo_control::proto::StoreErrorKind::Locked)
            }
            crate::view_state::StatusView::NoStatus(_) => false,
        }
    }

    /// What a draw at `now` tells the hold, from `views` at that moment: whether their view
    /// bars, and, behind a delete's end, the fence at the next call number of `places` or the
    /// reading (`Owed`). Returns whether what is wanted changed, for `apply`.
    fn told_by_a_draw(&self, views: &Views, places: &Places, now: Instant) -> bool {
        let (view, poll) = views.reading();
        let mut wanted = self.wanted();
        let held = wanted.held();
        wanted.barred = Self::bars(&view);
        let owed = wanted.owed;
        match owed {
            Owed::Unfenced => {
                wanted.owed = Owed::From(places.next_number());
                views.wake_the_poller();
            }
            Owed::From(fence)
                if poll.is_some_and(|poll| poll.number >= fence && wanted.reads(poll, now)) =>
            {
                wanted.owed = Owed::Nothing;
                wanted.late = None;
            }
            Owed::Nothing | Owed::From(_) => {}
        }
        wanted.held() != held
    }

    /// A draw of `views`, as `hold_by_the_view` makes it, with the core told at once and not
    /// on a thread of its own.
    #[cfg(test)]
    pub fn drawn(&self, core: Option<&dyn Core>, views: &Views, places: &Places) {
        self.drawn_at(core, views, places, Instant::now());
    }

    /// `drawn` at the moment `now` that the caller gives.
    #[cfg(test)]
    pub fn drawn_at(&self, core: Option<&dyn Core>, views: &Views, places: &Places, now: Instant) {
        if self.told_by_a_draw(views, places, now) {
            self.apply(core);
        }
    }

    /// What the newest view reads, `bars` of it, told with no views: the hold stands while it
    /// bars.
    #[cfg(test)]
    pub fn view_reads(&self, core: Option<&dyn Core>, barred: bool) {
        self.wanted().barred = barred;
        self.apply(core);
    }

    /// Whether the hold stands as last told to the core.
    #[cfg(test)]
    pub fn stands(&self) -> bool {
        *self.applied.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn apply(&self, core: Option<&dyn Core>) {
        let mut applied = self.applied.lock().unwrap_or_else(PoisonError::into_inner);
        let held = self.wanted().held();
        if held != *applied {
            *applied = held;
            if let Some(core) = core {
                core.hold_runs(held);
            }
        }
    }
}

/// What a draw of the app's view tells the agents' hold, from the drawer of `app::build`: the
/// views of the app's own state, read at the draw, and the next call number of its places, read
/// under the hold's lock; the draw lock keeps the draws in order. A change of what is wanted goes
/// to the core on a thread of its own. The view of the draw is not read: the views may hold a
/// newer one, taken while the draw ran.
pub fn hold_by_the_view<R: Runtime>(app: &AppHandle<R>, _view: &crate::view_state::StatusView) {
    let Some(state) = app.try_state::<super::capture::AppState>() else {
        return;
    };
    if !state
        .runs
        .told_by_a_draw(&state.views, state.control.places(), Instant::now())
    {
        return;
    }
    let core = app
        .try_state::<Agents>()
        .map(|agents| Arc::clone(&agents.0));
    let runs = Arc::clone(&state.runs);
    let _ = std::thread::Builder::new()
        .name("halvo-runs-hold".to_owned())
        .spawn(move || runs.apply(core.as_deref()));
}

/// The steps of the app's start that go with its `Core`, which the app takes on the thread of its
/// first tick.
pub type Start = Box<dyn FnOnce() + Send>;

/// The one place where the app makes its `Core`, and the steps of its start. On a Mac and on
/// Windows with a data directory it is the hub over the product's ports
/// (`halvo_coord_app::product`), whose count of agents that need the person goes to `tray` and
/// whose confirmations that only a native dialog answers are shown in the system's dialog of `app`
/// (`halvo_coord_app::System`: the alert on a Mac, the task dialog on Windows): its owner
/// streams, relay, run marks and defer go through `control`, its runs live under the data
/// directory, and its start's steps are `halvo_coord_app::product::start`. Elsewhere, and with no
/// data directory, it is `NoService`, and nothing calls `tray`.
pub fn core<R: Runtime>(
    app: &AppHandle<R>,
    tray: Box<dyn Fn(u32) + Send + Sync>,
    control: &Arc<dyn Control>,
    data_dir: Option<&DataDir>,
) -> (Agents, Start) {
    #[cfg(any(target_os = "macos", windows))]
    if let Some(dir) = data_dir {
        let hub = product_hub(app, tray, control, dir);
        let started = Arc::clone(&hub);
        return (
            Agents(hub),
            Box::new(move || halvo_coord_app::product::start(&started)),
        );
    }
    let _ = (app, tray, control, data_dir);
    (Agents(Arc::new(NoService)), Box::new(|| {}))
}

/// The runner of the app: the file halvo-runner with the system's suffix of an executable
/// (halvo-runner.exe on Windows) beside the app's own executable, absolute, as the supervisor
/// library takes it, or in `dir` where the app's path cannot be read.
#[cfg(any(target_os = "macos", windows))]
pub fn runner(dir: &DataDir) -> std::path::PathBuf {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| dir.path().to_path_buf());
    beside.join(format!("halvo-runner{}", std::env::consts::EXE_SUFFIX))
}

/// The hub over the product's ports, whose native confirmations are shown in the system's dialog
/// of `app`, and whose runner is `runner`.
#[cfg(any(target_os = "macos", windows))]
fn product_hub<R: Runtime>(
    app: &AppHandle<R>,
    tray: Box<dyn Fn(u32) + Send + Sync>,
    control: &Arc<dyn Control>,
    dir: &DataDir,
) -> Arc<halvo_coord_app::product::ProductHub> {
    use halvo_coord_app::product::{Calls, Places};
    let places = Places {
        data_dir: dir.path().to_path_buf(),
        runner: runner(dir),
        model: None,
    };
    let (streams, relayed, marked, deferred) = (
        Arc::clone(control),
        Arc::clone(control),
        Arc::clone(control),
        Arc::clone(control),
    );
    let calls = Calls {
        owner_stream: Arc::new(move || streams.owner_stream(streams.places().make())),
        relay: Arc::new(move |request: &[u8]| relayed.relay(relayed.places().make(), request)),
        mark: Arc::new(move |edge| marked.run_mark(marked.places().make(), edge)),
        defer: Arc::new(move |until| deferred.defer(deferred.places().make(), until)),
    };
    let dialog = Arc::new(halvo_coord_app::System::new(app.clone()));
    halvo_coord_app::product::hub(calls, places, tray, dialog)
}

/// The `Core` of the app until U5.4: `agents_snapshot` answers the stand-in state (`seq` 0,
/// empty lists, the note `note.no_service`), `agents_next` answers `{"closed":"yes"}` at once,
/// so that a view waits for nothing and calls again at its next show, every other command
/// answers `{"error":"no_service"}` where its row of `LIST` names that cause and
/// `{"error":"unknown_item"}` where it does not, and every hook does nothing.
pub struct NoService;

fn stand_in() -> Snapshot {
    Snapshot {
        seq: 0,
        rows: Vec::new(),
        items: Vec::new(),
        note: words::text("note.no_service", &[]).ok(),
    }
}

impl Core for NoService {
    fn call(&self, command: &str, _args: Option<&Value>) -> Reply {
        match LIST.iter().find(|c| c.name == command) {
            None => Reply::Error(Cause::Malformed),
            Some(c) if c.name == "agents_snapshot" => Reply::Snapshot(stand_in()),
            Some(c) if c.name == "agents_next" => Reply::Closed,
            Some(c) if c.errors.contains(&Cause::NoService) => Reply::Error(Cause::NoService),
            Some(_) => Reply::Error(Cause::UnknownItem),
        }
    }

    fn shown(&self) {}

    fn hidden(&self) {}

    fn destroyed(&self) {}

    fn tick(&self) {}

    fn quit(&self, _budget: Option<Duration>) -> Vec<(AgentId, RunId)> {
        Vec::new()
    }

    fn hold_runs(&self, _held: bool) -> Vec<(AgentId, RunId)> {
        Vec::new()
    }

    fn on_screen(&self, _item: &ItemId) {}

    fn answer_native(&self, _item: &ItemId, _answer: ConfirmAnswer) -> Result<(), Cause> {
        Err(Cause::NoService)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Condvar, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use halvo_control::client::{ClientError, Deadlines};
    use halvo_control::paths::DataDir;
    use halvo_control::proto::{DaemonStatus, ErrorCode, Platform, StoreState};
    use halvo_control::testing::{sample_status, ScriptedConnector, ScriptedDaemon};
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord::types::{AgentId, Cause, ItemId, RunId};
    use halvo_coord_app::{Agents, Core, Reply};
    use serde_json::Value;
    use tauri::test::{mock_builder, MockRuntime};
    use tauri::{App, Manager};

    use super::{hold_by_the_view, RunsHold};
    use crate::commands::capture::{
        delete_everything, run_offline, AppState, Before, DeleteEnd, Offline, DELETE_WAITS,
    };
    use crate::commands::control::{Control, Gated, Places};
    use crate::poller::Views;
    use crate::view_state::OfflineEnd;

    /// A core that records every `hold_runs` that reaches it.
    #[derive(Default)]
    struct Told {
        holds: Mutex<Vec<bool>>,
        told: Condvar,
    }

    impl Told {
        fn holds(&self) -> Vec<bool> {
            self.holds
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        fn wait_for(&self, count: usize) -> Vec<bool> {
            let holds = self.holds.lock().unwrap_or_else(PoisonError::into_inner);
            self.told
                .wait_timeout_while(holds, Duration::from_secs(30), |holds| holds.len() < count)
                .unwrap_or_else(PoisonError::into_inner)
                .0
                .clone()
        }
    }

    impl Core for Told {
        fn call(&self, _: &str, _: Option<&Value>) -> Reply {
            Reply::Closed
        }

        fn shown(&self) {}

        fn hidden(&self) {}

        fn destroyed(&self) {}

        fn tick(&self) {}

        fn quit(&self, _: Option<Duration>) -> Vec<(AgentId, RunId)> {
            Vec::new()
        }

        fn hold_runs(&self, held: bool) -> Vec<(AgentId, RunId)> {
            self.holds
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(held);
            self.told.notify_all();
            Vec::new()
        }

        fn on_screen(&self, _: &ItemId) {}

        fn answer_native(&self, _: &ItemId, _: ConfirmAnswer) -> Result<(), Cause> {
            Err(Cause::NoService)
        }
    }

    /// The app's state and `core` on the mock runtime, with no set-up and so no poller.
    fn app_of(core: &Arc<Told>) -> App<MockRuntime> {
        let core: Arc<dyn Core> = Arc::clone(core) as Arc<dyn Core>;
        mock_builder()
            .manage(AppState::new(|| Err::<DataDir, ()>(()), |_| Ok(())))
            .manage(Agents(core))
            .build(crate::app::context())
            .expect("the app builds on the mock runtime")
    }

    /// A delete of everything at the service's door that begins and ends with no poll between,
    /// whose answer is then taken and drawn.
    fn a_delete_answered_and_drawn(app: &App<MockRuntime>) {
        let state = app.state::<AppState>();
        let before = Before::of(app.handle(), &state);
        let call = state.control.places().make().number();
        drop(before.tell());
        let next = state.control.places().next_number();
        state.views.take_delete(call, next);
        hold_by_the_view(app.handle(), &state.views.view());
    }

    /// A poll made at this moment, answered with `status`, then drawn.
    fn a_poll_drawn(app: &App<MockRuntime>, status: DaemonStatus) {
        let state = app.state::<AppState>();
        let number = state.control.places().make().number();
        state.views.take_poll(number, &Ok(status));
        hold_by_the_view(app.handle(), &state.views.view());
    }

    fn deleting_with_the_field() -> DaemonStatus {
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.store = StoreState::Deleting;
        fields.store_id = None;
        fields.store_delete_unfinished = true;
        DaemonStatus::new(fields).expect("a status that holds")
    }

    #[test]
    fn behind_a_deletes_answer_the_runs_stay_held_until_a_poll_made_after_it_reads_the_store_open()
    {
        let core = Arc::new(Told::default());
        let app = app_of(&core);
        let state = app.state::<AppState>();
        a_delete_answered_and_drawn(&app);
        assert!(
            state.runs.stands(),
            "the hold stands behind the delete's answer, drawn as Connecting"
        );
        assert_eq!(
            core.holds(),
            [true],
            "no lift reaches the core behind the answer"
        );
        a_poll_drawn(&app, sample_status(Platform::Macos));
        assert_eq!(
            core.wait_for(2),
            [true, false],
            "the view of a poll made after the answer, which reads the store open, lifts the hold once"
        );
        assert!(!state.runs.stands(), "the hold is lifted");
    }

    #[test]
    fn behind_a_deletes_answer_the_runs_stay_held_where_that_poll_reads_deleting_with_the_field() {
        let core = Arc::new(Told::default());
        let app = app_of(&core);
        let state = app.state::<AppState>();
        a_delete_answered_and_drawn(&app);
        a_poll_drawn(&app, deleting_with_the_field());
        assert_eq!(
            core.holds(),
            [true],
            "no lift reaches the core from the delete's begin to the view of a poll that reads deleting"
        );
        assert!(
            state.runs.stands(),
            "the hold stands on while that view reads deleting with the field"
        );
    }

    #[test]
    fn behind_a_delete_whose_answer_passed_its_deadline_a_poll_past_its_deadline_is_no_reading() {
        let core = Arc::new(Told::default());
        let mut silent = ScriptedDaemon::new(sample_status(Platform::Macos));
        silent.set_silent(true);
        let control = Gated::new(ScriptedConnector::new(silent));
        let views = Views::default();
        let before = Before {
            modules: Vec::new(),
            runs: Arc::new(RunsHold::default()),
            core: Some(Arc::clone(&core) as Arc<dyn Core>),
        };
        let drawn = || {
            before
                .runs
                .drawn(before.core.as_deref(), &views, control.places());
        };
        let asked = delete_everything(&before, &control, &views, None, DELETE_WAITS);
        assert!(
            matches!(
                asked.end,
                DeleteEnd::Answered(Err(ClientError::Deadline { .. }))
            ),
            "the delete's own answer passed its deadline: {:?}",
            asked.end
        );
        views.take_delete(asked.call, asked.next_number_at_answer);
        drawn();
        let number = control.places().make().number();
        views.take_poll(number, &Err(ClientError::Deadline { seconds: 2 }));
        drawn();
        assert_eq!(
            core.holds(),
            [true],
            "behind a late answer, a poll that passed its deadline is no reading and lifts nothing"
        );
        let number = control.places().make().number();
        views.take_poll(number, &Ok(sample_status(Platform::Macos)));
        drawn();
        assert_eq!(
            core.holds(),
            [true, false],
            "the first poll behind it that answers, the store open, lifts the hold once"
        );
    }

    #[test]
    fn behind_a_late_answer_a_poll_past_its_deadline_lifts_the_hold_at_the_bound_and_not_before_it()
    {
        let core = Told::default();
        let runs = RunsHold::default();
        let views = Views::default();
        let places = Arc::<Places>::default();
        let late = Instant::now();
        let bound = Deadlines::V1.delete_all;
        let a_poll_past_its_deadline = || {
            views.take_poll(
                places.make().number(),
                &Err(ClientError::Deadline { seconds: 2 }),
            );
        };
        runs.delete_begins(Some(&core));
        runs.answer_passed_its_deadline(late);
        runs.delete_ends(Some(&core));
        runs.drawn_at(Some(&core), &views, &places, late);
        a_poll_past_its_deadline();
        runs.drawn_at(
            Some(&core),
            &views,
            &places,
            late + bound - Duration::from_millis(1),
        );
        assert_eq!(
            core.holds(),
            [true],
            "before the bound, a poll that passed its deadline lifts nothing"
        );
        a_poll_past_its_deadline();
        runs.drawn_at(Some(&core), &views, &places, late + bound);
        assert_eq!(
            core.holds(),
            [true, false],
            "at the bound, a poll that passed its deadline is a reading again and lifts the hold once"
        );
    }

    /// The offline delete of a stand-in program that finds no store, with no child to end.
    fn an_offline_run() -> Offline {
        let dir = DataDir::from_flag(crate::tests::scratch_dir("hold-no-service"))
            .expect("an absolute directory");
        let printed = format!("data_dir={}\nno_store\n", dir.path().display());
        Offline {
            program: PathBuf::from("/stand-in/halvo-capture"),
            data_dir: dir,
            children: None,
            run: Arc::new(move |_, _| OfflineEnd::Exited {
                code: 0,
                out: printed.clone(),
            }),
        }
    }

    /// The answers of a poll that finds no service: nothing listens, the data folder is held
    /// with nothing listening, or the system has no control channel.
    fn no_service() -> [ClientError; 3] {
        [
            ClientError::Unreachable {
                endpoint: String::new(),
            },
            ClientError::DataDirInUse {
                endpoint: String::new(),
            },
            ClientError::Unsupported,
        ]
    }

    /// The hold of `core` before a door, with no module.
    fn before_of(core: &Arc<Told>) -> Before {
        Before {
            modules: Vec::new(),
            runs: Arc::new(RunsHold::default()),
            core: Some(Arc::clone(core) as Arc<dyn Core>),
        }
    }

    #[test]
    fn behind_the_offline_doors_end_and_a_list_that_stopped_a_poll_that_finds_no_service_is_no_reading(
    ) {
        type Door = Box<dyn Fn(&Before, &Views)>;
        #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
        let mut doors: Vec<(&str, Door)> = vec![(
            "the offline door",
            Box::new(|before: &Before, _: &Views| {
                let _ = run_offline(&an_offline_run(), before);
            }),
        )];
        #[cfg(target_os = "macos")]
        doors.push((
            "a list of Remove Halvo that stopped",
            Box::new(|before: &Before, views: &Views| {
                let home = crate::tests::scratch_dir("hold-no-service-list").join("home");
                let data = DataDir::from_flag(home.join("data")).expect("an absolute directory");
                let stops = || {
                    (
                        crate::view_state::Outcome::Failed,
                        crate::view_state::Detail::default(),
                    )
                };
                let removal = crate::remove::remove_halvo(
                    crate::remove::Places::new(
                        &home,
                        halvo_control::model::APP_BUNDLE_ID,
                        &data,
                        true,
                    ),
                    None,
                    views.hold(),
                    &stops,
                    None,
                    before,
                );
                assert_eq!(removal.exit(), None, "the list stopped at its first step");
            }),
        ));
        for (door, end) in doors {
            let core = Arc::new(Told::default());
            let before = before_of(&core);
            let views = Views::default();
            let places = Arc::<Places>::default();
            let read = |answer: Result<DaemonStatus, ClientError>| {
                views.take_poll(places.make().number(), &answer);
                before.runs.drawn(before.core.as_deref(), &views, &places);
            };
            end(&before, &views);
            before.runs.drawn(before.core.as_deref(), &views, &places);
            for found in no_service() {
                let named = format!("{found:?}");
                read(Err(found));
                assert_eq!(
                    core.holds(),
                    [true],
                    "{door}: a poll that finds no service ({named}) is no reading and lifts nothing"
                );
            }
            read(Ok(deleting_with_the_field()));
            assert_eq!(
                core.holds(),
                [true],
                "{door}: a poll that answers that the store still deletes lifts nothing"
            );
            read(Ok(sample_status(Platform::Macos)));
            assert_eq!(
                core.holds(),
                [true, false],
                "{door}: the first poll that answers with the store open lifts the hold once"
            );
        }
    }

    #[test]
    fn behind_the_offline_doors_end_a_poll_that_finds_no_service_lifts_the_hold_at_the_bound_and_not_before_it(
    ) {
        let core = Arc::new(Told::default());
        let before = before_of(&core);
        let views = Views::default();
        let places = Arc::<Places>::default();
        let bound = Deadlines::V1.delete_all;
        let a_poll_that_finds_no_service_drawn_at = |now: Instant| {
            views.take_poll(
                places.make().number(),
                &Err(ClientError::Unreachable {
                    endpoint: String::new(),
                }),
            );
            before
                .runs
                .drawn_at(before.core.as_deref(), &views, &places, now);
        };
        let begun = Instant::now();
        let _ = run_offline(&an_offline_run(), &before);
        let ended = Instant::now();
        before
            .runs
            .drawn_at(before.core.as_deref(), &views, &places, ended);
        a_poll_that_finds_no_service_drawn_at(begun + bound - Duration::from_millis(1));
        assert_eq!(
            core.holds(),
            [true],
            "before the bound, a poll that finds no service lifts nothing"
        );
        a_poll_that_finds_no_service_drawn_at(ended + bound);
        assert_eq!(
            core.holds(),
            [true, false],
            "once the bound has passed since the end, a poll that finds no service is a reading again and lifts the hold once"
        );
    }

    #[test]
    fn behind_a_delete_that_the_service_answered_busy_a_poll_past_its_deadline_is_no_reading() {
        let core = Arc::new(Told::default());
        let mut busy = ScriptedDaemon::new(sample_status(Platform::Macos));
        busy.set_mutation_in_flight(true);
        let control = Gated::new(ScriptedConnector::new(busy));
        let views = Views::default();
        let before = before_of(&core);
        let drawn = || {
            before
                .runs
                .drawn(before.core.as_deref(), &views, control.places());
        };
        let asked = delete_everything(&before, &control, &views, None, DELETE_WAITS);
        assert!(
            matches!(
                &asked.end,
                DeleteEnd::Answered(Err(ClientError::Daemon(reply))) if reply.code() == ErrorCode::Busy
            ),
            "the service answered the delete busy: {:?}",
            asked.end
        );
        views.take_refusal(asked.next_number_at_answer);
        drawn();
        let number = control.places().make().number();
        views.take_poll(number, &Err(ClientError::Deadline { seconds: 2 }));
        drawn();
        assert_eq!(
            core.holds(),
            [true],
            "behind a busy answer, a poll that passed its deadline is no reading and lifts nothing"
        );
        let number = control.places().make().number();
        views.take_poll(number, &Ok(sample_status(Platform::Macos)));
        drawn();
        assert_eq!(
            core.holds(),
            [true, false],
            "the first poll behind it that answers, the store open, lifts the hold once"
        );
    }
}
