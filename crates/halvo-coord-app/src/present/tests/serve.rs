use super::{item, pay, ALLOW, REJECT};
use crate::present::{serve, Card, Dialog, DialogPresenter, Read, Target};
use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::{Cause, ItemId};
use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Seen {
    on_screen: usize,
    answers: Vec<ConfirmAnswer>,
    dropped: bool,
}

struct Fake {
    item: ItemId,
    replies: Mutex<VecDeque<Result<(), Cause>>>,
    seen: Arc<Mutex<Seen>>,
}

impl Fake {
    fn new(replies: &[Result<(), Cause>]) -> (Fake, Arc<Mutex<Seen>>) {
        let seen = Arc::new(Mutex::new(Seen::default()));
        let fake = Fake {
            item: item(),
            replies: Mutex::new(replies.iter().copied().collect()),
            seen: seen.clone(),
        };
        (fake, seen)
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .dropped = true;
    }
}

impl Target for Fake {
    fn item(&self) -> &ItemId {
        &self.item
    }

    fn on_screen(&self) {
        self.seen.lock().unwrap().on_screen += 1;
    }

    fn answer(&self, answer: ConfirmAnswer) -> Result<(), Cause> {
        self.seen.lock().unwrap().answers.push(answer);
        self.replies.lock().unwrap().pop_front().unwrap_or(Ok(()))
    }
}

struct Shows {
    shows: Mutex<VecDeque<(bool, ConfirmAnswer)>>,
    cards: Mutex<Vec<Card>>,
}

impl Shows {
    fn new(shows: &[(bool, ConfirmAnswer)]) -> Shows {
        Shows {
            shows: Mutex::new(shows.iter().copied().collect()),
            cards: Mutex::new(Vec::new()),
        }
    }

    fn count(&self) -> usize {
        self.cards.lock().unwrap().len()
    }
}

impl Dialog for Shows {
    fn show(&self, card: &Card, on_screen: &dyn Fn()) -> ConfirmAnswer {
        self.cards.lock().unwrap().push(card.clone());
        let (shown, answer) = self.shows.lock().unwrap().pop_front().unwrap();
        if shown {
            on_screen();
        }
        answer
    }
}

fn reads_pay() -> Box<Read> {
    Box::new(|i: &ItemId| {
        assert_eq!(i, &item());
        Ok(pay())
    })
}

#[test]
fn a_reader_that_refuses_the_card_shows_nothing_and_answers_nothing() {
    let (fake, seen) = Fake::new(&[]);
    let shows = Shows::new(&[(true, ALLOW)]);
    let read: Box<Read> = Box::new(|_: &ItemId| Err(Cause::UnknownItem));
    serve(&fake, &*read, &shows);
    assert_eq!(shows.count(), 0);
    let seen = seen.lock().unwrap();
    assert_eq!((seen.on_screen, seen.answers.len()), (0, 0));
}

#[test]
fn the_press_on_allow_reaches_the_item_as_allow_after_the_report_on_screen() {
    let (fake, seen) = Fake::new(&[Ok(())]);
    let shows = Shows::new(&[(true, ALLOW)]);
    serve(&fake, &*reads_pay(), &shows);
    assert_eq!(*shows.cards.lock().unwrap(), [pay()]);
    let seen = seen.lock().unwrap();
    assert_eq!((seen.on_screen, seen.answers.clone()), (1, vec![ALLOW]));
}

#[test]
fn a_reject_reaches_the_item_as_reject() {
    let (fake, seen) = Fake::new(&[Ok(())]);
    let shows = Shows::new(&[(true, REJECT)]);
    serve(&fake, &*reads_pay(), &shows);
    assert_eq!(seen.lock().unwrap().answers, [REJECT]);
}

