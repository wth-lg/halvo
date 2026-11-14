mod support;

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::mem::Call;
use halvo_coord::types::{Action, Cause, Class, EffectAnswer, ItemId, Limits, RunEvent, YesNo};
use halvo_coord_app::{Card, Core, Dialog, DialogPresenter, Hub, Reply};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::{agent, ask, calls_since, mark, started, world, Counts, Shared, DAY, T0};

const WAIT: Duration = Duration::from_secs(10);
const QUIET: Duration = Duration::from_millis(300);
const LAST_SHOW: usize = 5;

struct Chosen {
    shows: AtomicUsize,
    shown: Mutex<Sender<usize>>,
    press: Mutex<Receiver<ConfirmAnswer>>,
}

impl Dialog for Chosen {
    fn show(&self, _: &Card, on_screen: &dyn Fn()) -> ConfirmAnswer {
        let n = self.shows.fetch_add(1, Ordering::SeqCst) + 1;
        if n > LAST_SHOW {
            return ConfirmAnswer::Reject;
        }
        on_screen();
        let _ = self.shown.lock().unwrap().send(n);
        self.press
            .lock()
            .unwrap()
            .recv_timeout(WAIT)
            .unwrap_or(ConfirmAnswer::Reject)
    }
}

struct Rig {
    hub: Arc<Hub<Shared>>,
    item: String,
    shown: Receiver<usize>,
    press: Sender<ConfirmAnswer>,
    dialog: Arc<Chosen>,
    from: usize,
}

fn rig() -> Rig {
    let (shown_tx, shown) = channel();
    let (press, press_rx) = channel();
    let dialog = Arc::new(Chosen {
        shows: AtomicUsize::new(0),
        shown: Mutex::new(shown_tx),
        press: Mutex::new(press_rx),
    });
    let presenter = DialogPresenter::new(dialog.clone(), |_: &ItemId| {
        Card::new(&[Class::Payment], &[])
    });
    let hub = Hub::new(
        world(1, Limits::PRODUCT),
        Limits::PRODUCT,
        Counts::default().sink(),
        Box::new(presenter),
    );
    let c = hub.coordinator();
    Core::tick(&*hub);
    c.control(&agent(0), Action::RunNow).unwrap();
    let run = started(&hub).pop().unwrap();
    c.on_run(&run, RunEvent::Hello).unwrap();
    c.on_run(&run, ask(&run, 0, Class::Payment)).unwrap();
    let items = c.snapshot().items;
    assert_eq!(items.len(), 1, "{items:?}");
    let item = items[0].item().to_string();
    let from = mark(&hub);
    Rig {
        hub,
        item,
        shown,
        press,
        dialog,
        from,
    }
}

