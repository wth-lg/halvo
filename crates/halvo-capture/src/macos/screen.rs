use std::marker::PhantomData;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

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
use crate::macos::source_failure;
use crate::sink::SinkHandle;
use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, SourceState, Started,
    StopCause,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRateCap(i32);

impl FrameRateCap {
    pub const MAX_PER_SECOND: i32 = 2;

    const fn per_second(self) -> i32 {
        self.0
    }
}

pub const SCREEN_FRAME_CAP: FrameRateCap = FrameRateCap(FrameRateCap::MAX_PER_SECOND);

const _: () = assert!(
    SCREEN_FRAME_CAP.per_second() > 0
        && SCREEN_FRAME_CAP.per_second() <= FrameRateCap::MAX_PER_SECOND
);

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

/// The deadline of the screen source's `shutdown` (contract C12 item 7).
pub const SCREEN_STOP_DEADLINE: Duration = Duration::from_secs(6);

/// The part of the deadline that the stop keeps after its wait gives up.
pub const SCREEN_STOP_MARGIN: Duration = Duration::from_secs(1);

/// How long the stop waits for the stream's stop completion: the deadline less
/// the margin.
pub const SCREEN_STOP_WAIT: Duration = SCREEN_STOP_DEADLINE.saturating_sub(SCREEN_STOP_MARGIN);

const SCREEN_START_WAIT: Duration = Duration::from_secs(5);

const SHAREABLE_CONTENT_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReply {
    Stopped,
    Failed { code: i64 },
}

trait StopStream {
    fn ask_stop(&self, reply: Sender<StopReply>);
}

impl StopStream for Retained<SCStream> {
    fn ask_stop(&self, reply: Sender<StopReply>) {
        let handler = RcBlock::new(move |err: *mut NSError| {
            let answer = if err.is_null() {
                StopReply::Stopped
            } else {
                // SAFETY: `err` is not null: a live NSError that the completion
                // hands the handler for the length of the call; `code` is a
                // property getter that takes no argument.
                let code = unsafe { (*err).code() } as i64;
                StopReply::Failed { code }
            };
            let _ = reply.send(answer);
        });
        // SAFETY: the stream is live, and the handler has the completion
        // signature `void (^)(NSError *)`.
        unsafe { self.stopCaptureWithCompletionHandler(Some(&handler)) };
    }
}

