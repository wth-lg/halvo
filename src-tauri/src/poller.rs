use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use halvo_control::client::ClientError;
use halvo_control::proto::DaemonStatus;

use crate::commands::control::Control;
use crate::commands::Registration;
use crate::tray::TrayInstalled;
use crate::view_state::{view_of, Hold, NoStatusService, StatusView};

pub const POLL: Duration = Duration::from_millis(halvo_control::POLL_MS);

#[derive(Debug)]
struct Latest {
    view: StatusView,
    status: Option<DaemonStatus>,
    mutation_mark: u64,
    poll_number: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollAnswer {
    Stands,
    DroppedAsOlderThanTheLastMutation,
}

/// What a draw of the app's view does: the tray's refresh and every module's `on_status`.
pub struct Drawer {
    refresh: Box<dyn Fn(&StatusView) + Send + Sync>,
    modules: Vec<&'static dyn Registration>,
}

impl Drawer {
    pub fn new(
        refresh: Box<dyn Fn(&StatusView) + Send + Sync>,
        modules: Vec<&'static dyn Registration>,
    ) -> Self {
        Drawer { refresh, modules }
    }
}

impl std::fmt::Debug for Drawer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drawer")
            .field("modules", &self.modules.len())
            .finish()
    }
}

#[derive(Debug)]
pub struct Views {
    latest: Mutex<Latest>,
    woken: Mutex<bool>,
    wake: Condvar,
    draw_lock: Mutex<()>,
    drawer: OnceLock<Drawer>,
    hold: Arc<Hold>,
    pause_missed: Mutex<bool>,
}

impl Default for Views {
    fn default() -> Self {
        Views {
            latest: Mutex::new(Latest {
                view: StatusView::without_status(NoStatusService::Connecting),
                status: None,
                mutation_mark: 0,
                poll_number: None,
            }),
            woken: Mutex::new(false),
            wake: Condvar::new(),
            draw_lock: Mutex::new(()),
            drawer: OnceLock::new(),
            hold: Arc::default(),
            pause_missed: Mutex::new(false),
        }
    }
}

impl Views {
    fn latest(&self) -> std::sync::MutexGuard<'_, Latest> {
        self.latest.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The view of this moment, with the app's hold.
    pub fn view(&self) -> StatusView {
        let view = self.latest().view.clone();
        self.marked(view)
    }

    /// `view` with the app's hold, as every view that the app hands to the page or the tray.
    pub fn marked(&self, view: StatusView) -> StatusView {
        self.hold.mark(view)
    }

    /// The app's hold on capture after a child ended itself with status 5.
    pub fn hold(&self) -> &Arc<Hold> {
        &self.hold
    }

