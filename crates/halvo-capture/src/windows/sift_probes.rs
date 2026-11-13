//! Probes of verifier-interrogate-7. Scratch only, never merged. Each test
//! returns on its first line unless HALVO_SIFT_PROBE names it, so the job's
//! own 12-test runs them as tests that do nothing. A named probe has a time
//! limit of its own and prints `VI7` lines; it asserts nothing.

use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VK_F15,
};

use super::input::RawInputSource;
use super::screen::ScreenSource;
use crate::event::Modality;
use crate::source::{CaptureSource, SourceContext, Started};

fn enabled(name: &'static str, limit: Duration) -> bool {
    if std::env::var("HALVO_SIFT_PROBE").map_or(true, |named| named != name) {
        return false;
    }
    std::thread::spawn(move || {
        std::thread::sleep(limit);
        println!("VI7 {name} time_limit_reached_s={}", limit.as_secs());
        std::process::exit(3);
    });
    true
}

fn f15(released: bool) -> u32 {
    let input = INPUT {
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
    };
    // SAFETY: the slice holds one live INPUT and the size argument is the
    // size of one INPUT, as SendInput requires.
    unsafe { SendInput(&[input], core::mem::size_of::<INPUT>() as i32) }
}

struct Variant {
    name: &'static str,
    gap_ms: u64,
    settle_ms: u64,
    stop_before_the_last_release: bool,
}

/// Adversary 3's finding 1 without its hold: the same five F15 presses and
/// the same `shutdown`, with the pump free to read its queue. Nothing holds
/// the last-app cell. Each variant runs TRIALS times and prints its counts.
#[test]
fn sift_keys_at_a_stop_with_the_pump_not_held() {
    if !enabled(
        "sift_keys_at_a_stop_with_the_pump_not_held",
        Duration::from_secs(300),
    ) {
        return;
    }
    const PRESSES: u64 = 5;
    const TRIALS: u32 = 20;
    let _serial = super::tests::serial();
    let variants = [
        Variant {
            name: "control_stop_300_ms_after_the_last_key",
            gap_ms: 5,
            settle_ms: 300,
            stop_before_the_last_release: false,
        },
        Variant {
            name: "keys_5_ms_apart_stop_right_after_the_last_release",
            gap_ms: 5,
            settle_ms: 0,
            stop_before_the_last_release: false,
        },
        Variant {
            name: "keys_5_ms_apart_stop_right_after_the_last_press",
            gap_ms: 5,
            settle_ms: 0,
            stop_before_the_last_release: true,
        },
        Variant {
            name: "keys_with_no_gap_stop_right_after_the_last_release",
            gap_ms: 0,
            settle_ms: 0,
            stop_before_the_last_release: false,
        },
    ];
    for variant in &variants {
        let gap = Duration::from_millis(variant.gap_ms);
        let (mut sent, mut delivered_all, mut lost_all) = (0u64, 0u64, 0u64);
        let (mut trials_short, mut trials_odd, mut printed) = (0u32, 0u32, 0u32);
        let mut worst_stop_ms = 0u128;
        for trial in 0..TRIALS {
            let (sink, writer) = crate::sink::channel(64);
            let mut source = RawInputSource::keyboard(Arc::new(Mutex::new(None::<String>)));
            let begun = source.begin(SourceContext::new(sink));
            if !matches!(begun, Ok(Started::Running)) {
                println!(
                    "VI7 keys variant={} trial={trial} begin={begun:?}",
                    variant.name
                );
                trials_odd += 1;
                continue;
            }
            std::thread::sleep(Duration::from_millis(200));
            let mut accepted = 0;
            for press in 0..PRESSES {
                accepted += f15(false);
                let last = press + 1 == PRESSES;
                if last && variant.stop_before_the_last_release {
                    break;
                }
                std::thread::sleep(gap);
                accepted += f15(true);
                if !last {
                    std::thread::sleep(gap);
                }
            }
            std::thread::sleep(Duration::from_millis(variant.settle_ms));
            let began = Instant::now();
            let stop = source.shutdown();
            let stop_ms = began.elapsed().as_millis();
            if variant.stop_before_the_last_release {
                accepted += f15(true);
            }
            let mut delivered = 0u64;
            while let Ok(received) = writer.recv_timeout(Duration::from_millis(400)) {
                if received.event().modality == Modality::Keystroke {
                    delivered += 1;
                }
            }
            let lost = source.handoff_counts().lost;
            let odd = accepted != (PRESSES * 2) as u32 || stop != Ok(());
            if odd {
                trials_odd += 1;
            } else {
                sent += PRESSES;
                delivered_all += delivered;
                lost_all += lost;
                worst_stop_ms = worst_stop_ms.max(stop_ms);
                if delivered + lost < PRESSES {
                    trials_short += 1;
                }
            }
            if odd || ((trial < 2 || delivered + lost != PRESSES) && printed < 6) {
                printed += 1;
                println!(
                    "VI7 keys variant={} trial={trial} sendinput_events_accepted={accepted} stop={stop:?} stop_ms={stop_ms} presses={PRESSES} keystrokes_delivered={delivered} lost={lost}",
                    variant.name
                );
            }
        }
        println!(
            "VI7 keys summary variant={} trials={TRIALS} odd_trials={trials_odd} presses_sent={sent} keystrokes_delivered={delivered_all} counted_lost={lost_all} neither_delivered_nor_counted={} trials_with_a_press_neither_delivered_nor_counted={trials_short} slowest_stop_ms={worst_stop_ms}",
            variant.name,
            sent as i64 - delivered_all as i64 - lost_all as i64
        );
    }
    println!("VI7 keys done=true");
}