type Opened = (
    Box<dyn StopStream>,
    Retained<ScreenOutput>,
    DispatchRetained<DispatchQueue>,
);

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
    stream: Option<Box<dyn StopStream>>,
    output: Option<Retained<ScreenOutput>>,
    queue: Option<DispatchRetained<DispatchQueue>>,
    failure: Option<CaptureError>,
    stopped: bool,
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
            failure: None,
            stopped: false,
            _not_send: PhantomData,
        }
    }

    fn begin_with(
        &mut self,
        sink: SinkHandle,
        open: impl FnOnce(SinkHandle, ForegroundReader, FrameRateCap) -> Result<Opened, CaptureError>,
    ) -> Result<Started, CaptureError> {
        if self.stream.is_some() {
            return Err(CaptureError::AlreadyStarted);
        }
        match open(sink, self.foreground.clone(), self.cap) {
            Ok((stream, output, queue)) => {
                self.stream = Some(stream);
                self.output = Some(output);
                self.queue = Some(queue);
                self.failure = None;
                self.stopped = false;
                Ok(Started::Running)
            }
            Err(error) => {
                self.failure = Some(error.clone());
                Err(error)
            }
        }
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

fn screen_access_denied() -> bool {
    // SAFETY: a parameterless predicate of the system with no side effect.
    !unsafe { CGPreflightScreenCaptureAccess() }
}

fn content_refusal(denied: bool, answer: Result<i64, RecvTimeoutError>) -> CaptureError {
    match (denied, answer) {
        (true, _) => CaptureError::PermissionDenied(Modality::ScreenFrame),
        (false, Ok(code)) => CaptureError::Os {
            stage: "shareable_content",
            domain: "ScreenCaptureKit",
            code,
        },
        (false, Err(_)) => CaptureError::Timeout {
            stage: "shareable_content",
            ms: SHAREABLE_CONTENT_WAIT.as_millis() as u64,
        },
    }
}

fn fetch_shareable_content() -> Result<Retained<SCShareableContent>, CaptureError> {
    let (tx, rx): (Sender<Result<Retained<SCShareableContent>, i64>>, _) = channel();
    let handler = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let answer = if content.is_null() && err.is_null() {
            Err(0)
        } else if content.is_null() {
            // SAFETY: `err` is not null: a live NSError that the completion
            // hands the handler for the length of the call; `code` is a
            // property getter that takes no argument.
            Err(unsafe { (*err).code() } as i64)
        } else {
            // SAFETY: `content` is not null, and ScreenCaptureKit hands the
            // handler a +0 reference to a live SCShareableContent; retaining
            // it takes ownership.
            unsafe { Retained::retain(content) }.ok_or(0)
        };
        let _ = tx.send(answer);
    });
    // SAFETY: the handler has the completion signature
    // `void (^)(SCShareableContent *, NSError *)`. ScreenCaptureKit calls it on
    // a queue of its own, possibly after the wait below gave up. It holds
    // nothing but `tx`; a send to a receiver that is gone returns an error,
    // which the handler ignores, and the retained content drops with it. The
    // content is retained on ScreenCaptureKit's queue, handed over once through
    // the channel, and used by one thread at a time. That rests on what is
    // assumed: one owner at a time, no use on two threads at once, and no
    // documented tie of the object to the queue that delivered it. The SDK
    // marks nothing: neither objc2 nor SCShareableContent.h nor the
    // framework's Swift interface says that the class may be sent between
    // threads. This is a named limit and not a proof; macos-3 takes the
    // hand-over away.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&handler) };
    match rx.recv_timeout(SHAREABLE_CONTENT_WAIT) {
        Ok(Ok(content)) => Ok(content),
        Ok(Err(code)) => Err(content_refusal(screen_access_denied(), Ok(code))),
        Err(waited) => Err(content_refusal(screen_access_denied(), Err(waited))),
    }
}

fn open_stream(
    sink: SinkHandle,
    foreground: ForegroundReader,
    cap: FrameRateCap,
) -> Result<Opened, CaptureError> {
    let content = fetch_shareable_content()?;

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
    let config = stream_configuration(width, height, cap);

    let output = ScreenOutput::new(sink, foreground);
    // SAFETY: the receiver is a fresh allocation of SCStream; `filter` and
    // `config` live past the call, and the delegate may be nil.
    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &config, None)
    };
    let queue = DispatchQueue::new("com.halvo.capture.screen", None);
    let proto: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*output);
    // SAFETY: `stream`, `output` (which `proto` borrows) and the serial
    // dispatch queue `queue` are live for the call. SCStream.h says that the
    // start and the stop completions each carry an error that tells whether
    // the start or the stop failed. This source releases `output` and `queue`
    // only when the stream cannot run: after this call fails, after the start
    // completion reports an error, or after the stop completion reports none.
    // A start or a stop whose completion does not arrive in time, or a stop
    // that reports an error, keeps both for the life of the process, and the
    // output gives back its sink handle first. A second start answers
    // AlreadyStarted, and dropping a started source stops it first.
    unsafe {
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(
                proto,
                SCStreamOutputType::Screen,
                Some(&queue),
            )
            .map_err(|e| CaptureError::Os {
                stage: "add_stream_output",
                domain: "ScreenCaptureKit",
                code: e.code() as i64,
            })?;
    }

    let (stx, srx) = channel();
    let handler = RcBlock::new(move |err: *mut NSError| {
        // SAFETY: a non-null `err` is a live NSError that the completion hands
        // the handler for the length of the call; `code` is a property getter
        // that takes no argument.
        let code = (!err.is_null()).then(|| unsafe { (*err).code() } as i64);
        let _ = stx.send(code);
    });
    // SAFETY: `stream` is live, and the handler has the completion signature
    // `void (^)(NSError *)`.
    unsafe { stream.startCaptureWithCompletionHandler(Some(&handler)) };
    match srx.recv_timeout(SCREEN_START_WAIT) {
        Ok(None) => Ok((Box::new(stream), output, queue)),
        Ok(Some(code)) => Err(CaptureError::Os {
            stage: "start_capture",
            domain: "ScreenCaptureKit",
            code,
        }),
        Err(_) => {
            stream.ask_stop(channel().0);
            keep_for_the_life_of_the_process(Some(output), Some(queue));
            Err(CaptureError::Timeout {
                stage: "start_capture",
                ms: SCREEN_START_WAIT.as_millis() as u64,
            })
        }
    }
}

