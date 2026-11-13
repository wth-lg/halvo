//! Probes of interrogate-win2-adversary-3 (loss accounting). Scratch only:
//! every probe is ignored, so 12-test does not run it; the scratch step runs
//! them with `--ignored adv3_ --nocapture --test-threads=1`. Each prints one
//! `ADV3` line and then asserts what the question or the intent asks for.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_F15,
};

use super::input::RawInputSource;
use super::{
    ensure_message_queue, forward_one, now_tick, pump_messages, spawn_pump, HandOff, Lifecycle,
    LossCounts, Packet, HANDOFF_CAPACITY,
};
use crate::event::Modality;
use crate::sink::SinkWriter;
use crate::source::{CaptureError, CaptureSource, SourceContext, Started};

fn packet(kind: Modality, app: &str) -> Packet {
    Packet {
        kind,
        app: app.to_string(),
        byte_len: 2,
        tick: now_tick(),
    }
}

fn wait_for(what: &str, limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let began = Instant::now();
    while !done() {
        if began.elapsed() >= limit {
            println!("ADV3 wait gave up: {what}");
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

fn drain(writer: &SinkWriter, kind: Modality, quiet: Duration) -> usize {
    let mut seen = 0;
    while let Ok(received) = writer.recv_timeout(quiet) {
        if received.event().modality == kind {
            seen += 1;
        }
    }
    seen
}

fn key_input(key: VIRTUAL_KEY, released: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if released {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> u32 {
    // SAFETY: `inputs` is a live slice of INPUT values and the size argument
    // is the size of one INPUT, as SendInput requires.
    unsafe { SendInput(inputs, core::mem::size_of::<INPUT>() as i32) }
}

// H1: keys typed before `shutdown` was called and still in the pump's queue.
// The test holds the shared last-app cell, so the pump blocks inside its
// first WM_INPUT and the other key messages stay queued; then a stop starts,
// and the cell is let go 200 ms later, well inside the 1 s deadline.
#[test]
#[ignore = "adv3 probe: sends VK_F15 presses, as the functional tests do"]
fn adv3_h1_keys_queued_before_a_stop_are_delivered_or_counted() {
    const PRESSES: u32 = 5;
    let _serial = super::tests::serial();
    let (sink, writer) = crate::sink::channel(64);
    let last_app = Arc::new(Mutex::new(None::<String>));
    let mut source = RawInputSource::keyboard(Arc::clone(&last_app));
    let start = source.begin(SourceContext::new(sink));
    assert!(matches!(start, Ok(Started::Running)), "{start:?}");
    std::thread::sleep(Duration::from_millis(200));
    let held = last_app.lock().expect("the last-app cell");
    let mut accepted = 0;
    for _ in 0..PRESSES {
        accepted += send(&[key_input(VK_F15, false)]);
        std::thread::sleep(Duration::from_millis(5));
        accepted += send(&[key_input(VK_F15, true)]);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(300));
    let (done, stopped) = mpsc::channel();
    let began = Instant::now();
    std::thread::Builder::new()
        .name("adv3-h1-stop".to_string())
        .spawn(move || {
            let outcome = source.shutdown();
            let counts = source.handoff_counts();
            let _ = done.send((outcome, counts));
        })
        .expect("spawn the stop thread");
    std::thread::sleep(Duration::from_millis(200));
    drop(held);
    let outcome = stopped.recv_timeout(Duration::from_secs(5));
    let stop_ms = began.elapsed().as_millis();
    let delivered = drain(&writer, Modality::Keystroke, Duration::from_millis(1_000));
    let (stop, lost) = match &outcome {
        Ok((stop, counts)) => (format!("{stop:?}"), counts.lost),
        Err(e) => (format!("no answer ({e:?})"), 0),
    };
    println!(
        "ADV3 H1 presses_sent={PRESSES} sendinput_events_accepted={accepted} stop={stop} stop_ms={stop_ms} keystrokes_delivered={delivered} lost={lost} unaccounted={}",
        i64::from(PRESSES) - delivered as i64 - lost as i64
    );
    assert_eq!(
        accepted,
        PRESSES * 2,
        "INCONCLUSIVE: SendInput did not insert every key event"
    );
    assert!(
        matches!(outcome, Ok((Ok(()), _))),
        "INCONCLUSIVE: the stop did not answer Ok"
    );
    assert_eq!(
        delivered as u64 + lost,
        u64::from(PRESSES),
        "key presses typed before the stop were neither delivered nor counted"
    );
}

// H2 (question 4): after a stop that timed out, the pump still runs and what
// it sees goes into the closed hand-off, which counts it as lost.
#[test]
#[ignore = "adv3 probe"]
fn adv3_h2_events_seen_after_a_timed_out_stop_are_not_counted_as_lost() {
    let (sink, writer) = crate::sink::channel(16);
    let mut life = Lifecycle::default();
    let (go, wait_go) = mpsc::channel::<()>();
    let started = life.begin(
        &SourceContext::new(sink),
        "adv3-h2-forward",
        move |producer| {
            spawn_pump("adv3-h2-pump".to_string(), move |ready, _signal| {
                ready.report(Ok(()));
                if wait_go.recv_timeout(Duration::from_secs(10)).is_ok() {
                    for _ in 0..3 {
                        producer.push(packet(Modality::Keystroke, "after-stop"));
                    }
                }
            })
        },
    );
    assert_eq!(started, Ok(Started::Running));
    let stopped = life.shutdown();
    let _ = go.send(());
    let ended = wait_for("the pump did not end", Duration::from_secs(5), || {
        life.pump.as_ref().is_none_or(|p| p.has_ended())
    });
    let delivered = drain(&writer, Modality::Keystroke, Duration::from_millis(300));
    let lost = life.handoff_counts().lost;
    let late = life.shutdown();
    println!(
        "ADV3 H2 stop={stopped:?} pushes_after_stop=3 pump_ended={ended} delivered_after_stop={delivered} lost={lost} late_stop={late:?}"
    );
    assert!(
        matches!(stopped, Err(CaptureError::Timeout { .. })),
        "INCONCLUSIVE: the stop did not time out"
    );
    assert_eq!(
        lost, 0,
        "lost counts {lost} events that the pump saw after its stop, beside full-queue drops"
    );
}

// H3 (question 2): run 1 stops (shutdown answers Ok), the owner takes one
// event and draws a barrier, run 2 starts. Packets that run 1's hand-off
// held at the draw were neither queued in the sink nor blocked in an emit.
#[test]
#[ignore = "adv3 probe"]
fn adv3_h3_old_run_packets_arrive_after_a_barrier_drawn_after_the_stop() {
    let (sink, writer) = crate::sink::channel(1);
    assert!(forward_one(&sink, packet(Modality::Keystroke, "filler")).is_ok());
    let ctx = SourceContext::new(sink);
    let quiet_pump = || {
        spawn_pump("adv3-h3-quiet".to_string(), |ready, signal| {
            ensure_message_queue();
            ready.report(Ok(()));
            pump_messages(&signal);
        })
    };
    let mut life = Lifecycle::default();
    life.begin(&ctx, "adv3-h3-forward-1", |producer| {
        for i in 1..=5 {
            producer.push(packet(Modality::Keystroke, &format!("old-{i}")));
        }
        quiet_pump()
    })
    .expect("run 1");
    let queued = |life: &Lifecycle| life.handoff.as_ref().map_or(0, HandOff::queued);
    assert!(wait_for(
        "old-1 not parked",
        Duration::from_secs(5),
        || queued(&life) == 4
    ));
    let stop1 = life.shutdown();
    let filler = writer
        .recv_timeout(Duration::from_secs(2))
        .expect("the filler");
    assert!(wait_for(
        "old-2 not parked",
        Duration::from_secs(5),
        || queued(&life) == 3
    ));
    let barrier = writer.barrier();
    let held = queued(&life);
    life.begin(&ctx, "adv3-h3-forward-2", |producer| {
        producer.push(packet(Modality::Keystroke, "new-1"));
        quiet_pump()
    })
    .expect("run 2");
    let mut arrivals = Vec::new();
    while let Ok(received) = writer.recv_timeout(Duration::from_millis(500)) {
        arrivals.push((
            received.event().foreground_app.clone(),
            received.number().get(),
            received.is_before(barrier),
            received.event().captured_at.sequence.get(),
        ));
    }
    let _ = life.shutdown();
    let listed: Vec<String> = arrivals
        .iter()
        .map(|(app, number, before, seq)| {
            format!(
                "{app}#n{number}/s{seq}/{}",
                if *before { "before" } else { "after" }
            )
        })
        .collect();
    let old_after = arrivals
        .iter()
        .filter(|(app, _, before, _)| app.starts_with("old-") && !before)
        .count();
    println!(
        "ADV3 H3 stop1={stop1:?} filler={} held_in_handoff_at_barrier={held} old_after_barrier={old_after} arrivals={}",
        filler.event().foreground_app,
        listed.join(",")
    );
    assert!(
        old_after <= 2,
        "{old_after} packets of the stopped run arrive after the barrier; {held} were in the hand-off at the draw"
    );
}

// H4: with 1,023 packets queued and a pending motion sample, the motion
// sample takes the last slot and the button that comes with it is dropped.
#[test]
#[ignore = "adv3 probe"]
fn adv3_h4_a_button_is_not_dropped_while_a_motion_sample_takes_the_last_slot() {
    let (sink, _writer) = crate::sink::channel(1);
    assert!(forward_one(&sink, packet(Modality::Pointer, "filler")).is_ok());
    let handoff =
        HandOff::start(sink, "adv3-h4-forward", Arc::<LossCounts>::default()).expect("hand-off");
    let producer = handoff.producer();
    producer.push(packet(Modality::Pointer, "taken"));
    assert!(wait_for(
        "the forwarder took no packet",
        Duration::from_secs(5),
        || handoff.queued() == 0
    ));
    for _ in 0..HANDOFF_CAPACITY - 1 {
        producer.push(packet(Modality::Pointer, "button"));
    }
    producer.push_motion(packet(Modality::Pointer, "motion"));
    producer.push(packet(Modality::Pointer, "last-button"));
    let (queued, last, pending) = {
        let queue = handoff.shared.lock();
        (
            queue.packets.len(),
            queue
                .packets
                .back()
                .map(|p| p.app.clone())
                .unwrap_or_default(),
            queue.motion.is_some(),
        )
    };
    let counts = handoff.counts();
    println!(
        "ADV3 H4 queued={queued} last_queued={last} pending_motion={pending} lost={} coalesced={}",
        counts.lost, counts.coalesced
    );
    assert_eq!(
        counts.lost, 0,
        "the button was dropped and the motion sample took the last slot"
    );
}

// H6 (question 1): a keyboard source whose stop timed out is dropped while its
// pump is still blocked; a new keyboard source of the process is then refused
// until the old pump ends, and nothing tells the caller why.
#[test]
#[ignore = "adv3 probe: sends one VK_F15 press, as the functional tests do"]
fn adv3_h6_a_dropped_keyboard_source_whose_stop_timed_out_keeps_the_device() {
    let _serial = super::tests::serial();
    let (sink, _writer) = crate::sink::channel(64);
    let last_app = Arc::new(Mutex::new(None::<String>));
    let mut first = RawInputSource::keyboard(Arc::clone(&last_app));
    let start = first.begin(SourceContext::new(sink.clone()));
    assert!(matches!(start, Ok(Started::Running)), "{start:?}");
    std::thread::sleep(Duration::from_millis(200));
    let held = last_app.lock().expect("the last-app cell");
    let accepted = send(&[key_input(VK_F15, false)]) + send(&[key_input(VK_F15, true)]);
    std::thread::sleep(Duration::from_millis(200));
    let stop = first.shutdown();
    let first_state = first.health().state;
    drop(first);
    let mut second = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
    let while_stuck = second.begin(SourceContext::new(sink.clone()));
    let second_state = second.health().state;
    drop(held);
    let released = Instant::now();
    let mut after = second.begin(SourceContext::new(sink.clone()));
    while after.is_err() && released.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
        after = second.begin(SourceContext::new(sink.clone()));
    }
    let after_ms = released.elapsed().as_millis();
    let _ = second.shutdown();
    println!(
        "ADV3 H6 sendinput_events_accepted={accepted} first_stop={stop:?} first_health={first_state:?} second_begin_while_old_pump_stuck={while_stuck:?} second_health_then={second_state:?} second_begin_after_release={after:?} after_ms={after_ms}"
    );
    assert_eq!(
        accepted, 2,
        "INCONCLUSIVE: SendInput did not insert the key press"
    );
    assert!(
        matches!(stop, Err(CaptureError::Timeout { .. })),
        "INCONCLUSIVE: the stop did not time out"
    );
}

// H7 (question 3): after a stop that joined its pump, the drop of the pump
// handle posts WM_QUIT to the id of the joined thread. A thread that the
// system gives the same id receives it.
#[test]
#[ignore = "adv3 probe"]
fn adv3_h7_the_quit_post_after_a_completed_stop_reaches_a_thread_that_reuses_the_id() {
    use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_QUIT};
    let mut handle = spawn_pump("adv3-h7-quiet".to_string(), |ready, signal| {
        ensure_message_queue();
        ready.report(Ok(()));
        pump_messages(&signal);
    })
    .expect("the pump");
    let id = handle.id.get();
    let stop = handle.stop(super::INPUT_STOP_DEADLINE);
    let mut kept = None;
    let mut others = Vec::new();
    let mut tried = 0u32;
    while tried < 2_000 && kept.is_none() {
        tried += 1;
        let (told, tell) = mpsc::channel::<u32>();
        let (go, wait_go) = mpsc::channel::<bool>();
        let thread = std::thread::spawn(move || {
            ensure_message_queue();
            let _ = told.send(super::PumpThreadId::current().get());
            if !wait_go
                .recv_timeout(Duration::from_secs(30))
                .unwrap_or(false)
            {
                return None;
            }
            std::thread::sleep(Duration::from_millis(300));
            let mut msg = MSG::default();
            // SAFETY: `msg` is a live MSG that PeekMessageW fills.
            let got = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool();
            Some(got && msg.message == WM_QUIT)
        });
        let theirs = tell.recv_timeout(Duration::from_secs(5)).unwrap_or(0);
        if theirs == id {
            kept = Some((go, thread));
        } else {
            others.push((go, thread));
        }
    }
    for (go, thread) in others {
        let _ = go.send(false);
        let _ = thread.join();
    }
    let reused = kept.is_some();
    drop(handle);
    let received = kept.map(|(go, thread)| {
        let _ = go.send(true);
        thread.join().ok().flatten()
    });
    println!(
        "ADV3 H7 stop={stop:?} pump_id={id} threads_started={tried} id_reused={reused} quit_received_by_the_thread_with_that_id={received:?}"
    );
    assert_eq!(stop, Ok(()), "INCONCLUSIVE: the stop did not answer Ok");
    assert_ne!(
        received,
        Some(Some(true)),
        "the drop after a completed stop posted WM_QUIT to another thread that holds the id"
    );
}

fn hook_without_callback(
    event: u32,
    _callback: windows::Win32::UI::Accessibility::WINEVENTPROC,
) -> windows::Win32::UI::Accessibility::HWINEVENTHOOK {
    // SAFETY: SetWinEventHook takes the event range, no module and no
    // callback; the system refuses the hook, which is the case under test.
    unsafe {
        windows::Win32::UI::Accessibility::SetWinEventHook(
            event,
            event,
            None,
            None,
            0,
            0,
            super::foreground::HOOK_FLAGS,
        )
    }
}

fn hook_with_reversed_range(
    event: u32,
    callback: windows::Win32::UI::Accessibility::WINEVENTPROC,
) -> windows::Win32::UI::Accessibility::HWINEVENTHOOK {
    // SAFETY: SetWinEventHook takes an event range whose minimum is above its
    // maximum, no module and a callback with the WINEVENTPROC signature.
    unsafe {
        windows::Win32::UI::Accessibility::SetWinEventHook(
            event,
            event.wrapping_sub(1),
            None,
            callback,
            0,
            0,
            super::foreground::HOOK_FLAGS,
        )
    }
}

// H8 (the intent: a source that cannot start returns the failed step and its
// OS code): two hooks that the system refuses, through the injectable
// installer of the foreground source.
#[test]
#[ignore = "adv3 probe"]
fn adv3_h8_a_refused_hook_reports_a_nonzero_os_code() {
    let _serial = super::tests::serial();
    let (sink, _writer) = crate::sink::channel(4);
    let installers: [(&str, super::foreground::HookInstaller); 2] = [
        ("no_callback", hook_without_callback),
        ("reversed_range", hook_with_reversed_range),
    ];
    let mut codes = Vec::new();
    for (name, installer) in installers {
        let mut source = super::foreground::ForegroundAppSource::with_installer(installer);
        let started = source.begin(SourceContext::new(sink.clone()));
        let _ = source.shutdown();
        println!("ADV3 H8 installer={name} begin={started:?}");
        codes.push((name, started));
    }
    for (name, started) in codes {
        if let Err(CaptureError::Os { code, .. }) = started {
            assert_ne!(code, 0, "{name}: the refused hook reports OS code 0");
        }
    }
}
