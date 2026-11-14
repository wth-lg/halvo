use std::sync::{Arc, Mutex};
use std::time::Duration;

use halvo_control::client::ClientError;
use halvo_control::proto::DaemonStatus;

use crate::commands::control::Control;
use crate::commands::Registration;
use crate::tray::TrayInstalled;
use crate::view_state::{view_of, Service, StatusView};

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
                poll_number: None,
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

    pub fn take_mutation(&self, next_number_at_answer: u64, status: &DaemonStatus) {
        let mut latest = self.latest();
        latest.mutation_mark = latest.mutation_mark.max(next_number_at_answer);
        if latest
            .poll_number
            .is_some_and(|n| n >= next_number_at_answer)
        {
            return;
        }
        latest.view = StatusView::of(status);
        latest.status = Some(status.clone());
    }
}

pub fn pass(control: &dyn Control, views: &Views, tray: Option<&TrayInstalled>) -> StatusView {
    let made = control.places().make();
    let number = made.number();
    let answer = control.status(made, tray.is_some());
    views.take_poll(number, &answer);
    views.view()
}

pub fn round(
    control: &dyn Control,
    views: &Views,
    tray: Option<&TrayInstalled>,
    modules: &[&dyn Registration],
    refresh: &dyn Fn(&StatusView),
) -> StatusView {
    let view = pass(control, views, tray);
    refresh(&view);
    for module in modules {
        module.on_status(&view);
    }
    view
}

pub fn start(
    control: Arc<dyn Control>,
    views: Arc<Views>,
    tray: Option<TrayInstalled>,
    modules: Vec<&'static dyn Registration>,
    after: impl Fn(&StatusView) + Send + 'static,
) {
    std::thread::spawn(move || loop {
        round(control.as_ref(), &views, tray.as_ref(), &modules, &after);
        std::thread::sleep(POLL);
    });
}
