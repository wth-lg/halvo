//! Screen capture via ScreenCaptureKit.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_foundation::{NSArray, NSError, NSInteger, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCRunningApplication, SCShareableContent, SCStream,
    SCStreamConfiguration, SCStreamOutput, SCStreamOutputType, SCWindow,
};

use crate::event::Modality;
use crate::macos::foreground::{emit_metadata_only, ForegroundReader, Unpersisted};
use crate::secure_input::SecureInputError;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

/// A cap on the screen stream's frame rate. No value of this type allows more
/// than [`FrameRateCap::MAX_PER_SECOND`] frames a second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRateCap(i32);

impl FrameRateCap {
    /// The most frames a second that a cap allows.
    pub const MAX_PER_SECOND: i32 = 2;

    const fn per_second(self) -> i32 {
        self.0
    }
}

/// The cap on the screen stream's frame rate.
pub const SCREEN_FRAME_CAP: FrameRateCap = FrameRateCap(FrameRateCap::MAX_PER_SECOND);

const STREAM_QUEUE_DEPTH: NSInteger = 8;

struct ScreenIvars {
    sink: Mutex<Option<SinkHandle>>,
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
        fn sample_buffer_arrived(
            &self,
            _stream: &SCStream,
            sample: &CMSampleBuffer,
            ty: SCStreamOutputType,
        ) {
            on_sample(self.ivars(), sample, ty);
        }
    }
);

fn on_sample(ivars: &ScreenIvars, _sample: &CMSampleBuffer, ty: SCStreamOutputType) {
    if ty != SCStreamOutputType::Screen {
        return;
    }
    let sink = ivars.sink.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(sink) = sink.as_ref() else {
        return;
    };
    let _ = emit_metadata_only(
        sink,
        Modality::ScreenFrame,
        ivars.foreground.get(),
        Unpersisted::EMPTY,
    );
}

impl ScreenOutput {
    fn new(sink: SinkHandle, foreground: ForegroundReader) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ScreenIvars {
            sink: Mutex::new(Some(sink)),
            foreground,
        });
        // SAFETY: `this` is a fresh allocation of this class with its ivars set,
        // and `init` is NSObject's initializer: it takes no argument and returns
        // the object it was sent to.
        unsafe { msg_send![super(this), init] }
    }
}

/// Self-exclusion is enforced in each screen stream's content filter, built by
/// [`filter_excluding_self`]. The macOS secure-input guard delegates here.
pub fn self_exclusion_enforced_in_filter() -> Result<(), SecureInputError> {
    Ok(())
}

fn is_own_process(pid: libc::pid_t) -> bool {
    pid == std::process::id() as libc::pid_t
}

fn excluded_applications<T>(
    apps: impl IntoIterator<Item = T>,
    pid_of: impl Fn(&T) -> libc::pid_t,
) -> Vec<T> {
    apps.into_iter()
        .filter(|app| is_own_process(pid_of(app)))
        .collect()
}

fn filter_excluding_self(
    content: &SCShareableContent,
    display: &SCDisplay,
) -> Retained<SCContentFilter> {
    // SAFETY: `content` is a live SCShareableContent that the caller holds, and
    // `applications` is a property getter that takes no argument.
    let apps = unsafe { content.applications() };
    let excluded = excluded_applications(apps.iter(), |app| {
        // SAFETY: `app` is a retained element of `apps`, and `processID` is a
        // property getter that takes no argument.
        unsafe { app.processID() }
    });
    let excluded_refs: Vec<&SCRunningApplication> = excluded.iter().map(|r| &**r).collect();
    let excluded_apps = NSArray::from_slice(&excluded_refs);
    let no_windows: Retained<NSArray<SCWindow>> = NSArray::new();
    // SAFETY: the receiver is a fresh allocation of SCContentFilter; `display`
    // and the two arrays live past the call, and the arrays hold the element
    // types that the initializer declares.
    unsafe {
        SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            display,
            &excluded_apps,
            &no_windows,
        )
    }
}

