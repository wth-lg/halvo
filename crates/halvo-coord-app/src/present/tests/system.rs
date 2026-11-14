use super::pay;
#[cfg(windows)]
use super::REJECT;
#[cfg(windows)]
use crate::present::outcome::{from_task_dialog, TASK_ALLOW, TASK_REJECT};

#[test]
fn the_dialog_adds_the_rejecting_control_first() {
    assert_eq!(
        crate::present::system::controls(&pay()),
        ["Don't pay", "Pay"]
    );
}

#[cfg(windows)]
#[test]
fn the_task_dialog_rejects_by_default_cancels_and_closes_to_reject_and_sets_no_timer() {
    use crate::present::system::windows::{Texts, FLAGS};
    use windows::Win32::UI::Controls::{TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER};
    let card = pay();
    let texts = Texts::new(&card);
    let buttons = texts.buttons();
    let on_screen: &dyn Fn() = &|| {};
    let show = crate::present::guard::Show::new(super::arm(), on_screen);
    let config = texts.config(&buttons, &show);
    assert!({ config.pfCallback }.is_some());
    assert_eq!({ config.lpCallbackData }, &show as *const _ as isize);
    let (default, flags) = ({ config.nDefaultButton }, { config.dwFlags });
    assert_eq!(default, TASK_REJECT);
    assert_eq!(flags, FLAGS);
    assert_ne!(flags.0 & TDF_ALLOW_DIALOG_CANCELLATION.0, 0);
    assert_eq!(flags.0 & TDF_CALLBACK_TIMER.0, 0);
    assert_eq!({ config.dwCommonButtons }.0, 0);
    assert_eq!({ config.cButtons }, 2);
    let ids: Vec<i32> = buttons.iter().map(|b| b.nButtonID).collect();
    assert_eq!(ids, [TASK_REJECT, TASK_ALLOW]);
    let text = |i: usize| unsafe { { buttons[i].pszButtonText }.to_string() }.unwrap();
    assert_eq!((text(0), text(1)), ("Don't pay".into(), "Pay".into()));
    assert_eq!(
        from_task_dialog(Some(2)),
        REJECT,
        "Escape and close give IDCANCEL"
    );
}

mod seams {
    use super::super::{ALLOW, REJECT};
    use crate::present::system::{task_answer, wait_alert, Step};
    use halvo_coord::items::ConfirmAnswer;
    use std::cell::{Cell, RefCell};
    use std::sync::mpsc;

    fn alert(queued: Result<(), ()>, steps: &[Step], close: bool) -> (ConfirmAnswer, usize) {
        let (tx, rx) = mpsc::channel();
        for step in steps {
            tx.send(*step).unwrap();
        }
        let tx = if close {
            drop(tx);
            None
        } else {
            Some(tx)
        };
        let reports = Cell::new(0);
        let answer = wait_alert(queued, &rx, &|| reports.set(reports.get() + 1), &|| {});
        drop(tx);
        (answer, reports.get())
    }

    #[test]
    fn alert_the_report_then_1001_allows_after_one_report() {
        assert_eq!(
            alert(Ok(()), &[Step::Shown, Step::Response(1001)], false),
            (ALLOW, 1)
        );
    }

    #[test]
    fn alert_the_report_then_1000_rejects() {
        assert_eq!(
            alert(Ok(()), &[Step::Shown, Step::Response(1000)], false),
            (REJECT, 1)
        );
    }

    #[test]
    fn alert_the_report_then_a_closed_channel_rejects() {
        assert_eq!(alert(Ok(()), &[Step::Shown], true), (REJECT, 1));
    }

    #[test]
    fn alert_a_channel_closed_at_once_rejects_with_no_report() {
        assert_eq!(alert(Ok(()), &[], true), (REJECT, 0));
    }

    #[test]
    fn alert_a_response_with_no_report_before_it_rejects() {
        assert_eq!(alert(Ok(()), &[Step::Response(1001)], false), (REJECT, 0));
    }

    #[test]
    fn alert_a_queue_call_that_fails_rejects_with_no_report() {
        assert_eq!(
            alert(Err(()), &[Step::Shown, Step::Response(1001)], false),
            (REJECT, 0)
        );
    }