#[test]
fn an_early_answer_shows_the_dialog_once_more_and_the_next_answer_counts() {
    let (fake, seen) = Fake::new(&[Err(Cause::Early), Err(Cause::Early), Ok(())]);
    let shows = Shows::new(&[(true, ALLOW), (true, REJECT), (true, ALLOW)]);
    serve(&fake, &*reads_pay(), &shows);
    assert_eq!(shows.count(), 3);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.on_screen, 3);
    assert_eq!(seen.answers, [ALLOW, REJECT, ALLOW]);
}

#[test]
fn an_early_answer_of_a_dialog_that_was_not_on_screen_shows_it_no_more() {
    let (fake, seen) = Fake::new(&[Err(Cause::Early)]);
    let shows = Shows::new(&[(false, REJECT), (true, ALLOW)]);
    serve(&fake, &*reads_pay(), &shows);
    assert_eq!(shows.count(), 1);
    let seen = seen.lock().unwrap();
    assert_eq!((seen.on_screen, seen.answers.clone()), (0, vec![REJECT]));
}

#[test]
fn an_answer_refused_for_another_cause_shows_the_dialog_no_more() {
    for cause in [
        Cause::Stale,
        Cause::UnknownItem,
        Cause::State,
        Cause::NoService,
    ] {
        let (fake, seen) = Fake::new(&[Err(cause)]);
        let shows = Shows::new(&[(true, ALLOW), (true, ALLOW)]);
        serve(&fake, &*reads_pay(), &shows);
        assert_eq!(shows.count(), 1, "{cause:?}");
        assert_eq!(seen.lock().unwrap().answers, [ALLOW]);
    }
}

struct Held {
    started: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<ConfirmAnswer>>,
}

impl Dialog for Held {
    fn show(&self, _: &Card, on_screen: &dyn Fn()) -> ConfirmAnswer {
        on_screen();
        self.started.lock().unwrap().send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or(REJECT)
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

#[test]
fn present_returns_at_once_and_drops_the_item_on_its_own_thread_when_the_dialog_closes() {
    let (started_tx, started) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let dialog = Arc::new(Held {
        started: Mutex::new(started_tx),
        release: Mutex::new(release_rx),
    });
    let presenter = DialogPresenter::new(dialog, |_: &ItemId| Ok(pay()));
    let (fake, seen) = Fake::new(&[Ok(())]);
    let (returned_tx, returned) = mpsc::channel();
    std::thread::spawn(move || {
        presenter.spawn(fake);
        let _ = returned_tx.send(());
    });
    returned
        .recv_timeout(Duration::from_secs(5))
        .expect("present has not returned within 5 s: it serves the dialog on the caller's thread");
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(!seen.lock().unwrap().dropped, "open while the dialog is");
    release.send(ALLOW).unwrap();
    assert!(until(Duration::from_secs(10), || seen
        .lock()
        .unwrap()
        .dropped));
    assert_eq!(seen.lock().unwrap().answers, [ALLOW]);
}

#[test]
fn an_allow_from_a_show_that_was_never_on_screen_reaches_the_item_as_reject() {
    let (fake, seen) = Fake::new(&[Ok(())]);
    let shows = Shows::new(&[(false, ALLOW)]);
    serve(&fake, &*reads_pay(), &shows);
    let seen = seen.lock().unwrap();
    assert_eq!((seen.on_screen, seen.answers.clone()), (0, vec![REJECT]));
}

#[test]
fn the_mark_on_screen_is_one_per_show_so_a_second_show_never_on_screen_hands_on_reject() {
    let (fake, seen) = Fake::new(&[Err(Cause::Early), Ok(())]);
    let shows = Shows::new(&[(true, ALLOW), (false, ALLOW)]);
    serve(&fake, &*reads_pay(), &shows);
    assert_eq!(shows.count(), 2);
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.on_screen, 1,
        "one report: the first show alone was on screen"
    );
    assert_eq!(
        seen.answers,
        [ALLOW, REJECT],
        "the second show was never on screen, so its allow is handed on as reject"
    );
}