impl CaptureSource for ScreenSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        self.begin_with(ctx.sink().clone(), open_stream)
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let Some(stream) = self.stream.as_ref() else {
            return Ok(());
        };
        let (tx, rx) = channel();
        let asked = Instant::now();
        stream.ask_stop(tx);
        let mut completion = rx.recv_timeout(SCREEN_STOP_WAIT.saturating_sub(asked.elapsed()));
        if completion == Err(RecvTimeoutError::Disconnected) {
            std::thread::sleep(SCREEN_STOP_WAIT.saturating_sub(asked.elapsed()));
            completion = Err(RecvTimeoutError::Timeout);
        }
        if stopped_without_error(&completion) {
            self.stream = None;
            self.stopped = true;
            release(self.output.take(), self.queue.take());
            return Ok(());
        }
        keep_for_the_life_of_the_process(self.output.take(), self.queue.take());
        Err(match completion {
            Ok(StopReply::Failed { code }) => CaptureError::Os {
                stage: "stop_capture",
                domain: "ScreenCaptureKit",
                code,
            },
            Ok(StopReply::Stopped) | Err(_) => CaptureError::Timeout {
                stage: "stop_capture",
                ms: SCREEN_STOP_WAIT.as_millis() as u64,
            },
        })
    }

    fn health(&self) -> SourceHealth {
        let state = match (&self.stream, &self.failure) {
            (Some(_), _) => SourceState::Running,
            (None, Some(error)) => source_failure(error.clone()),
            (None, None) if self.stopped => SourceState::Stopped(StopCause::Request),
            (None, None) => SourceState::NotReported,
        };
        SourceHealth {
            state,
            counters: SourceCounters::default(),
        }
    }
}

impl Drop for ScreenSource {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn stopped_without_error(completion: &Result<StopReply, RecvTimeoutError>) -> bool {
    *completion == Ok(StopReply::Stopped)
}

fn give_back_the_sink(output: &ScreenOutput) {
    drop(
        output
            .ivars()
            .sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take(),
    );
}

fn release(output: Option<Retained<ScreenOutput>>, queue: Option<DispatchRetained<DispatchQueue>>) {
    if let Some(output) = &output {
        give_back_the_sink(output);
    }
    drop((output, queue));
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
    use crate::macos::foreground::foreground_state_for_test;
    use crate::sink::{self, SinkWriter};
    use core_foundation::base::{CFGetRetainCount, CFRelease};
    use objc2::rc::{Allocated, Weak};
    use objc2_core_media::{CMSampleTimingInfo, CMTimeFlags};
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::rc::Rc;

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
        assert_eq!(depth, 8);
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
        let (_fg_writer, reader) = foreground_state_for_test();
        let output = ScreenOutput::new(handle, reader);
        let weak = Weak::from_retained(&output);
        keep_for_the_life_of_the_process(Some(output), None);
        assert!(weak.load().is_some(), "the kept output was released");
    }

    #[test]
    fn a_kept_output_lets_the_drain_end() {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state_for_test();
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
        let (_writer, reader) = foreground_state_for_test();
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
        let (_writer, reader) = foreground_state_for_test();
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
        let (_writer, reader) = foreground_state_for_test();
        let output = ScreenOutput::new(handle, reader);
        let frame = Frame::new(16, 16);
        let before = frame.retain_count();
        deliver(&output, frame.sample());
        assert_eq!(frame.retain_count(), before);
    }

