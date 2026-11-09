//! Foreground-app capture via `NSWorkspace`, which also drives window titles.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;

use crate::event::Modality;
use crate::macos::{emit_metadata_only, window_title, ForegroundWriter, Unpersisted};
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

struct Frontmost {
    bundle_id: String,
    pid: libc::pid_t,
}

fn read_frontmost() -> Option<Frontmost> {
    autoreleasepool(|_| {
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
    foreground: Option<ForegroundWriter>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ForegroundSource {
    pub fn new(interval: Duration, foreground: ForegroundWriter) -> Self {
        Self {
            interval,
            foreground: Some(foreground),
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
        let foreground = self.foreground.take().ok_or(CaptureError::Start(
            "the foreground poller was already started".into(),
        ))?;
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

                            let _ = emit_metadata_only(
                                &sink,
                                Modality::ForegroundApp,
                                front.bundle_id.clone(),
                                Unpersisted::EMPTY,
                            );

                            if let Some(title) = window_title::focused_window_title(front.pid) {
                                let _ = emit_metadata_only(
                                    &sink,
                                    Modality::WindowTitle,
                                    front.bundle_id.clone(),
                                    Unpersisted {
                                        byte_len: title.len() as u64,
                                    },
                                );
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