fn stream_configuration(
    width: usize,
    height: usize,
    cap: FrameRateCap,
) -> Retained<SCStreamConfiguration> {
    // SAFETY: `new` allocates and initializes a default configuration and
    // takes no argument.
    let config = unsafe { SCStreamConfiguration::new() };
    // SAFETY: the setters run on a configuration that only this function
    // holds, before a stream reads it; `CMTime::new` is CMTimeMake, and the
    // interval that it makes has a positive timescale.
    unsafe {
        config.setWidth(width);
        config.setHeight(height);
        config.setMinimumFrameInterval(CMTime::new(1, cap.per_second()));
        config.setQueueDepth(STREAM_QUEUE_DEPTH);
    }
    config
}

/// The screen capture source. Created, started and stopped on one thread: the
/// type is not `Send`.
///
/// ```
/// fn on_one_thread<T>() {}
/// on_one_thread::<halvo_capture::macos::screen::ScreenSource>();
/// ```
///
/// ```compile_fail
/// fn across_threads<T: Send>() {}
/// across_threads::<halvo_capture::macos::screen::ScreenSource>();
/// ```
pub struct ScreenSource {
    cap: FrameRateCap,
    foreground: ForegroundReader,
    stream: Option<Retained<SCStream>>,
    output: Option<Retained<ScreenOutput>>,
    queue: Option<DispatchRetained<DispatchQueue>>,
    _not_send: PhantomData<*const ()>,
}

impl ScreenSource {
    pub fn new(cap: FrameRateCap, foreground: ForegroundReader) -> Self {
        Self {
            cap,
            foreground,
            stream: None,
            output: None,
            queue: None,
            _not_send: PhantomData,
        }
    }
}

fn fetch_shareable_content() -> Option<Retained<SCShareableContent>> {
    let (tx, rx): (Sender<Option<Retained<SCShareableContent>>>, _) = channel();
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, _err: *mut NSError| {
            let retained = if content.is_null() {
                None
            } else {
                // SAFETY: `content` is not null, and ScreenCaptureKit hands the
                // handler a +0 reference to a live SCShareableContent; retaining
                // it takes ownership.
                unsafe { Retained::retain(content) }
            };
            let _ = tx.send(retained);
        },
    );
    // SAFETY: the handler has the completion signature
    // `void (^)(SCShareableContent *, NSError *)`. ScreenCaptureKit calls it on
    // a queue of its own, possibly after the 10 s wait below gave up. It holds
    // nothing but `tx`; a send to a receiver that is gone returns an error,
    // which the handler ignores, and the retained content drops with it. The
    // content it sends crosses to the waiting thread, although neither objc2
    // nor SCShareableContent.h marks the class as safe to share between threads.
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

        // SAFETY: `content` is a live, retained SCShareableContent, and
        // `displays` is a property getter that takes no argument.
        let displays = unsafe { content.displays() };
        let display = displays
            .iter()
            .next()
            .ok_or_else(|| CaptureError::Start("no display to capture".into()))?;

        let filter = filter_excluding_self(&content, &display);

        // SAFETY: `display` is a live, retained SCDisplay, and `width` and
        // `height` are property getters that take no argument.
        let (width, height) = unsafe { (display.width() as usize, display.height() as usize) };
        let config = stream_configuration(width, height, self.cap);

        let output = ScreenOutput::new(sink, self.foreground.clone());
        // SAFETY: the receiver is a fresh allocation of SCStream; `filter` and
        // `config` live past the call, and the delegate may be nil.
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
        // SAFETY: `stream`, `output` (which `proto` borrows) and the serial
        // dispatch queue `queue` are live for the call. SCStream.h says that the
        // start and the stop completions each carry an error that tells whether
        // the start or the stop failed. This source releases `output` and
        // `queue` only when the stream cannot run: after this call fails, after
        // the start completion reports an error, or after the stop completion
        // reports none. A start or a stop whose completion does not arrive within
        // 5 s, or a stop that reports an error, keeps both for the life of the
        // process, and the output gives back its sink handle first. Not held
        // here: a second `start` without `stop`, or dropping a started source,
        // releases them while the stream may run (RR-macos-sources-07).
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
        // SAFETY: `stream` is live, and the handler has the completion signature
        // `void (^)(NSError *)`; it only stores a flag and sends on a channel.
        unsafe { stream.startCaptureWithCompletionHandler(Some(&handler)) };
        let completed = srx.recv_timeout(Duration::from_secs(5)).is_ok();
        if !started.load(Ordering::Relaxed) {
            if !completed {
                keep_for_the_life_of_the_process(Some(output), Some(queue));
            }
            return Err(CaptureError::Start("startCapture reported an error".into()));
        }

        self.stream = Some(stream);
        self.output = Some(output);
        self.queue = Some(queue);
        Ok(())
    }

    fn stop(&mut self) {
        let stopped = match self.stream.take() {
            None => true,
            Some(stream) => {
                let (tx, rx) = channel();
                let handler = RcBlock::new(move |err: *mut NSError| {
                    let _ = tx.send(err.is_null());
                });
                // SAFETY: `stream` is live, and the handler has the completion
                // signature `void (^)(NSError *)`; it only sends on a channel
                // whether the error is nil.
                unsafe { stream.stopCaptureWithCompletionHandler(Some(&handler)) };
                stopped_without_error(rx.recv_timeout(Duration::from_secs(5)))
            }
        };
        if stopped {
            self.output = None;
            self.queue = None;
        } else {
            keep_for_the_life_of_the_process(self.output.take(), self.queue.take());
        }
    }
}