    #[test]
    fn only_a_stop_completion_without_error_releases_the_output() {
        assert!(stopped_without_error(&Ok(StopReply::Stopped)));
        assert!(!stopped_without_error(&Ok(StopReply::Failed {
            code: -3808
        })));
        assert!(!stopped_without_error(&Err(RecvTimeoutError::Timeout)));
        assert!(!stopped_without_error(&Err(RecvTimeoutError::Disconnected)));
    }

    fn screen_ivars() -> (ScreenIvars, SinkWriter) {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state_for_test();
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
    fn a_frame_carries_the_app_in_front() {
        let (handle, writer) = sink::channel(8);
        let (fg_writer, reader) = foreground_state_for_test();
        let front = crate::macos::foreground::tests_support::front("com.example.editor", 41);
        fg_writer.set_for_test(Some(front));
        let ivars = ScreenIvars {
            sink: Mutex::new(Some(handle)),
            foreground: reader,
        };
        let frame = Frame::new(16, 16);
        on_sample(&ivars, frame.sample(), SCStreamOutputType::Screen);
        drop(ivars);
        let apps: Vec<String> = std::iter::from_fn(|| writer.recv())
            .map(|event| event.foreground_app)
            .collect();
        assert_eq!(apps, ["com.example.editor"]);
    }

    #[test]
    fn the_frame_handler_keeps_no_reference_to_the_sample() {
        let (ivars, _writer) = screen_ivars();
        let frame = Frame::new(16, 16);
        let before = frame.retain_count();
        on_sample(&ivars, frame.sample(), SCStreamOutputType::Screen);
        assert_eq!(frame.retain_count(), before);
    }
    type Answers = Rc<RefCell<std::collections::VecDeque<Option<StopReply>>>>;

    struct ScriptedStream {
        answers: Answers,
        asks: Rc<Cell<u32>>,
        held: RefCell<Vec<Sender<StopReply>>>,
    }

    impl StopStream for ScriptedStream {
        fn ask_stop(&self, reply: Sender<StopReply>) {
            self.asks.set(self.asks.get() + 1);
            match self.answers.borrow_mut().pop_front().flatten() {
                Some(answer) => {
                    let _ = reply.send(answer);
                }
                None => self.held.borrow_mut().push(reply),
            }
        }
    }

    fn scripted(
        answers: &[Option<StopReply>],
        asks: &Rc<Cell<u32>>,
    ) -> impl FnOnce(SinkHandle, ForegroundReader, FrameRateCap) -> Result<Opened, CaptureError>
    {
        let answers: Answers = Rc::new(RefCell::new(answers.iter().copied().collect()));
        let asks = asks.clone();
        move |sink, foreground, _cap| {
            Ok((
                Box::new(ScriptedStream {
                    answers,
                    asks,
                    held: RefCell::new(Vec::new()),
                }),
                ScreenOutput::new(sink, foreground),
                DispatchQueue::new("scripted", None),
            ))
        }
    }

    fn started(
        answers: &[Option<StopReply>],
    ) -> (
        ScreenSource,
        SinkHandle,
        SinkWriter,
        Rc<Cell<u32>>,
        Weak<ScreenOutput>,
    ) {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state_for_test();
        let mut source = ScreenSource::new(SCREEN_FRAME_CAP, reader);
        let asks = Rc::new(Cell::new(0));
        let first = source.begin_with(handle.clone(), scripted(answers, &asks));
        assert_eq!(first, Ok(Started::Running));
        let weak = Weak::from_retained(
            source
                .output
                .as_ref()
                .expect("a started source holds its output"),
        );
        (source, handle, writer, asks, weak)
    }

    fn counting_opener(
        opened: &Cell<u32>,
    ) -> impl FnOnce(SinkHandle, ForegroundReader, FrameRateCap) -> Result<Opened, CaptureError> + '_
    {
        move |_sink, _foreground, _cap| {
            opened.set(opened.get() + 1);
            Err(CaptureError::Start(
                "the counting opener opens nothing".into(),
            ))
        }
    }

    #[test]
    fn a_source_with_no_stream_calls_its_opener_once() {
        let (handle, _writer) = sink::channel(8);
        let (_fg_writer, reader) = foreground_state_for_test();
        let mut source = ScreenSource::new(SCREEN_FRAME_CAP, reader);
        let opened = Cell::new(0);
        let begun = source.begin_with(handle, counting_opener(&opened));
        assert_eq!(opened.get(), 1);
        assert_eq!(
            begun,
            Err(CaptureError::Start(
                "the counting opener opens nothing".into()
            ))
        );
    }

    fn sink_closed(handle: SinkHandle, writer: &SinkWriter) -> bool {
        drop(handle);
        writer
            .recv_timeout(Duration::from_secs(1))
            .map(|r| r.event().modality)
            == Err(RecvTimeoutError::Disconnected)
    }

    #[test]
    fn a_second_begin_is_refused_and_a_clean_shutdown_releases_the_output_and_the_sink() {
        let (mut source, handle, writer, asks, weak) = started(&[Some(StopReply::Stopped)]);
        let opened = Cell::new(0);
        let second = source.begin_with(handle.clone(), counting_opener(&opened));
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        assert_eq!(second, Err(CaptureError::AlreadyStarted));
        assert_eq!(opened.get(), 0, "a second begin opened a stream");
        assert!(
            shut == Ok(()) && took < SCREEN_STOP_DEADLINE,
            "{shut:?} after {took:?}"
        );
        assert_eq!(asks.get(), 1);
        assert!(weak.load().is_none(), "a clean stop kept the output");
        assert_eq!(
            source.health().state,
            SourceState::Stopped(StopCause::Request)
        );
        assert!(
            sink_closed(handle, &writer),
            "a handle of the sink is still held"
        );
    }

    #[test]
    fn a_stop_whose_wait_gives_up_answers_timeout_inside_six_seconds_and_leaves_no_output_or_queue()
    {
        let (mut source, handle, writer, asks, weak) = started(&[None, Some(StopReply::Stopped)]);
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        assert_eq!(
            shut,
            Err(CaptureError::Timeout {
                stage: "stop_capture",
                ms: 5000
            })
        );
        assert!(took < SCREEN_STOP_DEADLINE, "the shutdown took {took:?}");
        assert!(source.output.is_none() && source.queue.is_none());
        assert!(weak.load().is_some(), "the kept output was released");
        let opened = Cell::new(0);
        assert_eq!(
            source.begin_with(handle.clone(), counting_opener(&opened)),
            Err(CaptureError::AlreadyStarted)
        );
        assert_eq!(opened.get(), 0, "a begin after the timeout opened a stream");
        assert!(
            sink_closed(handle, &writer),
            "the kept output still holds the sink"
        );
        drop(source);
        assert_eq!(
            asks.get(),
            2,
            "the drop did not ask the stream to stop again"
        );
    }

    struct SlowAskStream(RefCell<Vec<Sender<StopReply>>>);

    impl StopStream for SlowAskStream {
        fn ask_stop(&self, reply: Sender<StopReply>) {
            std::thread::sleep(Duration::from_millis(1500));
            self.0.borrow_mut().push(reply);
        }
    }

    #[test]
    fn a_stop_whose_ask_takes_1500_ms_still_answers_inside_the_deadline() {
        let (handle, _writer) = sink::channel(8);
        let (_fg_writer, reader) = foreground_state_for_test();
        let mut source = ScreenSource::new(SCREEN_FRAME_CAP, reader);
        source
            .begin_with(handle, |sink, foreground, _cap| {
                Ok((
                    Box::new(SlowAskStream(RefCell::new(Vec::new()))),
                    ScreenOutput::new(sink, foreground),
                    DispatchQueue::new("slow-ask", None),
                ))
            })
            .expect("the source starts with a stream whose ask is slow");
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        source.stream = None;
        assert_eq!(
            shut,
            Err(CaptureError::Timeout {
                stage: "stop_capture",
                ms: 5000
            })
        );
        assert!(
            took < SCREEN_STOP_DEADLINE,
            "the shutdown took {took:?}, past the deadline of {SCREEN_STOP_DEADLINE:?}"
        );
    }

    struct DroppingStream;

    impl StopStream for DroppingStream {
        fn ask_stop(&self, reply: Sender<StopReply>) {
            drop(reply);
        }
    }

    #[test]
    fn a_stop_whose_reply_is_dropped_unanswered_waits_its_wait_and_answers_timeout() {
        let (handle, writer) = sink::channel(8);
        let (_fg_writer, reader) = foreground_state_for_test();
        let mut source = ScreenSource::new(SCREEN_FRAME_CAP, reader);
        source
            .begin_with(handle.clone(), |sink, foreground, _cap| {
                Ok((
                    Box::new(DroppingStream),
                    ScreenOutput::new(sink, foreground),
                    DispatchQueue::new("dropping", None),
                ))
            })
            .expect("the source starts with a stream that drops its reply");
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        let (output, queue) = (source.output.is_none(), source.queue.is_none());
        source.stream = None;
        assert_eq!(
            shut,
            Err(CaptureError::Timeout {
                stage: "stop_capture",
                ms: 5000
            })
        );
        assert!(
            took >= SCREEN_STOP_WAIT,
            "a Timeout was answered after {took:?}, before the wait had run"
        );
        assert!(
            output && queue,
            "the output or the queue was left in the source"
        );
        assert!(
            sink_closed(handle, &writer),
            "the kept output still holds the sink"
        );
    }

    #[test]
    fn a_stop_that_reports_an_error_keeps_the_output_and_answers_its_code() {
        let (mut source, handle, writer, _asks, weak) = started(&[
            Some(StopReply::Failed { code: -3808 }),
            Some(StopReply::Stopped),
        ]);
        let shut = source.shutdown();
        assert_eq!(
            shut,
            Err(CaptureError::Os {
                stage: "stop_capture",
                domain: "ScreenCaptureKit",
                code: -3808
            })
        );
        assert!(
            weak.load().is_some(),
            "the output of a failed stop was released"
        );
        assert!(
            sink_closed(handle, &writer),
            "the kept output still holds the sink"
        );
    }

    #[test]
    fn a_clean_stop_gives_back_the_sink_while_another_holder_keeps_the_output() {
        let (mut source, handle, writer, _asks, _weak) = started(&[Some(StopReply::Stopped)]);
        let held_elsewhere = source
            .output
            .clone()
            .expect("a started source holds its output");
        let shut = source.shutdown();
        let closed = sink_closed(handle, &writer);
        drop(held_elsewhere);
        assert_eq!(shut, Ok(()));
        assert!(
            closed,
            "the output that a holder keeps still holds the sink"
        );
    }

    #[test]
    fn a_dropped_started_source_asks_the_stream_to_stop_and_releases_the_output() {
        let (source, handle, writer, asks, weak) = started(&[Some(StopReply::Stopped)]);
        drop(source);
        assert_eq!(asks.get(), 1);
        assert!(weak.load().is_none());
        assert!(sink_closed(handle, &writer));
    }

    #[test]
    fn the_stop_wait_is_inside_the_deadline() {
        assert_eq!(SCREEN_STOP_DEADLINE, Duration::from_secs(6));
        assert_eq!(SCREEN_STOP_WAIT, Duration::from_secs(5));
        assert!(SCREEN_STOP_WAIT < SCREEN_STOP_DEADLINE);
    }

    #[test]
    fn only_a_denial_of_the_system_is_a_permission_error_at_the_content_read() {
        assert_eq!(
            content_refusal(true, Err(RecvTimeoutError::Timeout)),
            CaptureError::PermissionDenied(Modality::ScreenFrame)
        );
        assert_eq!(
            content_refusal(false, Ok(-3801)),
            CaptureError::Os {
                stage: "shareable_content",
                domain: "ScreenCaptureKit",
                code: -3801
            }
        );
        assert_eq!(
            content_refusal(false, Err(RecvTimeoutError::Timeout)),
            CaptureError::Timeout {
                stage: "shareable_content",
                ms: 10_000
            }
        );
    }
}
