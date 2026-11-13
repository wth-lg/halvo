mod support;

use halvo_coord::types::{Action, Limits, RunEvent};
use halvo_coord_app::{on_window_event, Close, Core, Hub, Waits, CLOSE};
use serde_json::{json, Value};
use std::sync::Arc;
use std::thread::{sleep, JoinHandle};
use std::time::{Duration, Instant};
use support::{agent, app, fixture, invoke, started, world, Counts, Dialogs, Shared};
use tauri::test::MockRuntime;
use tauri::{WebviewWindow, WindowEvent};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn hub(agents: usize, next: u64, coalesce: u64) -> Arc<Hub<Shared>> {
    let waits = Waits {
        next: ms(next),
        coalesce: ms(coalesce),
    };
    let hub = Hub::with_waits(
        world(agents, Limits::PRODUCT),
        Limits::PRODUCT,
        Counts::default().sink(),
        Dialogs::default().boxed(),
        waits,
    );
    hub.coordinator().tick();
    hub
}

fn wait_next(main: &WebviewWindow<MockRuntime>, after: u32) -> JoinHandle<(String, Duration)> {
    let main = main.clone();
    std::thread::spawn(move || {
        let start = Instant::now();
        let reply = invoke(&main, "agents_next", json!({ "after": after })).unwrap();
        (reply, start.elapsed())
    })
}

fn seq_of(reply: &str) -> u64 {
    serde_json::from_str::<Value>(reply).unwrap()["seq"]
        .as_u64()
        .unwrap()
}

#[test]
fn the_waits_of_the_product_are_next_wait_s_and_coalesce_ms() {
    let waits = Waits::of(&Limits::PRODUCT);
    assert_eq!(waits.next, Duration::from_secs(Limits::PRODUCT.next_wait_s));
    assert_eq!(waits.coalesce, ms(Limits::PRODUCT.coalesce_ms));
    assert_eq!(
        (waits.next, waits.coalesce),
        (Duration::from_secs(25), ms(250))
    );
}

#[test]
fn a_next_whose_seq_is_old_answers_the_whole_state_at_once() {
    let hub = hub(1, 10_000, 100);
    let (_app, main, _other) = app(hub.clone());
    let (reply, took) = wait_next(&main, 0).join().unwrap();
    assert!(took < ms(5_000), "{took:?}");
    assert_eq!(
        reply,
        serde_json::to_string(&hub.coordinator().snapshot()).unwrap()
    );
}

#[test]
fn a_waiting_next_answers_the_whole_state_a_coalesce_after_the_change() {
    let hub = hub(1, 10_000, 300);
    let (_app, main, _other) = app(hub.clone());
    let seq = hub.coordinator().snapshot().seq;
    let waiting = wait_next(&main, seq);
    sleep(ms(200));
    let changed = Instant::now();
    hub.coordinator()
        .control(&agent(0), Action::RunNow)
        .unwrap();
    let (reply, took) = waiting.join().unwrap();
    assert!(changed.elapsed() >= ms(300), "{:?}", changed.elapsed());
    assert!(took < ms(5_000), "{took:?}");
    assert!(seq_of(&reply) > u64::from(seq));
    assert_eq!(
        reply,
        serde_json::to_string(&hub.coordinator().snapshot()).unwrap()
    );
}

#[test]
fn changes_inside_coalesce_ms_reach_the_page_as_one_reply() {
    let hub = hub(1, 10_000, 600);
    let (_app, main, _other) = app(hub.clone());
    let seq = hub.coordinator().snapshot().seq;
    let waiting = wait_next(&main, seq);
    sleep(ms(150));
    hub.coordinator()
        .control(&agent(0), Action::RunNow)
        .unwrap();
    let run = started(&hub).pop().unwrap();
    let after_first = hub.coordinator().snapshot().seq;
    sleep(ms(150));
    hub.coordinator().on_run(&run, RunEvent::Hello).unwrap();
    hub.coordinator().control(&agent(0), Action::Stop).unwrap();
    let last = hub.coordinator().snapshot().seq;
    assert!(last > after_first, "the second change raised seq");
    let (reply, _) = waiting.join().unwrap();
    assert_eq!(
        seq_of(&reply),
        u64::from(last),
        "one reply holds both changes"
    );
}

