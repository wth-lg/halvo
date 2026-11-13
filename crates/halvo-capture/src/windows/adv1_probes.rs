//! Probes of interrogate-win2-adversary-1: ignored tests that print `ADV1`
//! lines and assert nothing, run by a step of their own in a scratch run.

use super::input::RawInputSource;
use super::screen::ScreenSource;
use super::{
    ensure_message_queue, forward_one, now_tick, pump_messages, spawn_pump, Lifecycle, Packet,
    PumpHandle, PumpThreadId,
};
use crate::event::Modality;
use crate::source::{CaptureError, CaptureSource, SourceContext};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn limited(name: &'static str, secs: u64, body: impl FnOnce() + Send + 'static) {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        body();
        let _ = done.send(());
    });
    if finished.recv_timeout(Duration::from_secs(secs)).is_err() {
        println!("ADV1 {name} TIMEOUT after {secs} s");
        std::process::exit(3);
    }
}

fn key(app: String) -> Packet {
    Packet {
        kind: Modality::Keystroke,
        app,
        byte_len: 2,
        tick: now_tick(),
    }
}

fn quiet_pump() -> Result<PumpHandle, CaptureError> {
    spawn_pump("adv1-pump-quiet".to_string(), |ready, signal| {
        ensure_message_queue();
        ready.report(Ok(()));
        pump_messages(&signal);
    })
}

fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let began = Instant::now();
    while !done() {
        if began.elapsed() > limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    true
}

/// Question 2: the packets that a stopped run still held, after a begin.
#[test]
#[ignore = "adv1 probe"]
fn adv1_q2_old_run_packets_after_a_restart() {
    limited("q2", 120, || {
        const TRIALS: usize = 40;
        let (mut old_at_or_after_resume, mut old_total) = (0, 0);
        let (mut old_after_new, mut clock_back, mut short) = (0, 0, 0);
        for trial in 0..TRIALS {
            let (sink, writer) = crate::sink::channel(1);
            let _ = forward_one(&sink, key("filler".to_string()));
            let ctx = SourceContext::new(sink.clone());
            let mut life = Lifecycle::default();
            let first = life.begin(&ctx, "adv1-fwd-1", |producer| {
                for i in 0..5 {
                    producer.push(key(format!("old-{i}")));
                }
                quiet_pump()
            });
            let parked = wait_for(Duration::from_secs(2), || {
                life.handoff.as_ref().is_some_and(|h| h.queued() == 4)
            });
            let stop = life.shutdown();
            let pause = writer.barrier();
            std::thread::sleep(Duration::from_millis(5));
            let second = life.begin(&ctx, "adv1-fwd-2", |producer| {
                for i in 0..5 {
                    producer.push(key(format!("new-{i}")));
                }
                quiet_pump()
            });
            let resume = writer.barrier();
            let mut got = Vec::new();
            while got.len() < 11 {
                match writer.recv_timeout(Duration::from_millis(500)) {
                    Ok(received) => got.push(received),
                    Err(_) => break,
                }
            }
            let _ = life.shutdown();
            let order: Vec<&str> = got
                .iter()
                .map(|r| r.event().foreground_app.as_str())
                .collect();
            if trial < 3 {
                println!(
                    "ADV1 q2 trial={trial} first={first:?} parked={parked} stop={stop:?} \
                     second={second:?} pause_eq_resume={} order={order:?}",
                    pause == resume
                );
            }
            if got.len() < 11 {
                short += 1;
            }
            let mut seen_new = false;
            let mut inverted = false;
            for r in &got {
                let app = r.event().foreground_app.as_str();
                if app.starts_with("new-") {
                    seen_new = true;
                } else if app.starts_with("old-") {
                    old_total += 1;
                    if !r.is_before(resume) {
                        old_at_or_after_resume += 1;
                    }
                    if seen_new {
                        inverted = true;
                    }
                }
            }
            if inverted {
                old_after_new += 1;
            }
            if got
                .windows(2)
                .any(|w| w[1].event().captured_at.wall_clock < w[0].event().captured_at.wall_clock)
            {
                clock_back += 1;
            }
        }
        println!(
            "ADV1 q2 summary trials={TRIALS} short_trials={short} old_packets={old_total} \
             old_numbered_at_or_after_the_resume_barrier={old_at_or_after_resume} \
             trials_with_an_old_packet_after_a_new_one={old_after_new} \
             trials_with_wall_clock_going_back={clock_back}"
        );
    });
}