fn stopped_without_error(completion: Result<bool, RecvTimeoutError>) -> bool {
    completion == Ok(true)
}

fn keep_for_the_life_of_the_process(
    output: Option<Retained<ScreenOutput>>,
    queue: Option<DispatchRetained<DispatchQueue>>,
) {
    if let Some(output) = &output {
        drop(
            output
                .ivars()
                .sink
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }
    std::mem::forget(output);
    std::mem::forget(queue);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos::foreground::foreground_state;
    use crate::sink::{self, SinkWriter};
    use core_foundation::base::{CFGetRetainCount, CFRelease};
    use objc2::rc::{Allocated, Weak};
    use objc2_core_media::{CMSampleTimingInfo, CMTimeFlags};
    use std::ffi::c_void;

    #[test]
    fn self_exclusion_is_enforced_in_the_filter() {
        assert!(self_exclusion_enforced_in_filter().is_ok());
    }

    #[test]
    fn only_the_own_process_is_excluded_and_every_other_entry_is_kept() {
        let own = std::process::id() as libc::pid_t;
        let apps = [
            (own + 1, "com.example.editor"),
            (own, ""),
            (1, "com.example.finder"),
            (own + 2, ""),
        ];
        let excluded = excluded_applications(apps, |&(pid, _)| pid);
        assert_eq!(excluded, [(own, "")]);
    }

    #[test]
    fn the_stream_asks_for_two_frames_a_second_the_named_depth_and_no_audio() {
        let config = stream_configuration(640, 480, SCREEN_FRAME_CAP);
        // SAFETY: property getters that take no argument, on a live configuration.
        let (interval, depth, audio, microphone, size) = unsafe {
            (
                config.minimumFrameInterval(),
                config.queueDepth(),
                config.capturesAudio(),
                config.captureMicrophone(),
                (config.width(), config.height()),
            )
        };
        assert_eq!(
            (interval.value, interval.timescale),
            (1, 2),
            "minimum frame interval"
        );
        assert_eq!(depth, STREAM_QUEUE_DEPTH);
        assert!(!audio, "the stream captures system audio");
        assert!(!microphone, "the stream captures the microphone");
        assert_eq!(size, (640, 480));
    }

    #[link(name = "CoreVideo", kind = "framework")]
    extern "C" {
        fn CVPixelBufferCreate(
            allocator: *const c_void,
            width: usize,
            height: usize,
            pixel_format: u32,
            attributes: *const c_void,
            buffer: *mut *mut c_void,
        ) -> i32;
    }

    #[link(name = "CoreMedia", kind = "framework")]
    extern "C" {
        fn CMVideoFormatDescriptionCreateForImageBuffer(
            allocator: *const c_void,
            image: *mut c_void,
            format: *mut *mut c_void,
        ) -> i32;
        fn CMSampleBufferCreateReadyWithImageBuffer(
            allocator: *const c_void,
            image: *mut c_void,
            format: *mut c_void,
            timing: *const CMSampleTimingInfo,
            sample: *mut *mut c_void,
        ) -> i32;
    }

    const PIXEL_FORMAT_BGRA: u32 = 0x4247_5241;

    struct Frame {
        image: *mut c_void,
        sample: *mut c_void,
    }

    impl Frame {
        fn new(width: usize, height: usize) -> Self {
            let mut image = std::ptr::null_mut();
            // SAFETY: a null allocator and null attributes are allowed, and
            // `image` is a valid place for the new pixel buffer.
            let status = unsafe {
                CVPixelBufferCreate(
                    std::ptr::null(),
                    width,
                    height,
                    PIXEL_FORMAT_BGRA,
                    std::ptr::null(),
                    &mut image,
                )
            };
            assert_eq!(status, 0, "CVPixelBufferCreate");
            let mut format = std::ptr::null_mut();
            // SAFETY: `image` is the live pixel buffer made above, and `format`
            // is a valid place for the new description.
            let status = unsafe {
                CMVideoFormatDescriptionCreateForImageBuffer(std::ptr::null(), image, &mut format)
            };
            assert_eq!(status, 0, "CMVideoFormatDescriptionCreateForImageBuffer");
            let invalid = CMTime {
                value: 0,
                timescale: 0,
                flags: CMTimeFlags(0),
                epoch: 0,
            };
            let timing = CMSampleTimingInfo {
                duration: invalid,
                presentationTimeStamp: CMTime {
                    value: 0,
                    timescale: 2,
                    flags: CMTimeFlags::Valid,
                    epoch: 0,
                },
                decodeTimeStamp: invalid,
            };
            let mut sample = std::ptr::null_mut();
            // SAFETY: `image` and `format` are live and `format` describes
            // `image`; `timing` lives past the call, and `sample` is a valid
            // place for the new sample buffer.
            let status = unsafe {
                CMSampleBufferCreateReadyWithImageBuffer(
                    std::ptr::null(),
                    image,
                    format,
                    &timing,
                    &mut sample,
                )
            };
            // SAFETY: `format` is the +1 reference that the create call above
            // returned; the sample buffer holds a reference of its own.
            unsafe { CFRelease(format) };
            assert_eq!(status, 0, "CMSampleBufferCreateReadyWithImageBuffer");
            Self { image, sample }
        }

        fn sample(&self) -> &CMSampleBuffer {
            // SAFETY: `sample` is a live CMSampleBuffer that this frame owns
            // until it drops, and the reference borrows the frame.
            unsafe { &*(self.sample as *const CMSampleBuffer) }
        }

        fn retain_count(&self) -> isize {
            // SAFETY: `sample` is a live CF object that this frame owns.
            unsafe { CFGetRetainCount(self.sample) }
        }
    }

    impl Drop for Frame {
        fn drop(&mut self) {
            // SAFETY: both are +1 references that `new` created and that this
            // frame owns alone.
            unsafe {
                CFRelease(self.sample);
                CFRelease(self.image);
            }
        }
    }

    fn deliver(output: &ScreenOutput, sample: &CMSampleBuffer) {
        let stream = SCStream::alloc();
        let stream_ptr = Allocated::as_ptr(&stream);
        // SAFETY: ScreenOutput implements this selector with these argument
        // types; the method never reads its stream argument, which points to an
        // allocated SCStream that stays allocated, and `sample` is live for the
        // call.
        unsafe {
            let _: () = msg_send![
                output,
                stream: stream_ptr,
                didOutputSampleBuffer: sample,
                ofType: SCStreamOutputType::Screen
            ];
        }
        std::mem::forget(stream);
    }

    #[test]
    fn a_kept_output_stays_alive() {
        let (handle, _writer) = sink::channel(8);
        let (_fg_writer, reader) = foreground_state();
        let output = ScreenOutput::new(handle, reader);
        let weak = Weak::from_retained(&output);
        keep_for_the_life_of_the_process(Some(output), None);
        assert!(weak.load().is_some(), "the kept output was released");
    }

    #[test]
    fn a_kept_output_lets_the_drain_end() {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state();
        let output = ScreenOutput::new(handle, reader);
        keep_for_the_life_of_the_process(Some(output), Some(DispatchQueue::new("kept", None)));
        assert_eq!(
            writer
                .recv_timeout(Duration::from_secs(1))
                .map(|received| received.event().modality),
            Err(RecvTimeoutError::Disconnected),
            "the kept output still holds a handle of the sink"
        );
    }

    #[test]
    fn a_sample_that_reaches_a_kept_output_emits_nothing() {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state();
        let output = ScreenOutput::new(handle.clone(), reader);
        keep_for_the_life_of_the_process(Some(output.clone()), None);
        let frame = Frame::new(16, 16);
        deliver(&output, frame.sample());
        drop(handle);
        assert_eq!(
            writer
                .recv_timeout(Duration::from_secs(1))
                .map(|received| received.event().modality),
            Err(RecvTimeoutError::Disconnected),
            "a kept output emitted an event or still holds a handle"
        );
    }

    #[test]
    fn the_stream_output_message_gives_exactly_one_screen_frame_event() {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state();
        let output = ScreenOutput::new(handle, reader);
        let frame = Frame::new(16, 16);
        deliver(&output, frame.sample());
        drop(output);
        let kinds: Vec<Modality> = std::iter::from_fn(|| writer.recv())
            .map(|event| event.modality)
            .collect();
        assert_eq!(kinds, [Modality::ScreenFrame]);
    }

    #[test]
    fn the_stream_output_message_keeps_no_reference_to_the_sample() {
        let (handle, _sink_writer) = sink::channel(8);
        let (_writer, reader) = foreground_state();
        let output = ScreenOutput::new(handle, reader);
        let frame = Frame::new(16, 16);
        let before = frame.retain_count();
        deliver(&output, frame.sample());
        assert_eq!(frame.retain_count(), before);
    }

    #[test]
    fn only_a_stop_completion_without_error_releases_the_output() {
        assert!(stopped_without_error(Ok(true)));
        assert!(!stopped_without_error(Ok(false)));
        assert!(!stopped_without_error(Err(RecvTimeoutError::Timeout)));
        assert!(!stopped_without_error(Err(RecvTimeoutError::Disconnected)));
    }

    fn screen_ivars() -> (ScreenIvars, SinkWriter) {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state();
        let ivars = ScreenIvars {
            sink: Mutex::new(Some(handle)),
            foreground: reader,
        };
        (ivars, writer)
    }

    #[test]
    fn a_complete_frame_gives_exactly_one_screen_frame_event() {
        let (ivars, writer) = screen_ivars();
        let frame = Frame::new(16, 16);
        on_sample(&ivars, frame.sample(), SCStreamOutputType::Screen);
        drop(ivars);
        let kinds: Vec<Modality> = std::iter::from_fn(|| writer.recv())
            .map(|event| event.modality)
            .collect();
        assert_eq!(kinds, [Modality::ScreenFrame]);
    }

    #[test]
    fn the_frame_handler_keeps_no_reference_to_the_sample() {
        let (ivars, _writer) = screen_ivars();
        let frame = Frame::new(16, 16);
        let before = frame.retain_count();
        on_sample(&ivars, frame.sample(), SCStreamOutputType::Screen);
        assert_eq!(frame.retain_count(), before);
    }
}