#[test]
fn two_replies_that_carry_a_state_are_coalesce_ms_apart() {
    let hub = hub(1, 10_000, 400);
    let (_app, main, _other) = app(hub.clone());
    let (first, _) = wait_next(&main, 0).join().unwrap();
    let at = Instant::now();
    hub.coordinator()
        .control(&agent(0), Action::RunNow)
        .unwrap();
    let (second, _) = wait_next(&main, seq_of(&first) as u32).join().unwrap();
    assert!(at.elapsed() >= ms(380), "{:?}", at.elapsed());
    assert!(seq_of(&second) > seq_of(&first));
}

#[test]
fn a_next_with_no_change_answers_the_old_seq_with_empty_lists_after_its_wait() {
    let hub = hub(0, 300, 50);
    let (_app, main, _other) = app(hub.clone());
    let (reply, took) = wait_next(&main, 1).join().unwrap();
    assert!(took >= ms(300), "{took:?}");
    assert_eq!(reply, fixture("reply-next-same"));
}

#[test]
fn a_waiting_next_answers_closed_at_a_hide_and_at_once_while_hidden() {
    let hub = hub(1, 10_000, 50);
    let (_app, main, _other) = app(hub.clone());
    let seq = hub.coordinator().snapshot().seq;
    let waiting = wait_next(&main, seq);
    sleep(ms(200));
    main.hide().unwrap();
    hub.hidden();
    let (reply, took) = waiting.join().unwrap();
    assert_eq!(reply, fixture("reply-closed"));
    assert!(took < ms(5_000), "{took:?}");
    let (reply, took) = wait_next(&main, seq).join().unwrap();
    assert_eq!(reply, fixture("reply-closed"), "a call while hidden");
    assert!(took < ms(5_000), "{took:?}");
    let (reply, _) = wait_next(&main, 0).join().unwrap();
    assert_eq!(reply, fixture("reply-closed"), "even with a newer state");
    hub.shown();
    let (reply, _) = wait_next(&main, 0).join().unwrap();
    assert_eq!(
        seq_of(&reply),
        u64::from(seq),
        "shown again, it answers the state"
    );
}

#[test]
fn a_waiting_next_answers_closed_when_the_window_main_is_destroyed() {
    let hub = hub(1, 10_000, 50);
    let (_app, main, other) = app(hub.clone());
    let seq = hub.coordinator().snapshot().seq;
    let waiting = wait_next(&main, seq);
    sleep(ms(200));
    on_window_event(&other.as_ref().window(), &WindowEvent::Destroyed);
    on_window_event(&main.as_ref().window(), &WindowEvent::Focused(false));
    sleep(ms(200));
    assert!(
        !waiting.is_finished(),
        "another window or event closes nothing"
    );
    let window = main.as_ref().window();
    main.destroy().unwrap();
    on_window_event(&window, &WindowEvent::Destroyed);
    let (reply, took) = waiting.join().unwrap();
    assert_eq!(reply, fixture("reply-closed"));
    assert!(took < ms(5_000), "{took:?}");
}

#[cfg(target_os = "macos")]
#[test]
fn on_macos_u02s_verdict_hides_the_window() {
    assert_eq!(CLOSE, Close::Hide);
}

#[cfg(target_os = "windows")]
#[test]
fn on_windows_u02s_verdict_destroys_the_window() {
    assert_eq!(CLOSE, Close::Destroy);
}

#[test]
fn the_form_of_this_systems_verdict_ends_the_waiting_call() {
    let hub = hub(1, 10_000, 50);
    let (_app, main, _other) = app(hub.clone());
    let seq = hub.coordinator().snapshot().seq;
    let waiting = wait_next(&main, seq);
    sleep(ms(200));
    match CLOSE {
        Close::Hide => {
            main.hide().unwrap();
            hub.hidden();
        }
        Close::Destroy => {
            let window = main.as_ref().window();
            main.destroy().unwrap();
            on_window_event(&window, &WindowEvent::Destroyed);
        }
    }
    assert_eq!(waiting.join().unwrap().0, fixture("reply-closed"));
}