    fn order(queued: Result<(), ()>, steps: &[Step]) -> Vec<&'static str> {
        let (tx, rx) = mpsc::channel();
        for step in steps {
            tx.send(*step).unwrap();
        }
        drop(tx);
        let order = RefCell::new(Vec::new());
        wait_alert(queued, &rx, &|| order.borrow_mut().push("report"), &|| {
            order.borrow_mut().push("hold")
        });
        order.into_inner()
    }

    #[test]
    fn alert_the_hold_starts_once_the_report_has_returned_and_never_without_a_report() {
        let shown = [Step::Shown, Step::Response(1001)];
        assert_eq!(order(Ok(()), &shown), ["report", "hold"]);
        assert_eq!(order(Ok(()), &[Step::Shown]), ["report", "hold"]);
        assert!(order(Ok(()), &[Step::Response(1001)]).is_empty());
        assert!(order(Ok(()), &[]).is_empty());
        assert!(order(Err(()), &shown).is_empty());
    }

    #[test]
    fn task_dialog_a_failed_call_beside_the_allowing_id_rejects() {
        assert_eq!(task_answer(Err(()), 100), REJECT);
    }

    #[test]
    fn task_dialog_a_call_that_returned_allows_for_100_alone() {
        assert_eq!(task_answer(Ok::<(), ()>(()), 100), ALLOW);
        for pressed in [101, 2, 0] {
            assert_eq!(task_answer(Ok::<(), ()>(()), pressed), REJECT, "{pressed}");
        }
    }
}

mod moment {
    use crate::present::guard::Guard;
    use crate::present::system::notice;
    use std::time::{Duration, Instant};

    const ARM: Duration = Duration::from_millis(500);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn answers(guard: Guard, at: Instant) -> bool {
        let mut copy = guard;
        copy.press(at)
    }

    #[test]
    fn moment_the_key_window_notice_before_the_app_is_active_reports_at_the_app_notice() {
        let t0 = Instant::now();
        let mut guard = Guard::new(ARM);
        assert!(
            !notice(&mut guard, false, true, t0),
            "key, the app not active"
        );
        assert!(
            notice(&mut guard, true, true, t0 + ms(300)),
            "the app became active"
        );
        guard.reported(t0 + ms(300));
        assert!(!answers(guard, t0 + ms(799)));
        assert!(answers(guard, t0 + ms(800)));
    }

    #[test]
    fn moment_the_app_notice_before_the_window_is_key_reports_at_the_key_notice() {
        let t0 = Instant::now();
        let mut guard = Guard::new(ARM);
        assert!(
            !notice(&mut guard, true, false, t0),
            "active, the window not key"
        );
        assert!(
            notice(&mut guard, true, true, t0 + ms(300)),
            "the window became key"
        );
        guard.reported(t0 + ms(300));
        assert!(!answers(guard, t0 + ms(799)));
        assert!(answers(guard, t0 + ms(800)));
    }

    #[test]
    fn moment_no_notice_reports_while_the_app_is_not_active_or_the_window_not_key() {
        let t0 = Instant::now();
        let mut guard = Guard::new(ARM);
        for (active, key) in [(false, false), (false, true), (true, false)] {
            assert!(
                !notice(&mut guard, active, key, t0),
                "active={active} key={key}"
            );
        }
        assert!(
            !answers(guard, t0 + ms(10_000)),
            "no report, so no press answers"
        );
    }

    #[test]
    fn moment_a_later_notice_with_both_starts_the_hold_anew_and_reports_nothing() {
        let t0 = Instant::now();
        let mut guard = Guard::new(ARM);
        assert!(notice(&mut guard, true, true, t0));
        guard.reported(t0);
        assert!(
            !notice(&mut guard, false, true, t0 + ms(600)),
            "the app not active"
        );
        assert!(
            answers(guard, t0 + ms(600)),
            "a notice while not active starts nothing"
        );
        assert!(
            !notice(&mut guard, true, true, t0 + ms(700)),
            "reported once"
        );
        assert!(!answers(guard, t0 + ms(1_199)));
        assert!(answers(guard, t0 + ms(1_200)));
    }
}
