//! Probes of verifier 103 for unit win-2. Scratch only, never merged. Each
//! test returns on its first line unless HALVO_GATE_PROBE names it, so the
//! job's own steps run them as tests that do nothing. A named probe has a time
//! limit of its own, prints `GP` lines and asserts nothing. One probe with two
//! parameters: the end of a source's run and the timing of the input before
//! it. Every input event is an F15 key, a relative pointer move or a wheel
//! notch, made by the probe itself.

use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_WHEEL, MOUSEINPUT, VK_F15,
};

use super::RawInputSource;
use crate::event::Modality;
use crate::source::{CaptureSource, SourceContext, Started};

fn enabled(name: &'static str, limit: Duration) -> bool {
    if std::env::var("HALVO_GATE_PROBE").map_or(true, |named| named != name) {
        return false;
    }
    std::thread::spawn(move || {
        std::thread::sleep(limit);
        println!("GP {name} time_limit_reached_s={}", limit.as_secs());
        std::process::exit(3);
    });
    true
}

fn trials() -> u32 {
    std::env::var("HALVO_GATE_TRIALS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(20)
}

fn send(input: INPUT) -> u64 {
    // SAFETY: the slice holds one live INPUT and the size argument is the
    // size of one INPUT, as SendInput requires.
    u64::from(unsafe { SendInput(&[input], core::mem::size_of::<INPUT>() as i32) })
}

fn key(released: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_F15,
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

fn pointer_move(dx: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy: 0,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn wheel_notch() -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 120,
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Stop,
    ShutdownSinkFull,
    Drop,
    DropAfterTimeout,
}

#[derive(Clone, Copy)]
enum Input {
    Keys {
        presses: u32,
        end_before_release: bool,
    },
    Pointer {
        moves: u32,
        wheel_last: bool,
    },
}

struct Timing {
    name: &'static str,
    input: Input,
    gap_ms: u64,
    settle_ms: u64,
    pump_held: bool,
}

const fn keys(presses: u32, end_before_release: bool) -> Input {
    Input::Keys {
        presses,
        end_before_release,
    }
}

const fn pointer(wheel_last: bool) -> Input {
    Input::Pointer {
        moves: 30,
        wheel_last,
    }
}

const fn timing(
    name: &'static str,
    input: Input,
    gap_ms: u64,
    settle_ms: u64,
    pump_held: bool,
) -> Timing {
    Timing {
        name,
        input,
        gap_ms,
        settle_ms,
        pump_held,
    }
}

const TIMINGS: [Timing; 16] = [
    timing(
        "control_300_ms_after_the_last_key",
        keys(5, false),
        5,
        300,
        false,
    ),
    timing("right_after_the_last_release", keys(5, false), 5, 0, false),
    timing("right_after_the_last_press", keys(5, true), 5, 0, false),
    timing(
        "no_gap_right_after_the_last_release",
        keys(5, false),
        0,
        0,
        false,
    ),
    timing(
        "no_gap_right_after_the_last_press",
        keys(5, true),
        0,
        0,
        false,
    ),
    timing(
        "no_gap_1_ms_after_the_last_key",
        keys(5, false),
        0,
        1,
        false,
    ),
    timing(
        "no_gap_5_ms_after_the_last_key",
        keys(5, false),
        0,
        5,
        false,
    ),
    timing("burst_of_40_keys_no_gap", keys(40, false), 0, 0, false),
    timing("pump_held_keys_queued_300_ms", keys(5, false), 5, 300, true),
    timing("pump_held_no_gap_no_settle", keys(5, false), 0, 0, true),
    timing("pump_held_burst_of_40_keys", keys(40, false), 0, 50, true),
    timing("pointer_control_300_ms", pointer(false), 2, 300, false),
    timing("pointer_no_gap_wheel_inside", pointer(false), 0, 0, false),
    timing("pointer_no_gap_wheel_last", pointer(true), 0, 0, false),
    timing(
        "pointer_pump_held_wheel_inside",
        pointer(false),
        0,
        100,
        true,
    ),
    timing("pointer_pump_held_wheel_last", pointer(true), 0, 0, true),
];

#[derive(Default)]
struct Sums {
    trials: u32,
    odd: u32,
    accepted: u64,
    delivered: u64,
    lost: u64,
    coalesced: u64,
    neither: u64,
    both: u64,
    trials_neither: u32,
    trials_both: u32,
    slowest_end_ms: u128,
}

fn send_input(input: Input, gap: Duration) -> (u64, u64) {
    let (mut wanted, mut accepted) = (0u64, 0u64);
    match input {
        Input::Keys {
            presses,
            end_before_release,
        } => {
            for press in 0..presses {
                wanted += 1;
                accepted += send(key(false));
                let last = press + 1 == presses;
                if last && end_before_release {
                    break;
                }
                std::thread::sleep(gap);
                let _ = send(key(true));
                if !last {
                    std::thread::sleep(gap);
                }
            }
        }
        Input::Pointer { moves, wheel_last } => {
            for step in 0..moves {
                if !wheel_last && step == moves / 2 {
                    wanted += 1;
                    accepted += send(wheel_notch());
                }
                wanted += 1;
                accepted += send(pointer_move(if step.is_multiple_of(2) { 3 } else { -3 }));
                std::thread::sleep(gap);
            }
            if wheel_last {
                wanted += 1;
                accepted += send(wheel_notch());
            }
        }
    }
    (wanted, accepted)
}

fn one_trial(end: End, timing: &Timing, trial: u32, sums: &mut Sums) {
    sums.trials += 1;
    std::thread::sleep(Duration::from_millis(30));
    let capacity = if end == End::ShutdownSinkFull {
        1
    } else {
        4_096
    };
    let (sink, writer) = crate::sink::channel(capacity);
    let last_app = Arc::new(Mutex::new(None::<String>));
    let (mut source, kind) = match timing.input {
        Input::Keys { .. } => (
            RawInputSource::keyboard(Arc::clone(&last_app)),
            Modality::Keystroke,
        ),
        Input::Pointer { .. } => (
            RawInputSource::pointer(Arc::clone(&last_app)),
            Modality::Pointer,
        ),
    };
    let begun = source.begin(SourceContext::new(sink.clone()));
    if !matches!(begun, Ok(Started::Running)) {
        sums.odd += 1;
        println!(
            "GP trial end={end:?} timing={} trial={trial} odd=true begin=[{begun:?}]",
            timing.name
        );
        return;
    }
    let counts = Arc::clone(&source.life.counts);
    std::thread::sleep(Duration::from_millis(50));
    let held = timing
        .pump_held
        .then(|| last_app.lock().unwrap_or_else(PoisonError::into_inner));
    let (wanted, accepted) = send_input(timing.input, Duration::from_millis(timing.gap_ms));
    let sent = Instant::now();
    std::thread::sleep(Duration::from_millis(timing.settle_ms));
    let end_after_last_send_us = sent.elapsed().as_micros();
    let (done, answered) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let began = Instant::now();
        let (answer, kept) = match end {
            End::Stop | End::ShutdownSinkFull => {
                let stopped = source.shutdown();
                (format!("{stopped:?}"), Some(source))
            }
            End::Drop => {
                drop(source);
                ("dropped".to_string(), None)
            }
            End::DropAfterTimeout => {
                let stopped = source.shutdown();
                let stop_ms = began.elapsed().as_millis();
                drop(source);
                (
                    format!("{stopped:?} after {stop_ms} ms, then dropped"),
                    None,
                )
            }
        };
        let _ = done.send((answer, began.elapsed().as_millis(), kept));
    });
    let outcome = if end == End::DropAfterTimeout {
        let outcome = answered.recv_timeout(Duration::from_secs(5));
        drop(held);
        outcome
    } else {
        if held.is_some() {
            std::thread::sleep(Duration::from_millis(200));
        }
        drop(held);
        answered.recv_timeout(Duration::from_secs(5))
    };
    let lost_when_the_end_returned = counts.read().lost;
    if let Input::Keys {
        end_before_release: true,
        ..
    } = timing.input
    {
        let _ = send(key(true));
    }
    let (answer, end_ms, answered_in_time) = match outcome {
        Ok((answer, end_ms, kept)) => {
            drop(kept);
            (answer, end_ms, true)
        }
        Err(_) => ("no answer in 5 s".to_string(), 5_000, false),
    };
    drop(sink);
    let (mut delivered, mut other_kinds) = (0u64, 0u64);
    let mut disconnected = false;
    let drain_began = Instant::now();
    while drain_began.elapsed() < Duration::from_secs(8) {
        match writer.recv_timeout(Duration::from_millis(100)) {
            Ok(received) if received.event().modality == kind => delivered += 1,
            Ok(_) => other_kinds += 1,
            Err(RecvTimeoutError::Disconnected) => {
                disconnected = true;
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    let after = counts.read();
    let counted = delivered + after.lost + after.coalesced;
    let neither = accepted.saturating_sub(counted);
    let both = counted.saturating_sub(accepted);
    let odd = wanted != accepted || !answered_in_time || !disconnected;
    println!(
        "GP trial end={end:?} timing={} trial={trial} events_sent={wanted} events_accepted_by_the_system={accepted} delivered={delivered} counted_lost={} coalesced={} in_neither_count={neither} in_two_counts={both} lost_when_the_end_returned={lost_when_the_end_returned} events_of_another_kind={other_kinds} end_ms={end_ms} end_after_last_send_us={end_after_last_send_us} sink_closed_after_the_end={disconnected} odd={odd} end_answer=[{answer}]",
        timing.name, after.lost, after.coalesced
    );
    if odd {
        sums.odd += 1;
        return;
    }
    sums.accepted += accepted;
    sums.delivered += delivered;
    sums.lost += after.lost;
    sums.coalesced += after.coalesced;
    sums.neither += neither;
    sums.both += both;
    sums.trials_neither += u32::from(neither > 0);
    sums.trials_both += u32::from(both > 0);
    sums.slowest_end_ms = sums.slowest_end_ms.max(end_ms);
}

fn run(end: End, name: &'static str) {
    if !enabled(name, Duration::from_secs(900)) {
        return;
    }
    let _serial = crate::windows::tests::serial();
    println!();
    let mut variants = 0;
    for timing in &TIMINGS {
        if end == End::DropAfterTimeout && !timing.pump_held {
            continue;
        }
        variants += 1;
        let mut sums = Sums::default();
        for trial in 0..trials() {
            one_trial(end, timing, trial, &mut sums);
        }
        println!(
            "GP summary end={end:?} timing={} trials={} odd_trials={} events_accepted_by_the_system={} delivered={} counted_lost={} coalesced={} in_neither_count={} in_two_counts={} trials_with_an_event_in_neither_count={} trials_with_an_event_in_two_counts={} slowest_end_ms={}",
            timing.name,
            sums.trials,
            sums.odd,
            sums.accepted,
            sums.delivered,
            sums.lost,
            sums.coalesced,
            sums.neither,
            sums.both,
            sums.trials_neither,
            sums.trials_both,
            sums.slowest_end_ms
        );
    }
    println!("GP done end={end:?} variants={variants}");
}

#[test]
fn gate_probe_end_stop() {
    run(End::Stop, "gate_probe_end_stop");
}

#[test]
fn gate_probe_end_shutdown_sink_full() {
    run(End::ShutdownSinkFull, "gate_probe_end_shutdown_sink_full");
}

#[test]
fn gate_probe_end_drop() {
    run(End::Drop, "gate_probe_end_drop");
}

#[test]
fn gate_probe_end_drop_after_timeout() {
    run(End::DropAfterTimeout, "gate_probe_end_drop_after_timeout");
}