/// The screen stop whose stop thread cannot be spawned, on the product's own
/// `ScreenSource`. At the head this is the control: the stop thread spawns.
/// Under the temporary change of the second step, `Stopping::start` answers
/// the spawn error and drops the session, as a failed `Builder::spawn` drops
/// its closure, when HALVO_SIFT_FAIL_STOP_SPAWN is set.
#[test]
fn sift_a_screen_stop_whose_stop_thread_cannot_be_spawned() {
    if !enabled(
        "sift_a_screen_stop_whose_stop_thread_cannot_be_spawned",
        Duration::from_secs(120),
    ) {
        return;
    }
    let _serial = super::tests::serial();
    let (sink, writer) = crate::sink::channel(256);
    let mut screen = ScreenSource::new(Arc::default());
    let begun = screen.begin(SourceContext::new(sink.clone()));
    std::thread::sleep(Duration::from_millis(700));
    let while_running = screen.health().state;
    let first_stop = screen.shutdown();
    let after_first_stop = screen.health().state;
    let second_stop = screen.shutdown();
    let second_begin = screen.begin(SourceContext::new(sink.clone()));
    let after_second_begin = screen.health().state;
    std::thread::sleep(Duration::from_millis(700));
    let third_stop = screen.shutdown();
    let lost = screen.handoff_counts().lost;
    drop(screen);
    drop(sink);
    let dropped = Instant::now();
    let mut frames = 0u32;
    let mut closed_ms = None;
    while dropped.elapsed() < Duration::from_secs(8) {
        match writer.recv_timeout(Duration::from_millis(50)) {
            Ok(_) => frames += 1,
            Err(RecvTimeoutError::Disconnected) => {
                closed_ms = Some(dropped.elapsed().as_millis());
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    println!(
        "VI7 spawnfail-real fail_env_set={} begin={begun:?} state_while_running={while_running:?} first_stop={first_stop:?} state_after_first_stop={after_first_stop:?} second_stop={second_stop:?} second_begin={second_begin:?} state_after_second_begin={after_second_begin:?} third_stop={third_stop:?} lost={lost} frames_received={frames} sink_closed_ms_after_the_source_and_the_handles_were_dropped={closed_ms:?}",
        std::env::var_os("HALVO_SIFT_FAIL_STOP_SPAWN").is_some()
    );
}
