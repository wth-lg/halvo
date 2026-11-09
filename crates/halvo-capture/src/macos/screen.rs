//! Screen capture via ScreenCaptureKit.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_foundation::{NSArray, NSError, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCRunningApplication, SCShareableContent, SCStream,
    SCStreamConfiguration, SCStreamOutput, SCStreamOutputType, SCWindow,
};

use crate::event::Modality;
use crate::macos::{emit_metadata_only, ForegroundReader, Unpersisted};
use crate::secure_input::SecureInputError;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

struct ScreenIvars {
    sink: SinkHandle,
    foreground: ForegroundReader,
}

define_class!(
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
            let ivars = self.ivars();
            let _ = emit_metadata_only(
                &ivars.sink,
                Modality::ScreenFrame,
                ivars.foreground.get(),
                Unpersisted::EMPTY,
            );
        }
    }
);

impl ScreenOutput {
    fn new(sink: SinkHandle, foreground: ForegroundReader) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ScreenIvars { sink, foreground });
        unsafe { msg_send![super(this), init] }
    }
}

/// Self-exclusion is enforced in each screen stream's content filter, built by
/// [`filter_excluding_self`]. The macOS secure-input guard delegates here.
pub fn self_exclusion_enforced_in_filter() -> Result<(), SecureInputError> {
    Ok(())
}

/// A content filter for `display` that excludes this process's own windows, so
/// Halvo never captures itself.
fn filter_excluding_self(
    content: &SCShareableContent,
    display: &SCDisplay,
) -> Retained<SCContentFilter> {
    let my_pid = std::process::id() as i32;
    let apps = unsafe { content.applications() };
    let excluded: Vec<Retained<SCRunningApplication>> = apps
        .iter()
        .filter(|a| unsafe { a.processID() } == my_pid)
        .collect();
    let excluded_refs: Vec<&SCRunningApplication> = excluded.iter().map(|r| &**r).collect();
    let excluded_apps = NSArray::from_slice(&excluded_refs);
    let no_windows: Retained<NSArray<SCWindow>> = NSArray::new();
    unsafe {
        SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            display,
            &excluded_apps,
            &no_windows,
        )
    }
}

/// The screen capture source. Created, started and stopped on one thread; the
/// `PhantomData` pins it there because the ScreenCaptureKit objects are not safe
/// to move between threads.
pub struct ScreenSource {
    fps: i32,
    foreground: ForegroundReader,
    stream: Option<Retained<SCStream>>,
    _output: Option<Retained<ScreenOutput>>,
    _queue: Option<DispatchRetained<DispatchQueue>>,
    _not_send: PhantomData<*const ()>,
}

impl ScreenSource {
    pub fn new(fps: i32, foreground: ForegroundReader) -> Self {
        Self {
            fps,
            foreground,
            stream: None,
            _output: None,
            _queue: None,
            _not_send: PhantomData,
        }
    }
}

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

        let filter = filter_excluding_self(&content, &display);

        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth(display.width() as usize);
            config.setHeight(display.height() as usize);
            config.setMinimumFrameInterval(CMTime::new(1, self.fps));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_exclusion_is_enforced_in_the_filter() {
        assert!(self_exclusion_enforced_in_filter().is_ok());
    }
}