#[test]
fn a_hide_and_a_show_before_the_waiting_call_wakes_still_end_it_with_closed() {
    let hub = hub(1, 10_000, 50);
    let (_app, main, _other) = app(hub.clone());
    let seq = hub.coordinator().snapshot().seq;
    let waiting = wait_next(&main, seq);
    sleep(ms(200));
    hub.hidden();
    hub.shown();
    let (reply, took) = waiting.join().unwrap();
    assert_eq!(reply, fixture("reply-closed"));
    assert!(took < ms(5_000), "{took:?}");
}

#[test]
fn a_next_whose_seq_is_old_answers_well_inside_one_coalesce() {
    let hub = hub(1, 4_000, 2_000);
    let (_app, main, _other) = app(hub.clone());
    let (reply, took) = wait_next(&main, 0).join().unwrap();
    assert!(
        took < ms(1_000),
        "at once, not a coalesce of 2 s later: {took:?}"
    );
    assert_eq!(seq_of(&reply), u64::from(hub.coordinator().snapshot().seq));
}

#[test]
fn a_waiting_next_answers_one_coalesce_after_the_change_and_not_later() {
    let hub = hub(1, 4_000, 2_000);
    let (_app, main, _other) = app(hub.clone());
    let seq = hub.coordinator().snapshot().seq;
    let waiting = wait_next(&main, seq);
    sleep(ms(300));
    let changed = Instant::now();
    hub.coordinator()
        .control(&agent(0), Action::RunNow)
        .unwrap();
    let (reply, _) = waiting.join().unwrap();
    let after = changed.elapsed();
    assert!(
        after >= ms(1_000) && after < ms(3_000),
        "point 1 of draft-rulings-u41-gates.md waits for a ruling: as built, a call that waits \
         answers coalesce_ms (2 s here) after the first change that it sees; it answered {after:?} \
         after the change"
    );
    assert!(seq_of(&reply) > u64::from(seq));
}

#[test]
fn a_next_with_no_change_answers_after_next_wait_and_not_later() {
    let hub = hub(0, 4_000, 2_000);
    let (_app, main, _other) = app(hub.clone());
    let (reply, took) = wait_next(&main, 1).join().unwrap();
    assert!(
        took >= ms(3_000) && took < ms(6_000),
        "next_wait is 4 s: {took:?}"
    );
    assert_eq!(reply, fixture("reply-next-same"));
}

const PIN: &str =
    "draft-rulings-u41-gates.md waits for a ruling; this test pins what the head does today";

#[test]
fn pin_point_2_a_next_whose_arguments_do_not_fit_answers_the_whole_state_at_once() {
    let hub = hub(1, 4_000, 2_000);
    let (_app, main, _other) = app(hub.clone());
    let state = serde_json::to_string(&hub.coordinator().snapshot()).unwrap();
    hub.hidden();
    for _ in 0..2 {
        let start = Instant::now();
        let reply = invoke(&main, "agents_next", json!({"after": "x"}));
        let took = start.elapsed();
        assert_eq!(
            reply,
            Ok(state.clone()),
            "point 2 of {PIN}: the whole state, also while the window is hidden"
        );
        assert!(
            took < ms(1_000),
            "point 2 of {PIN}: at once, with no coalescing: {took:?}"
        );
    }
}

#[test]
fn pin_point_4_a_next_whose_after_is_above_the_seq_answers_the_whole_state_at_once() {
    let hub = hub(1, 4_000, 2_000);
    let (_app, main, _other) = app(hub.clone());
    let seq = hub.coordinator().snapshot().seq;
    let (reply, took) = wait_next(&main, seq + 5).join().unwrap();
    assert_eq!(seq_of(&reply), u64::from(seq), "point 4 of {PIN}");
    assert!(took < ms(1_000), "point 4 of {PIN}: at once: {took:?}");
}