/// Questions 1 and 8: what a source whose stop timed out holds after its drop.
#[test]
#[ignore = "adv1 probe"]
fn adv1_q8_a_dropped_source_whose_stop_timed_out_keeps_the_sink_open() {
    limited("q8", 60, || {
        let (sink, writer) = crate::sink::channel(16);
        let mut life = Lifecycle::default();
        let begun = life.begin(
            &SourceContext::new(sink.clone()),
            "adv1-fwd-held",
            |producer| {
                spawn_pump("adv1-pump-held".to_string(), move |ready, _signal| {
                    ready.report(Ok(()));
                    let _held = producer;
                    std::thread::sleep(Duration::from_millis(3_000));
                })
            },
        );
        let began = Instant::now();
        let stop = life.shutdown();
        let stop_ms = began.elapsed().as_millis();
        let dropped_at = Instant::now();
        drop(life);
        let drop_ms = dropped_at.elapsed().as_millis();
        drop(sink);
        let mut closed_ms = None;
        while dropped_at.elapsed() < Duration::from_secs(10) {
            if let Err(RecvTimeoutError::Disconnected) =
                writer.recv_timeout(Duration::from_millis(20))
            {
                closed_ms = Some(dropped_at.elapsed().as_millis());
                break;
            }
        }
        println!(
            "ADV1 q8 begin={begun:?} stop={stop:?} stop_ms={stop_ms} drop_ms={drop_ms} \
             sink_closed_ms_after_the_drop={closed_ms:?}"
        );
    });
}

fn screen_frames(
    writer: crate::sink::SinkWriter,
) -> (mpsc::Receiver<(usize, Instant)>, mpsc::Receiver<()>) {
    let (gone, gone_rx) = mpsc::channel();
    let (frame, frame_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut frames = 0;
        loop {
            match writer.recv_timeout(Duration::from_millis(20)) {
                Ok(_) => {
                    frames += 1;
                    let _ = frame.send(());
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = gone.send((frames, Instant::now()));
                    return;
                }
            }
        }
    });
    (gone_rx, frame_rx)
}

/// Question 6 and lens 1, on the product's screen source: a frame callback
/// that waits on the shared last-app cell (held here) makes the library's
/// stop wait; then a drop.
#[test]
#[ignore = "adv1 probe"]
fn adv1_q6_screen_stop_with_a_held_frame_callback_then_drop() {
    limited("q6-drop", 120, || {
        let _serial = super::tests::serial();
        let last_app: Arc<Mutex<Option<String>>> = Arc::default();
        let held = last_app.lock().expect("the cell");
        let (sink, writer) = crate::sink::channel(64);
        let (gone, _frames) = screen_frames(writer);
        let mut screen = ScreenSource::new(Arc::clone(&last_app));
        let start = screen.begin(SourceContext::new(sink.clone()));
        std::thread::sleep(Duration::from_millis(1_500));
        let began = Instant::now();
        let first = screen.shutdown();
        let first_ms = began.elapsed().as_millis();
        let state = screen.health().state;
        let again = screen.begin(SourceContext::new(sink.clone()));
        let counts = screen.handoff_counts();
        drop(sink);
        let dropped_at = Instant::now();
        drop(screen);
        let drop_ms = dropped_at.elapsed().as_millis();
        let open_1s_after_drop = gone.recv_timeout(Duration::from_secs(1)).is_err();
        let (sink2, writer2) = crate::sink::channel(64);
        let (_gone2, frames2) = screen_frames(writer2);
        let mut other = ScreenSource::new(Arc::default());
        let other_start = other.begin(SourceContext::new(sink2));
        let other_frame = frames2.recv_timeout(Duration::from_secs(3)).is_ok();
        let other_began = Instant::now();
        let other_stop = other.shutdown();
        let other_stop_ms = other_began.elapsed().as_millis();
        let released = Instant::now();
        drop(held);
        let closed = gone
            .recv_timeout(Duration::from_secs(15))
            .map(|(frames, at)| (frames, at.duration_since(released).as_millis()));
        println!(
            "ADV1 q6-drop start={start:?} first_stop={first:?} first_stop_ms={first_ms} \
             state={state:?} begin_after_timeout={again:?} counts={counts:?} drop_ms={drop_ms} \
             sink_open_1s_after_drop={open_1s_after_drop} other_start={other_start:?} \
             other_frame={other_frame} other_stop={other_stop:?} other_stop_ms={other_stop_ms} \
             sink_closed_after_release={closed:?}"
        );
    });
}

