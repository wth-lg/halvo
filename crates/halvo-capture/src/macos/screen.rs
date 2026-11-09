//! Screen capture via ScreenCaptureKit.
//!
//! An `SCStream` delivers frames to a listen-only `SCStreamOutput` delegate on a
//! dedicated dispatch queue. The stream is frame-rate throttled
//! ([`SCStreamConfiguration::setMinimumFrameInterval`]) and the delegate reads
//! nothing from the sample buffer, so no frame is buffered beyond the bounded
//! sink and resident memory stays flat (see the module memory note).
//!
//! Self-exclusion is by construction: the `SCContentFilter` excludes Halvo's own
//! running application, so a captured frame can never contain a Halvo window.
//! ScreenCaptureKit additionally yields no content for windows the owner marks
//! protected and for DRM media, so password windows and protected video are
//! excluded by the framework, not by Halvo policy.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_foundation::{NSArray, NSError, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCRunningApplication, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamOutput, SCStreamOutputType, SCWindow,
};

use crate::event::{CaptureStreamEvent, CapturedAt, Modality};
use crate::macos::{unpersisted_placement, SharedForeground};
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource, EventSink};

/// Ivars the frame delegate carries: the emit path and the shared foreground app.
struct ScreenIvars {
    sink: SinkHandle,
    foreground: SharedForeground,
}

define_class!(
    // A listen-only screen-capture output. It observes frames and emits a
    // metadata event per frame; it never writes back to the stream.
    #[unsafe(super(NSObject))]
    #[name = "HalvoScreenOutput"]
    #[ivars = ScreenIvars]
    struct ScreenOutput;

    unsafe impl NSObjectProtocol for ScreenOutput {}

    unsafe impl SCStreamOutput for ScreenOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(&self, _stream: &SCStream, _sample: &CMSampleBuffer, ty: SCStreamOutputType) {
            if ty != SCStreamOutputType::Screen {
                return;
            }
            // Record that a frame occurred; read nothing out of the buffer, so it
            // is released as soon as this returns. byte_len is 0 because no pixels
            // are persisted in this phase.
            let ivars = self.ivars();
            let (segment_ref, segment_key_id) = unpersisted_placement(0);
            let event = CaptureStreamEvent::new_raw(
                CapturedAt {
                    wall_clock: SystemTime::now(),
                    sequence: ivars.sink.next_sequence(),
                },
                Modality::ScreenFrame,
                ivars.foreground.get(),
                segment_ref,
                segment_key_id,
            );
            let _ = ivars.sink.emit(event);
        }
    }
);

impl ScreenOutput {
    fn new(sink: SinkHandle, foreground: SharedForeground) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ScreenIvars { sink, foreground });
        unsafe { msg_send![super(this), init] }
    }
}

/// The screen capture source. Holds the live stream, its delegate and the sample
/// queue for the duration of a run. Not `Send`: it is created, started and
/// stopped on one thread (the ScreenCaptureKit objects are not thread-safe to
/// move), which the timed run honors.
pub struct ScreenSource {
    fps: i32,
    foreground: SharedForeground,
    stream: Option<Retained<SCStream>>,
    _output: Option<Retained<ScreenOutput>>,
    _queue: Option<DispatchRetained<DispatchQueue>>,
}

impl ScreenSource {
    pub fn new(fps: i32, foreground: SharedForeground) -> Self {
        Self {
            fps,
            foreground,
            stream: None,
            _output: None,
            _queue: None,
        }
    }
}

/// Fetch the shareable content (displays, windows, running apps) through the
/// async completion-handler API, blocking briefly for the result.
fn fetch_shareable_content() -> Option<Retained<SCShareableContent>> {
    let (tx, rx): (Sender<Option<Retained<SCShareableContent>>>, _) = channel();
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, _err: *mut NSError| {
            // SAFETY: ScreenCaptureKit hands us a +0 reference; retain it to own it.
            let retained = if content.is_null() {
                None
            } else {
                unsafe { Retained::retain(content) }
            };
            let _ = tx.send(retained);
        },
    );
    // SAFETY: the handler matches the documented completion-handler signature.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&handler) };
    rx.recv_timeout(Duration::from_secs(10)).ok().flatten()
}

impl CaptureSource for ScreenSource {
    fn modality(&self) -> Modality {
        Modality::ScreenFrame
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let content = fetch_shareable_content()
            .ok_or(CaptureError::PermissionDenied(Modality::ScreenFrame))?;

        let displays = unsafe { content.displays() };
        let display = displays
            .iter()
            .next()
            .ok_or_else(|| CaptureError::Start("no display to capture".into()))?;

        // Self-exclusion: exclude Halvo's own running application from the frame.
        let my_pid = std::process::id() as i32;
        let apps = unsafe { content.applications() };
        let excluded: Vec<Retained<SCRunningApplication>> = apps
            .iter()
            .filter(|a| unsafe { a.processID() } == my_pid)
            .collect();
        let excluded_refs: Vec<&SCRunningApplication> = excluded.iter().map(|r| &**r).collect();
        let excluded_apps = NSArray::from_slice(&excluded_refs);
        let no_windows: Retained<NSArray<SCWindow>> = NSArray::new();

        let filter = unsafe {
            SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
                SCContentFilter::alloc(),
                &display,
                &excluded_apps,
                &no_windows,
            )
        };

        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth(display.width() as usize);
            config.setHeight(display.height() as usize);
            // The frame-rate throttle: at most `fps` frames per second.
            config.setMinimumFrameInterval(CMTime::new(1, self.fps));
            // A shallow frame-pool depth; the delegate drains instantly, so a
            // deep queue would only hold pixels we never read.
            config.setQueueDepth(3);
        }

        let output = ScreenOutput::new(sink, self.foreground.clone());
        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &config,
                None,
            )
        };
        let queue = DispatchQueue::new("com.halvo.capture.screen", None);
        let proto: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*output);
        unsafe {
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(
                    proto,
                    SCStreamOutputType::Screen,
                    Some(&queue),
                )
                .map_err(|e| CaptureError::Start(format!("addStreamOutput failed: {e:?}")))?;
        }

        // Start capture and wait for the completion handler to confirm.
        let started = Arc::new(AtomicBool::new(false));
        let (stx, srx) = channel();
        let handler = {
            let started = started.clone();
            RcBlock::new(move |err: *mut NSError| {
                started.store(err.is_null(), Ordering::Relaxed);
                let _ = stx.send(());
            })
        };
        unsafe { stream.startCaptureWithCompletionHandler(Some(&handler)) };
        let _ = srx.recv_timeout(Duration::from_secs(5));
        if !started.load(Ordering::Relaxed) {
            return Err(CaptureError::Start("startCapture reported an error".into()));
        }

        self.stream = Some(stream);
        self._output = Some(output);
        self._queue = Some(queue);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            let (tx, rx) = channel();
            let handler = RcBlock::new(move |_err: *mut NSError| {
                let _ = tx.send(());
            });
            unsafe { stream.stopCaptureWithCompletionHandler(Some(&handler)) };
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
        self._output = None;
        self._queue = None;
    }
}
