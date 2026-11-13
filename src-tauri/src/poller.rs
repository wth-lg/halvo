use std::sync::{Arc, Mutex};
use std::time::Duration;

use halvo_control::client::ClientError;
use halvo_control::proto::DaemonStatus;

use crate::commands::control::Control;
use crate::commands::Registration;
use crate::tray::TrayInstalled;
use crate::view_state::{view_of, Service, StatusView};

/// The period of the poller's passes (contracts.md C10.2, `halvo_control::POLL_MS`).
pub const POLL: Duration = Duration::from_millis(halvo_control::POLL_MS);

#[derive(Debug)]
struct Latest {
    view: StatusView,
    status: Option<DaemonStatus>,
    mutation_mark: u64,
}

/// The newest view of the capture service, which the poller, the commands and the tray share.
/// A status that a call made before the last mutation was answered never replaces the
/// mutation's view.
#[derive(Debug)]
pub struct Views {
    latest: Mutex<Latest>,
}

impl Default for Views {
    fn default() -> Self {
        Views {
            latest: Mutex::new(Latest {
                view: StatusView::without_status(Service::Connecting),
                status: None,
                mutation_mark: 0,
            }),
        }
    }
}

impl Views {
    fn latest(&self) -> std::sync::MutexGuard<'_, Latest> {
        self.latest.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn view(&self) -> StatusView {
        self.latest().view.clone()
    }

    pub fn status(&self) -> Option<DaemonStatus> {
        self.latest().status.clone()
    }

    /// Takes the answer of a status request made as call `number`; returns whether it stands.
    pub fn take_poll(&self, number: u64, answer: &Result<DaemonStatus, ClientError>) -> bool {
        let mut latest = self.latest();
        if number < latest.mutation_mark {
            return false;
        }
        latest.view = view_of(answer);
        latest.status = answer.as_ref().ok().cloned();
        true
    }

    /// Takes the status of a mutation's reply, answered when the next call made would take
    /// the number `mark`.
    pub fn take_mutation(&self, mark: u64, status: &DaemonStatus) {
        let mut latest = self.latest();
        latest.mutation_mark = latest.mutation_mark.max(mark);
        latest.view = StatusView::of(status);
        latest.status = Some(status.clone());
    }
}

/// One pass of the poller: one status request, claiming the indicator only with the proof of
/// an installed tray; the view stands unless a mutation answered after the request was made.
/// Returns the view that stands after the pass.
pub fn pass(control: &dyn Control, views: &Views, tray: Option<&TrayInstalled>) -> StatusView {
    let made = control.places().make();
    let number = made.number();
    let answer = control.status(made, tray.is_some());
    views.take_poll(number, &answer);
    views.view()
}

/// A pass, then `on_status` with its view for every module of `modules`, also when the
/// pass found no status.
pub fn pass_and_tell(
    control: &dyn Control,
    views: &Views,
    tray: Option<&TrayInstalled>,
    modules: &[&dyn Registration],
) -> StatusView {
    let view = pass(control, views, tray);
    for module in modules {
        module.on_status(&view);
    }
    view
}

/// Runs a pass every `POLL` for as long as the process runs, tells `modules` of each view and
/// hands it to `after`.
pub fn start(
    control: Arc<dyn Control>,
    views: Arc<Views>,
    tray: Option<TrayInstalled>,
    modules: Vec<&'static dyn Registration>,
    after: impl Fn(&StatusView) + Send + 'static,
) {
    std::thread::spawn(move || loop {
        let view = pass_and_tell(control.as_ref(), &views, tray.as_ref(), &modules);
        after(&view);
        std::thread::sleep(POLL);
    });
}
