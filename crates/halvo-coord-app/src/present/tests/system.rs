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
    use crate::present::system::{task_answer, wait_alert, Release, Step};
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
    fn alert_a_report_that_panics_still_starts_the_hold_and_the_answer_follows() {
        let (tx, rx) = mpsc::channel();
        for step in [Step::Shown, Step::Response(1001)] {
            tx.send(step).unwrap();
        }
        drop(tx);
        let order = RefCell::new(Vec::new());
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            wait_alert(
                Ok::<(), ()>(()),
                &rx,
                &|| {
                    order.borrow_mut().push("report");
                    panic!("the report on screen panicked");
                },
                &|| order.borrow_mut().push("hold"),
            )
        }));
        assert_eq!(answer.ok(), Some(ALLOW), "the show goes on to its answer");
        assert_eq!(order.into_inner(), ["report", "hold"]);
    }

    #[test]
    fn alert_the_release_runs_once_on_every_way_out_of_the_show_a_panic_among_them() {
        let released = Cell::new(0);
        {
            let _release = Release::new(|| released.set(released.get() + 1));
            assert_eq!(released.get(), 0, "not before the show ends");
        }
        assert_eq!(released.get(), 1, "a show that returns");
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _release = Release::new(|| released.set(released.get() + 1));
            assert_eq!(released.get(), 1, "not before the show unwinds");
            panic!("a panic inside the show");
        }));
        assert!(unwound.is_err());
        assert_eq!(released.get(), 2, "a show that unwinds from a panic");
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