fn until(limit: Duration, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

impl Rig {
    fn present(&self) -> Reply {
        self.hub
            .call("agents_present", Some(&json!({ "item": self.item })))
    }

    fn shows(&self) -> usize {
        self.dialog.shows.load(Ordering::SeqCst)
    }

    fn clock(&self, at: u64) {
        self.hub
            .coordinator()
            .ports()
            .inner()
            .with(|p| p.set_clock(T0 + at, DAY));
        self.hub.coordinator().tick();
    }

    fn armed(&self) -> bool {
        self.hub
            .coordinator()
            .snapshot()
            .items
            .iter()
            .any(|i| i.armed() == YesNo::Yes)
    }

    fn sent(&self, answer: EffectAnswer) -> usize {
        calls_since(&self.hub, self.from)
            .iter()
            .filter(|c| match c {
                Call::AnswerConfirm(_, answers, _) => answers.iter().any(|a| a.answer == answer),
                _ => false,
            })
            .count()
    }

    fn thread_ended(&self) -> bool {
        until(WAIT, || Arc::strong_count(&self.dialog) == 2)
    }
}

#[test]
fn agents_present_shows_the_dialog_once() {
    let rig = rig();
    assert_eq!(rig.present(), Reply::Ok);
    assert_eq!(rig.shown.recv_timeout(WAIT), Ok(1), "the dialog was shown");
    assert!(rig.shown.recv_timeout(QUIET).is_err(), "and once");
    assert_eq!(rig.shows(), 1);
    drop(rig.press);
}

#[test]
fn the_report_on_screen_reaches_the_item_so_that_it_arms() {
    let rig = rig();
    assert!(!rig.armed());
    assert_eq!(rig.present(), Reply::Ok);
    assert_eq!(rig.shown.recv_timeout(WAIT), Ok(1));
    rig.clock(Limits::PRODUCT.arm_ms);
    assert!(
        rig.armed(),
        "the dialog reported itself on screen and a tick came after arm_ms"
    );
    drop(rig.press);
}

#[test]
fn a_press_on_the_rejecting_control_sends_no_allow_once() {
    let rig = rig();
    assert_eq!(rig.present(), Reply::Ok);
    assert_eq!(rig.shown.recv_timeout(WAIT), Ok(1));
    rig.clock(Limits::PRODUCT.arm_ms);
    rig.press.send(ConfirmAnswer::Reject).unwrap();
    assert!(
        rig.thread_ended(),
        "the thread ends after the counted answer"
    );
    assert_eq!(rig.sent(EffectAnswer::AllowOnce), 0);
    assert_eq!(rig.sent(EffectAnswer::RejectOnce), 1);
    assert!(rig.hub.coordinator().snapshot().items.is_empty());
}

#[test]
fn an_early_allow_is_refused_shown_once_more_and_only_the_press_of_the_second_show_counts() {
    let rig = rig();
    assert_eq!(rig.present(), Reply::Ok);
    assert_eq!(rig.shown.recv_timeout(WAIT), Ok(1));
    rig.press.send(ConfirmAnswer::Allow).unwrap();
    assert_eq!(
        rig.shown.recv_timeout(WAIT),
        Ok(2),
        "early: shown once more"
    );
    rig.clock(Limits::PRODUCT.arm_ms);
    assert_eq!(
        rig.sent(EffectAnswer::AllowOnce),
        0,
        "the early allow is not handed in again after the tick"
    );
    rig.press.send(ConfirmAnswer::Allow).unwrap();
    assert!(
        rig.thread_ended(),
        "the thread ends after the counted answer"
    );
    assert_eq!(rig.sent(EffectAnswer::AllowOnce), 1);
    assert_eq!(rig.shows(), 2);
}

#[test]
fn a_second_agents_present_while_the_thread_holds_the_item_shows_no_second_dialog() {
    let rig = rig();
    assert_eq!(rig.present(), Reply::Ok);
    assert_eq!(rig.shown.recv_timeout(WAIT), Ok(1));
    for _ in 0..2 {
        assert_eq!(rig.present(), Reply::Ok);
    }
    assert!(rig.shown.recv_timeout(QUIET).is_err(), "no second dialog");
    assert_eq!(rig.shows(), 1);
    drop(rig.press);
}

#[test]
fn the_pages_allow_for_the_native_item_answers_presenter_while_the_dialog_is_open() {
    let rig = rig();
    assert_eq!(rig.present(), Reply::Ok);
    assert_eq!(rig.shown.recv_timeout(WAIT), Ok(1));
    rig.clock(Limits::PRODUCT.arm_ms);
    let allow = json!({ "item": rig.item, "choice": "allow" });
    assert_eq!(
        rig.hub.call("agents_answer", Some(&allow)),
        Reply::Error(Cause::Presenter)
    );
    assert_eq!(rig.sent(EffectAnswer::AllowOnce), 0);
    drop(rig.press);
}

#[test]
fn a_press_on_allow_after_a_lapse_at_the_wait_limit_sends_no_allow_once() {
    let rig = rig();
    assert_eq!(rig.present(), Reply::Ok);
    assert_eq!(rig.shown.recv_timeout(WAIT), Ok(1));
    rig.clock(Limits::PRODUCT.wait_limit_s * 1000);
    assert!(rig.hub.coordinator().snapshot().items.is_empty(), "lapsed");
    rig.press.send(ConfirmAnswer::Allow).unwrap();
    assert!(
        rig.thread_ended(),
        "a refused answer that is not early ends the thread"
    );
    assert!(
        rig.shown.recv_timeout(QUIET).is_err(),
        "no show after the lapse"
    );
    assert_eq!(rig.sent(EffectAnswer::AllowOnce), 0);
}

#[test]
fn a_press_on_allow_after_the_quit_sends_no_allow_once() {
    let rig = rig();
    assert_eq!(rig.present(), Reply::Ok);
    assert_eq!(rig.shown.recv_timeout(WAIT), Ok(1));
    rig.clock(Limits::PRODUCT.arm_ms);
    let hub = rig.hub.clone();
    let (quit_tx, quit) = channel();
    std::thread::spawn(move || {
        hub.quit(Some(Duration::from_millis(200)));
        let _ = quit_tx.send(());
    });
    quit.recv_timeout(WAIT)
        .expect("the quit returned while the dialog was open");
    rig.press.send(ConfirmAnswer::Allow).unwrap();
    assert!(
        rig.thread_ended(),
        "a refused answer that is not early ends the thread"
    );
    assert!(
        rig.shown.recv_timeout(QUIET).is_err(),
        "no show after the quit"
    );
    assert_eq!(rig.sent(EffectAnswer::AllowOnce), 0);
}