    /// Whether the last answer of a person's Pause or Resume was that of a Pause that did not
    /// happen.
    pub fn pause_missed(&self) -> bool {
        *self.pause_missed.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keeps whether the answer of a person's Pause or Resume was that of a Pause that did not
    /// happen, until the next such answer.
    pub fn set_pause_missed(&self, missed: bool) {
        *self.pause_missed.lock().unwrap_or_else(|e| e.into_inner()) = missed;
    }

    pub fn status(&self) -> Option<DaemonStatus> {
        self.latest().status.clone()
    }

    pub fn take_poll(&self, number: u64, answer: &Result<DaemonStatus, ClientError>) -> PollAnswer {
        let mut latest = self.latest();
        if number < latest.mutation_mark {
            return PollAnswer::DroppedAsOlderThanTheLastMutation;
        }
        latest.view = view_of(answer);
        latest.status = answer.as_ref().ok().cloned();
        latest.poll_number = Some(latest.poll_number.map_or(number, |n| n.max(number)));
        PollAnswer::Stands
    }

    pub fn take_mutation(&self, call: u64, next_number_at_answer: u64, status: &DaemonStatus) {
        self.take_answer(
            call,
            next_number_at_answer,
            StatusView::of(status),
            Some(status.clone()),
        );
    }

    /// Every answer of a mutation that its command returns as Ok, a no-status view among them,
    /// with the number of its own call: the mark moves; an answer whose call was made before the
    /// newest answer taken, that is whose number is below the mark as it stood before this
    /// answer, moves nothing in the view (ruling AP-19 H1); otherwise the view is replaced unless
    /// a poll made after the answer stands.
    pub fn take_answer(
        &self,
        call: u64,
        next_number_at_answer: u64,
        view: StatusView,
        status: Option<DaemonStatus>,
    ) {
        let mut latest = self.latest();
        let mark_before = latest.mutation_mark;
        latest.mutation_mark = latest.mutation_mark.max(next_number_at_answer);
        if call < mark_before {
            return;
        }
        if latest
            .poll_number
            .is_some_and(|n| n >= next_number_at_answer)
        {
            return;
        }
        latest.view = view;
        latest.status = status;
    }

    /// An answer that its command returns as Err leaves the view and moves the mark, so that no
    /// poll whose call was made before it is taken.
    pub fn take_refusal(&self, next_number_at_answer: u64) {
        let mut latest = self.latest();
        latest.mutation_mark = latest.mutation_mark.max(next_number_at_answer);
    }

    /// The answer of a delete: the view becomes Connecting with no store and no sizes on the
    /// platform of the view before, unless a poll made after the answer stands; the mark moves,
    /// and the poller is woken for a poll made after the answer. The delete's own call is not
    /// held against the mark: a delete's answer always takes the view to Connecting, which names
    /// no store, so that no answer taken before it can leave the store of before standing
    /// (ruling AP-19 H1).
    pub fn take_delete(&self, _call: u64, next_number_at_answer: u64) {
        {
            let mut latest = self.latest();
            let platform = match &latest.view {
                StatusView::Running(view) => view.platform,
                StatusView::NoStatus(view) => view.platform,
            };
            latest.mutation_mark = latest.mutation_mark.max(next_number_at_answer);
            if !latest
                .poll_number
                .is_some_and(|n| n >= next_number_at_answer)
            {
                latest.view = StatusView::no_status(NoStatusService::Connecting, platform);
                latest.status = None;
            }
        }
        *self.woken.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.notify_all();
    }

    pub fn set_drawer(&self, drawer: Drawer) {
        let _ = self.drawer.set(drawer);
    }

    /// Draws the view of this moment with the app's drawer, under the one draw lock. Never on
    /// the main thread: the tray's calls hand their work to it and wait.
    pub fn draw_now(&self) {
        if let Some(drawer) = self.drawer.get() {
            draw(self, &*drawer.refresh, &drawer.modules);
        }
    }

    /// Sleeps until `period` has passed or a delete's answer wakes the poller.
    pub fn sleep(&self, period: Duration) {
        let woken = self.woken.lock().unwrap_or_else(|e| e.into_inner());
        let (mut woken, _) = self
            .wake
            .wait_timeout_while(woken, period, |woken| !*woken)
            .unwrap_or_else(|e| e.into_inner());
        *woken = false;
    }
}

/// An answer of a mutation, as its command returns it: Ok with a status, Ok with a no-status
/// view, or Err.
pub fn take_command_answer(
    views: &Views,
    call: u64,
    next_number_at_answer: u64,
    answer: &Result<DaemonStatus, ClientError>,
) {
    match answer {
        Ok(status) => views.take_mutation(call, next_number_at_answer, status),
        Err(error) => match crate::view_state::read_error(error) {
            crate::view_state::Reading {
                service,
                command_error: None,
            } => views.take_answer(
                call,
                next_number_at_answer,
                StatusView::without_status(service),
                None,
            ),
            crate::view_state::Reading {
                command_error: Some(_),
                ..
            } => views.take_refusal(next_number_at_answer),
        },
    }
}

pub fn pass(control: &dyn Control, views: &Views, tray: Option<&TrayInstalled>) -> StatusView {
    let made = control.places().make();
    let number = made.number();
    let answer = control.status(made, tray.is_some() && !views.hold.stands());
    views.take_poll(number, &answer);
    views.view()
}

/// One draw: under the draw lock, the view of Views at this moment goes to the refresh and to
/// every module, so that the later reading draws last. The lock of Views is held only for the
/// read.
pub fn draw(views: &Views, refresh: &dyn Fn(&StatusView), modules: &[&dyn Registration]) {
    let _drawing = views.draw_lock.lock().unwrap_or_else(|e| e.into_inner());
    let view = views.view();
    refresh(&view);
    for module in modules {
        module.on_status(&view);
    }
}

/// One round of the poller: a pass, then a draw of the view of that moment.
pub fn round(control: &dyn Control, views: &Views, tray: Option<&TrayInstalled>) -> StatusView {
    let view = pass(control, views, tray);
    views.draw_now();
    view
}

pub fn start(control: Arc<dyn Control>, views: Arc<Views>, tray: Option<TrayInstalled>) {
    std::thread::spawn(move || loop {
        round(control.as_ref(), &views, tray.as_ref());
        views.sleep(POLL);
    });
}
