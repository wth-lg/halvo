//! Foreground-app capture via `NSWorkspace`, which also drives window titles.
//!
//! A change-detected poll of `NSWorkspace.frontmostApplication` emits a
//! `foreground_app` event on each real app switch and, on the same change, reads
//! the newly focused window's title (see [`crate::macos::window_title`]) and
//! emits a `window_title` event. One poller owns both signals, so there is no
//! second reader racing the foreground state.
//!
//! The poll is change-detected: one property read at [`crate::macos::FOREGROUND_POLL`]
//! cadence, emitting only on a transition, so a quiet desktop produces no events
//! and costs almost nothing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;

use crate::event::{CaptureStreamEvent, CapturedAt, Modality};
use crate::macos::{unpersisted_placement, window_title, SharedForeground};
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource, EventSink};

/// The current foreground app's bundle id and process id, read from NSWorkspace.
struct Frontmost {
    bundle_id: String,
    pid: libc::pid_t,
}

/// Read the current foreground application. Returns `None` when no app is
/// frontmost (rare: e.g. during fast user switching).
fn read_frontmost() -> Option<Frontmost> {
    autoreleasepool(|_| {
        // NSWorkspace shared accessors read these properties safely; the pool
        // bounds the autoreleased temporaries each call creates.
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let bundle_id = app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let pid = app.processIdentifier();
        Some(Frontmost { bundle_id, pid })
    })
}

/// The foreground-app source (also emits window-title events on each change).
pub struct ForegroundSource {
    interval: Duration,
    foreground: SharedForeground,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ForegroundSource {
    pub fn new(interval: Duration, foreground: SharedForeground) -> Self {
        Self {
            interval,
            foreground,
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }
}

impl CaptureSource for ForegroundSource {
    fn modality(&self) -> Modality {
        Modality::ForegroundApp
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let stop = self.stop.clone();
        let foreground = self.foreground.clone();
        let interval = self.interval;

        let thread = std::thread::Builder::new()
            .name("halvo-foreground".into())
            .spawn(move || {
                let mut last_bundle = String::new();
                while !stop.load(Ordering::Relaxed) {
                    if let Some(front) = read_frontmost() {
                        if front.bundle_id != last_bundle {
                            last_bundle = front.bundle_id.clone();
                            foreground.set(front.bundle_id.clone());

                            // foreground_app event.
                            let (segment_ref, segment_key_id) = unpersisted_placement(0);
                            let event = CaptureStreamEvent::new_raw(
                                CapturedAt {
                                    wall_clock: SystemTime::now(),
                                    sequence: sink.next_sequence(),
                                },
                                Modality::ForegroundApp,
                                front.bundle_id.clone(),
                                segment_ref,
                                segment_key_id,
                            );
                            let _ = sink.emit(event);

                            // window_title event on the same change, if the title
                            // is readable (needs Accessibility).
                            if let Some(title) = window_title::focused_window_title(front.pid) {
                                let (segment_ref, segment_key_id) =
                                    unpersisted_placement(title.len() as u64);
                                let event = CaptureStreamEvent::new_raw(
                                    CapturedAt {
                                        wall_clock: SystemTime::now(),
                                        sequence: sink.next_sequence(),
                                    },
                                    Modality::WindowTitle,
                                    front.bundle_id.clone(),
                                    segment_ref,
                                    segment_key_id,
                                );
                                let _ = sink.emit(event);
                            }
                        }
                    }
                    std::thread::sleep(interval);
                }
            })
            .map_err(|e| CaptureError::Start(format!("foreground thread: {e}")))?;

        self.thread = Some(thread);
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