/// Questions 4 and 6: the same held callback, then a second shutdown.
#[test]
#[ignore = "adv1 probe"]
fn adv1_q6_screen_stop_with_a_held_frame_callback_then_stop_again() {
    limited("q6-again", 120, || {
        let _serial = super::tests::serial();
        let last_app: Arc<Mutex<Option<String>>> = Arc::default();
        let held = last_app.lock().expect("the cell");
        let (sink, writer) = crate::sink::channel(64);
        let (gone, _frames) = screen_frames(writer);
        let mut screen = ScreenSource::new(Arc::clone(&last_app));
        let start = screen.begin(SourceContext::new(sink.clone()));
        std::thread::sleep(Duration::from_millis(1_500));
        let began = Instant::now();
        let first = screen.shutdown();
        let first_ms = began.elapsed().as_millis();
        let counts_at_timeout = screen.handoff_counts();
        let released = Instant::now();
        drop(held);
        std::thread::sleep(Duration::from_millis(500));
        let state_after_release = screen.health().state;
        let began = Instant::now();
        let second = screen.shutdown();
        let second_ms = began.elapsed().as_millis();
        let state = screen.health().state;
        let counts = screen.handoff_counts();
        drop(screen);
        drop(sink);
        let closed = gone
            .recv_timeout(Duration::from_secs(15))
            .map(|(frames, at)| (frames, at.duration_since(released).as_millis()));
        println!(
            "ADV1 q6-again start={start:?} first_stop={first:?} first_stop_ms={first_ms} \
             counts_at_timeout={counts_at_timeout:?} state_500ms_after_release={state_after_release:?} \
             second_stop={second:?} second_stop_ms={second_ms} state={state:?} counts={counts:?} \
             sink_closed={closed:?}"
        );
    });
}

/// Question 3: the quit post of a pump handle's drop after a stop that
/// joined the thread, when the thread's id belongs to another thread.
#[test]
#[ignore = "adv1 probe"]
fn adv1_q3_the_drop_after_a_clean_stop_posts_to_a_reused_id() {
    limited("q3", 150, || {
        let mut handle = quiet_pump().expect("the pump");
        let dead = handle.id;
        let stop = handle.stop(super::INPUT_STOP_DEADLINE);
        let mut parked = Vec::new();
        let mut victim = None;
        for tries in 0..4_000 {
            let (id_tx, id_rx) = mpsc::channel();
            let (ask_tx, ask_rx) = mpsc::channel::<u64>();
            let (say_tx, say_rx) = mpsc::channel::<bool>();
            let spawned = std::thread::Builder::new()
                .stack_size(64 * 1024)
                .spawn(move || {
                    ensure_message_queue();
                    let _ = id_tx.send(PumpThreadId::current());
                    while let Ok(wait_ms) = ask_rx.recv() {
                        let _ = say_tx.send(saw_quit(Duration::from_millis(wait_ms)));
                    }
                });
            let Ok(thread) = spawned else { break };
            let Ok(id) = id_rx.recv() else { break };
            if id == dead {
                victim = Some((thread, ask_tx, say_rx, tries));
                break;
            }
            parked.push((thread, ask_tx));
        }
        let line = match victim {
            Some((thread, ask, say, tries)) => {
                let _ = ask.send(0);
                let before = say.recv_timeout(Duration::from_secs(5));
                drop(handle);
                let _ = ask.send(1_000);
                let after = say.recv_timeout(Duration::from_secs(5));
                drop(ask);
                let _ = thread.join();
                format!(
                    "reused_after_spawns={tries} quit_before_the_drop={before:?} \
                     quit_after_the_drop={after:?}"
                )
            }
            None => {
                drop(handle);
                format!("not_reused_in_spawns={}", parked.len())
            }
        };
        for (thread, ask) in parked {
            drop(ask);
            let _ = thread.join();
        }
        println!("ADV1 q3 stop={stop:?} dead_id={} {line}", dead.get());
    });
}

fn saw_quit(wait: Duration) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_QUIT};
    let began = Instant::now();
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills; no window is
        // named, so it reads this thread's own queue.
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                return true;
            }
        }
        if began.elapsed() >= wait {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn hook_code(started: Result<crate::source::Started, CaptureError>) -> String {
    match started {
        Err(CaptureError::Os { stage, code, .. }) => format!("stage={stage} code={code:#x}"),
        other => format!("{other:?}"),
    }
}

fn no_hook(
    _event: u32,
    _callback: windows::Win32::UI::Accessibility::WINEVENTPROC,
) -> windows::Win32::UI::Accessibility::HWINEVENTHOOK {
    windows::Win32::UI::Accessibility::HWINEVENTHOOK::default()
}

fn in_context_hook_without_a_module(
    event: u32,
    callback: windows::Win32::UI::Accessibility::WINEVENTPROC,
) -> windows::Win32::UI::Accessibility::HWINEVENTHOOK {
    use windows::Win32::UI::WindowsAndMessaging::WINEVENT_INCONTEXT;
    // SAFETY: SetWinEventHook takes the event range, no module and a callback
    // with the WINEVENTPROC signature; an in-context hook without a module is
    // refused by the system, which is what this probe asks.
    unsafe {
        windows::Win32::UI::Accessibility::SetWinEventHook(
            event,
            event,
            None,
            callback,
            0,
            0,
            WINEVENT_INCONTEXT,
        )
    }
}

/// Question 12 and the intent's "the failed step and its OS code": the code
/// that a hook source reports when its hook does not install.
#[test]
#[ignore = "adv1 probe"]
fn adv1_q12_the_os_code_of_a_hook_that_does_not_install() {
    limited("q12", 60, || {
        let _serial = super::tests::serial();
        let (sink, _writer) = crate::sink::channel(4);
        let mut stub = super::foreground::ForegroundAppSource::with_installer(no_hook);
        let stub_start = hook_code(stub.begin(SourceContext::new(sink.clone())));
        let _ = stub.shutdown();
        let mut real = super::foreground::ForegroundAppSource::with_installer(
            in_context_hook_without_a_module,
        );
        let real_start = hook_code(real.begin(SourceContext::new(sink.clone())));
        let _ = real.shutdown();
        let mut title = super::window_title::WindowTitleSource::with_installer(
            in_context_hook_without_a_module,
        );
        let title_start = hook_code(title.begin(SourceContext::new(sink)));
        let _ = title.shutdown();
        println!(
            "ADV1 q12 stub_installer: {stub_start} | real_SetWinEventHook_refused: {real_start} \
             | title_real_refused: {title_start}"
        );
    });
}

/// Lens 3, from the finder lane: a pending motion sample and a button at a
/// queue with one free slot.
#[test]
#[ignore = "adv1 probe"]
fn adv1_motion_takes_the_last_slot_and_the_button_is_lost() {
    limited("motion", 60, || {
        let (sink, writer) = crate::sink::channel(1);
        let pointer = |app: &str| Packet {
            kind: Modality::Pointer,
            app: app.to_string(),
            byte_len: 12,
            tick: now_tick(),
        };
        let _ = forward_one(&sink, pointer("filler"));
        let handoff =
            super::HandOff::start(sink, "adv1-fwd-motion", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        producer.push(pointer("taken"));
        let took = wait_for(Duration::from_secs(5), || handoff.queued() == 0);
        for _ in 0..super::HANDOFF_CAPACITY - 1 {
            producer.push(pointer("queued"));
        }
        producer.push_motion(pointer("motion"));
        producer.push(pointer("button"));
        let queued = handoff.queued();
        let counts = handoff.counts();
        let pending = handoff.pending_motion();
        let mut apps = Vec::new();
        while let Ok(received) = writer.recv_timeout(Duration::from_millis(500)) {
            apps.push(received.event().foreground_app.clone());
        }
        let count = |app: &str| apps.iter().filter(|a| a.as_str() == app).count();
        println!(
            "ADV1 motion forwarder_took={took} queued={queued} pending_motion={pending} \
             counts={counts:?} delivered_button={} delivered_motion={} delivered_total={}",
            count("button"),
            count("motion"),
            apps.len()
        );
    });
}

fn keyboard_targets() -> Result<usize, u32> {
    use windows::Win32::UI::Input::{GetRegisteredRawInputDevices, RAWINPUTDEVICE};
    let size = core::mem::size_of::<RAWINPUTDEVICE>() as u32;
    let mut count = 0u32;
    // SAFETY: with no array the call writes the number of registrations into
    // `count`, a live local.
    let probe = unsafe { GetRegisteredRawInputDevices(None, &mut count, size) };
    if probe == u32::MAX {
        return Err(probe);
    }
    let mut devices = vec![RAWINPUTDEVICE::default(); count as usize];
    if count == 0 {
        return Ok(0);
    }
    // SAFETY: `devices` holds `count` initialised entries, the number that the
    // first call gave, and outlives the call.
    let got = unsafe { GetRegisteredRawInputDevices(Some(devices.as_mut_ptr()), &mut count, size) };
    if got == u32::MAX {
        return Err(got);
    }
    Ok(devices[..got as usize]
        .iter()
        .filter(|d| d.usUsagePage == 1 && d.usUsage == 6)
        .count())
}

/// Question 9: a second keyboard source that begins while the first stops.
#[test]
#[ignore = "adv1 probe"]
fn adv1_q9_a_begin_while_another_keyboard_source_stops() {
    limited("q9", 120, || {
        let _serial = super::tests::serial();
        const CYCLES: usize = 20;
        let (sink, _writer) = crate::sink::channel(64);
        let mut odd = Vec::new();
        for cycle in 0..CYCLES {
            let mut first = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
            let first_start = first.begin(SourceContext::new(sink.clone()));
            let while_first = keyboard_targets();
            let ctx = SourceContext::new(sink.clone());
            let racer = std::thread::spawn(move || {
                let mut second = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
                let began = Instant::now();
                let mut refused = 0;
                loop {
                    match second.begin(SourceContext::new(ctx.sink().clone())) {
                        Err(CaptureError::AlreadyStarted)
                            if began.elapsed() < Duration::from_secs(5) =>
                        {
                            refused += 1;
                        }
                        other => return (second, other, refused),
                    }
                }
            });
            std::thread::sleep(Duration::from_millis(10));
            let stopped = if cycle % 2 == 0 {
                first.shutdown()
            } else {
                drop(first);
                Ok(())
            };
            let (mut second, second_start, refused) = racer.join().expect("the racer");
            let while_second = keyboard_targets();
            let second_stop = second.shutdown();
            let after = keyboard_targets();
            let line = format!(
                "cycle={cycle} first_start={first_start:?} while_first={while_first:?} \
                 first_stop={stopped:?} second_start={second_start:?} refused={refused} \
                 while_second={while_second:?} second_stop={second_stop:?} after={after:?}"
            );
            if cycle < 2
                || first_start.is_err()
                || second_start.is_err()
                || while_first != Ok(1)
                || while_second != Ok(1)
                || after != Ok(0)
            {
                odd.push(line);
            }
        }
        for line in &odd {
            println!("ADV1 q9 {line}");
        }
        println!(
            "ADV1 q9 summary cycles={CYCLES} lines_printed={}",
            odd.len()
        );
    });
}
