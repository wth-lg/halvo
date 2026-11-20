#![deny(clippy::let_underscore_must_use)]

use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicI64, AtomicU8, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use block2::RcBlock;
use core_foundation::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation::base::{CFGetTypeID, TCFType};
use core_foundation::date::CFDate;
use core_foundation::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
use core_foundation::number::{
    kCFNumberSInt64Type, CFNumberGetTypeID, CFNumberGetValue, CFNumberRef,
};
use core_foundation::runloop::{
    kCFRunLoopCommonModes, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext,
    CFRunLoopTimerInvalidate, CFRunLoopTimerRef,
};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::NSWorkspace;
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_foundation::{NSArray, NSError, NSInteger, NSObjectProtocol, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCFrameStatus, SCRunningApplication, SCShareableContent, SCStream,
    SCStreamConfiguration, SCStreamDelegate, SCStreamFrameInfoStatus, SCStreamOutput,
    SCStreamOutputType, SCWindow,
};

use crate::event::{AppIdentity, EventClock, FloorEvent, Modality};
use crate::floor::exclude::{DropReason, FloorHandle, PrivateWindow, ProcessId, Subject, Verdict};
use crate::floor::{kind, Switch};
use crate::macos::foreground::{bundle_identifier, executable_path, ForegroundReader, HalvoApp};
use crate::macos::source_failure;
use crate::secure_input_macos::SystemProbe;
use crate::sink::SinkHandle;
use crate::source::{
    CaptureError, CaptureSource, HealthReason, SourceContext, SourceCounters, SourceHealth,
    SourceState, Started, StopCause,
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

/// The stop code with which ScreenCaptureKit tells that the person stopped the
/// stream from the system's control (SCStreamErrorUserStopped, SCError.h).
const USER_STOPPED: i64 = -3817;

/// The first wait between two rebuilds of the filter that did not list Halvo's
/// app; each further wait doubles, up to [`REBUILD_MAX_BACKOFF`].
const REBUILD_FIRST_BACKOFF: Duration = Duration::from_secs(1);

const REBUILD_MAX_BACKOFF: Duration = Duration::from_secs(60);

/// How often the rebuild's timer reads whether a rebuild is due.
const REBUILD_TICK: Duration = Duration::from_secs(1);

/// What the frame callback does with one sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameAction {
    /// One activity record: a complete or started frame with an image buffer.
    Record,
    /// No record, and the stream delivers: an idle frame.
    Quiet,
    /// No record, and the stream reads impaired with this reason.
    Impaired(HealthReason),
    /// No record and no change: no status, a stop, or a complete or started
    /// status with no image buffer.
    Ignore,
}

const COMPLETE: NSInteger = SCFrameStatus::Complete.0;
const IDLE: NSInteger = SCFrameStatus::Idle.0;
const BLANK: NSInteger = SCFrameStatus::Blank.0;
const SUSPENDED: NSInteger = SCFrameStatus::Suspended.0;
const STARTED: NSInteger = SCFrameStatus::Started.0;

fn frame_action(status: Option<NSInteger>, has_image: bool) -> FrameAction {
    match status {
        Some(COMPLETE | STARTED) if has_image => FrameAction::Record,
        Some(IDLE) => FrameAction::Quiet,
        Some(BLANK) => FrameAction::Impaired(HealthReason::StreamBlank),
        Some(SUSPENDED) => FrameAction::Impaired(HealthReason::StreamSuspended),
        _ => FrameAction::Ignore,
    }
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetSampleAttachmentsArray(
        sample: *const c_void,
        create_if_necessary: u8,
    ) -> CFArrayRef;
    fn CMSampleBufferGetImageBuffer(sample: *const c_void) -> *const c_void;
}

/// The value of SCStreamFrameInfoStatus in the first attachments dictionary of
/// `sample`, or `None` when the sample carries none.
fn frame_status(sample: &CMSampleBuffer) -> Option<NSInteger> {
    let sample = (sample as *const CMSampleBuffer).cast::<c_void>();
    // SAFETY: `sample` is a live CMSampleBuffer for the length of the call. With
    // create set to 0 the getter makes nothing and returns the buffer's own
    // array at +0, or null.
    let array = unsafe { CMSampleBufferGetSampleAttachmentsArray(sample, 0) };
    if array.is_null() {
        return None;
    }
    // SAFETY: `array` is a live CFArray that the sample owns while it lives.
    if unsafe { CFArrayGetCount(array) } < 1 {
        return None;
    }
    // SAFETY: index 0 lies inside the array; its element is the attachments
    // dictionary of the first sample, at +0.
    let attachments = unsafe { CFArrayGetValueAtIndex(array, 0) } as CFDictionaryRef;
    if attachments.is_null() {
        return None;
    }
    // SAFETY: SCStreamFrameInfoStatus is a constant NSString that
    // ScreenCaptureKit exports and never writes.
    let key: &NSString = unsafe { SCStreamFrameInfoStatus };
    // SAFETY: `attachments` is a live CFDictionary and the key is a live
    // NSString, which is toll-free bridged with CFString.
    let value = unsafe { CFDictionaryGetValue(attachments, (key as *const NSString).cast()) };
    if value.is_null() {
        return None;
    }
    // SAFETY: `value` is a live CF object that the dictionary owns.
    if unsafe { CFGetTypeID(value) } != unsafe { CFNumberGetTypeID() } {
        return None;
    }
    let mut raw: i64 = 0;
    // SAFETY: `value` is a CFNumber, as the type check shows, and `raw` has room
    // for the 64-bit integer that the call writes.
    let read = unsafe {
        CFNumberGetValue(
            value as CFNumberRef,
            kCFNumberSInt64Type,
            (&mut raw as *mut i64).cast(),
        )
    };
    read.then_some(raw as NSInteger)
}

/// Whether `sample` carries an image buffer. No byte of the image is read.
fn has_image(sample: &CMSampleBuffer) -> bool {
    // SAFETY: `sample` is a live CMSampleBuffer for the length of the call; the
    // getter returns its image buffer at +0, or null, and reads no pixel.
    !unsafe { CMSampleBufferGetImageBuffer((sample as *const CMSampleBuffer).cast()) }.is_null()
}

/// What the stream's own callbacks have told, shared with the source's health:
/// the last impairment of the frames, the stop code of the delegate, and
/// whether the current filter lists Halvo's app.
#[derive(Default)]
struct StreamFacts {
    condition: AtomicU8,
    stop_code: AtomicI64,
    lists_app: AtomicU8,
    listed: Mutex<Vec<libc::pid_t>>,
}

const DELIVERING: u8 = 0;
const BLANK_FRAMES: u8 = 1;
const SUSPENDED_FRAMES: u8 = 2;
const NO_FILTER: u8 = 0;
const FILTER_LACKS_APP: u8 = 1;
const FILTER_LISTS_APP: u8 = 2;

impl StreamFacts {
    fn frame(&self, action: FrameAction) {
        let condition = match action {
            FrameAction::Record | FrameAction::Quiet => DELIVERING,
            FrameAction::Impaired(HealthReason::StreamSuspended) => SUSPENDED_FRAMES,
            FrameAction::Impaired(_) => BLANK_FRAMES,
            FrameAction::Ignore => return,
        };
        self.condition.store(condition, Ordering::Relaxed);
    }

    fn stopped_with(&self, code: i64) {
        self.stop_code.store(code, Ordering::SeqCst);
    }

    fn filter(&self, listed: Vec<libc::pid_t>, lists_app: bool) {
        *self.listed.lock().unwrap_or_else(PoisonError::into_inner) = listed;
        let fact = if lists_app {
            FILTER_LISTS_APP
        } else {
            FILTER_LACKS_APP
        };
        self.lists_app.store(fact, Ordering::SeqCst);
    }

    fn listed(&self) -> Vec<libc::pid_t> {
        self.listed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn lists_app(&self) -> Option<bool> {
        match self.lists_app.load(Ordering::SeqCst) {
            NO_FILTER => None,
            fact => Some(fact == FILTER_LISTS_APP),
        }
    }

    fn state(&self) -> SourceState {
        match self.stop_code.load(Ordering::SeqCst) {
            0 => {}
            code => return stopped_by(code),
        }
        match self.condition.load(Ordering::Relaxed) {
            BLANK_FRAMES => SourceState::Impaired {
                reason: HealthReason::StreamBlank,
            },
            SUSPENDED_FRAMES => SourceState::Impaired {
                reason: HealthReason::StreamSuspended,
            },
            _ => SourceState::Running,
        }
    }
}

/// The state that the stop code of the stream's delegate gives: stopped with
/// the cause user for SCStreamErrorUserStopped, failed for any other code.
fn stopped_by(code: i64) -> SourceState {
    if code == USER_STOPPED {
        SourceState::Stopped(StopCause::User)
    } else {
        source_failure(CaptureError::Os {
            stage: "stream_stopped",
            domain: "ScreenCaptureKit",
            code,
        })
    }
}

/// What decides whether a frame becomes a record: the app in front, Halvo's
/// app, the exclusion decision and its secure-input probe.
struct FrameRule {
    foreground: ForegroundReader,
    floor: FloorHandle,
    halvo: HalvoApp,
    probe: SystemProbe,
}

impl FrameRule {
    /// The keep verdict for a frame that arrives now, on the app in front read
    /// at its arrival through the reader that the poll uses. None before the
    /// poll has answered once, when that read fails or names nothing in front,
    /// while Halvo's app is in front, and when the decision drops.
    fn verdict(&self) -> Option<crate::floor::exclude::Keep<kind::ScreenFrame>> {
        let front = self.foreground.read_now()?;
        if self.halvo.selects_front(&front) {
            return None;
        }
        let pid = ProcessId::new(u32::try_from(front.pid).ok()?);
        let subject = Subject::new(front.identity.to_event(), pid);
        match self
            .floor
            .decide::<kind::ScreenFrame>(subject, PrivateWindow::Unknown, &self.probe)
        {
            Verdict::Keep(keep) => Some(keep),
            Verdict::Drop(_) => None,
        }
    }
}

struct ScreenIvars {
    sink: Mutex<Option<SinkHandle>>,
    rule: FrameRule,
    facts: Arc<StreamFacts>,
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

    unsafe impl SCStreamDelegate for ScreenOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn stream_stopped(&self, _stream: &SCStream, error: &NSError) {
            self.ivars().facts.stopped_with(error.code() as i64);
        }
    }
);

fn on_sample(ivars: &ScreenIvars, sample: &CMSampleBuffer, ty: SCStreamOutputType) {
    if ty != SCStreamOutputType::Screen {
        return;
    }
    let action = frame_action(frame_status(sample), has_image(sample));
    ivars.facts.frame(action);
    if action == FrameAction::Record {
        record_frame(ivars);
    }
}

fn record_frame(ivars: &ScreenIvars) {
    let Some(keep) = ivars.rule.verdict() else {
        return;
    };
    let sink = ivars.sink.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(sink) = sink.as_ref() {
        sink.emit_event(FloorEvent::screen_frame(keep, None, EventClock::system()));
    }
}

/// What a stream's output takes at its start.
struct FrameContext {
    sink: SinkHandle,
    rule: FrameRule,
    facts: Arc<StreamFacts>,
}

impl ScreenOutput {
    fn new(frames: FrameContext) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ScreenIvars {
            sink: Mutex::new(Some(frames.sink)),
            rule: frames.rule,
            facts: frames.facts,
        });
        // SAFETY: `this` is a fresh allocation of this class with its ivars set,
        // and `init` is NSObject's initializer: it takes no argument and returns
        // the object it was sent to.
        unsafe { msg_send![super(this), init] }
    }
}

/// Which applications the filter leaves out: this process, Halvo's app by its
/// one predicate, and every process that the exclusion decision places under
/// the exclusion root. Every other application is kept, a password manager
/// too, whose frames the decision drops while it is in front.
struct Selection {
    own: libc::pid_t,
    halvo: HalvoApp,
    floor: FloorHandle,
    probe: SystemProbe,
}

impl Selection {
    fn excludes(&self, pid: libc::pid_t, bundle_id: &str, path: Option<&str>) -> bool {
        pid == self.own || self.halvo.selects(bundle_id, path) || self.under_root(pid)
    }

    fn under_root(&self, pid: libc::pid_t) -> bool {
        RootTree {
            floor: &self.floor,
            probe: &self.probe,
        }
        .holds(pid)
    }
}

/// The tree of the exclusion root, as the selection and the rebuild plan read
/// it: core-2's exclusion decision over the floor's root and its parent-pid
/// lookup, asked for the kind foreground_app, whose decision reads no switch
/// and no secure-input probe.
struct RootTree<'a> {
    floor: &'a FloorHandle,
    probe: &'a SystemProbe,
}

impl RootTree<'_> {
    /// Whether the decision places `pid` under the root: the root itself or a
    /// process whose lineage reaches it. A process whose lineage reads unknown
    /// is not; with no root, no process is.
    fn holds(&self, pid: libc::pid_t) -> bool {
        let Ok(pid) = u32::try_from(pid) else {
            return false;
        };
        let subject = Subject::new(AppIdentity::unknown(), ProcessId::new(pid));
        matches!(
            self.floor
                .decide::<kind::ForegroundApp>(subject, PrivateWindow::Unknown, self.probe),
            Verdict::Drop(DropReason::ExclusionTree)
        )
    }
}

/// One application of the shareable content as the selection reads it.
struct AppEntry {
    pid: libc::pid_t,
    bundle_id: String,
    path: Option<String>,
}

fn excluded_applications<T>(
    apps: impl IntoIterator<Item = T>,
    entry_of: impl Fn(&T) -> AppEntry,
    selection: &Selection,
) -> Vec<T> {
    apps.into_iter()
        .filter(|app| {
            let entry = entry_of(app);
            selection.excludes(entry.pid, &entry.bundle_id, entry.path.as_deref())
        })
        .collect()
}

fn entry_of(app: &SCRunningApplication) -> AppEntry {
    // SAFETY: `app` is a live SCRunningApplication of the content that the
    // caller holds, and both getters are property getters that take no
    // argument. The block reads the pid and the retained bundle id alone; the
    // text of the id is made outside it, by `entry_from`.
    let (pid, bundle_id) = unsafe { (app.processID(), app.bundleIdentifier()) };
    entry_from(pid, &bundle_id)
}

/// The entry of the application `pid` whose bundle id is `bundle_id`, read
/// through `bundle_identifier`, which never aborts.
fn entry_from(pid: libc::pid_t, bundle_id: &NSString) -> AppEntry {
    AppEntry {
        pid,
        bundle_id: bundle_identifier(bundle_id),
        path: executable_path(pid),
    }
}

/// The filter of `display` that leaves out what `selection` excludes, with the
/// pids that it leaves out and whether Halvo's app is among them.
fn filter_for(
    content: &SCShareableContent,
    display: &SCDisplay,
    selection: &Selection,
) -> (Retained<SCContentFilter>, Vec<libc::pid_t>, bool) {
    // SAFETY: `content` is a live SCShareableContent that the caller holds, and
    // `applications` is a property getter that takes no argument.
    let apps = unsafe { content.applications() };
    let excluded = excluded_applications(apps.iter(), |app| entry_of(app), selection);
    let entries: Vec<AppEntry> = excluded.iter().map(|app| entry_of(app)).collect();
    let lists_app = entries.iter().any(|entry| {
        selection
            .halvo
            .selects(&entry.bundle_id, entry.path.as_deref())
    });
    let pids = entries.iter().map(|entry| entry.pid).collect();
    let excluded_refs: Vec<&SCRunningApplication> = excluded.iter().map(|r| &**r).collect();
    let excluded_apps = NSArray::from_slice(&excluded_refs);
    let no_windows: Retained<NSArray<SCWindow>> = NSArray::new();
    // SAFETY: the receiver is a fresh allocation of SCContentFilter; `display`
    // and the two arrays live past the call, and the arrays hold the element
    // types that the initializer declares.
    let filter = unsafe {
        SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            display,
            &excluded_apps,
            &no_windows,
        )
    };
    (filter, pids, lists_app)
}

/// The display that the source captures: the main display by its id, else the
/// first of the list (README RD-39).
fn chosen_display(ids: &[u32], main: u32) -> Option<usize> {
    ids.iter()
        .position(|&id| id == main)
        .or((!ids.is_empty()).then_some(0))
}

fn display_ids(displays: &NSArray<SCDisplay>) -> Vec<u32> {
    displays
        .iter()
        // SAFETY: `display` is a retained element of `displays`, and `displayID`
        // is a property getter that takes no argument.
        .map(|display| unsafe { display.displayID() })
        .collect()
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
            reply.send(answer).ok();
        });
        // SAFETY: the stream is live, and the handler has the completion
        // signature `void (^)(NSError *)`.
        unsafe { self.stopCaptureWithCompletionHandler(Some(&handler)) };
    }
}

/// When the filter is built again (B14-19 (2), ruling MA-35 R3): while Halvo's
/// app runs under a pid that the current filter does not list, at once when
/// the app comes in front, else after a wait of 1 s that doubles up to 60 s
/// after each rebuild that still did not list it; and while a running process
/// that the decision places under the exclusion root is not listed, with a
/// wait of its own for each such pid, 1 s doubling up to 60 s, which starts
/// again when that pid is listed or no longer runs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RebuildPlan {
    next: Option<Instant>,
    backoff: Duration,
    app_in_front: bool,
    tree: std::collections::BTreeMap<libc::pid_t, TreeWait>,
    pending: Vec<libc::pid_t>,
}

/// The wait of one unlisted pid under the exclusion root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TreeWait {
    next: Option<Instant>,
    backoff: Duration,
}

impl RebuildPlan {
    fn new() -> Self {
        Self {
            next: None,
            backoff: REBUILD_FIRST_BACKOFF,
            app_in_front: false,
            tree: std::collections::BTreeMap::new(),
            pending: Vec::new(),
        }
    }

    /// Whether a rebuild is due at `now`, for the pids that the current filter
    /// lists, the pids of Halvo's app, whether the app is in front, the pids of
    /// the running applications and the tree of the exclusion root.
    fn due(
        &mut self,
        listed: &[libc::pid_t],
        app: &[libc::pid_t],
        app_in_front: bool,
        running: &[libc::pid_t],
        tree: &RootTree<'_>,
        now: Instant,
    ) -> bool {
        let came_in_front = app_in_front && !self.app_in_front;
        self.app_in_front = app_in_front;
        let app_due = !app.iter().all(|pid| listed.contains(pid))
            && (came_in_front || self.next.is_none_or(|next| now >= next));
        let unlisted: Vec<libc::pid_t> = running
            .iter()
            .copied()
            .filter(|pid| !listed.contains(pid) && tree.holds(*pid))
            .collect();
        self.tree.retain(|pid, _| unlisted.contains(pid));
        self.pending = unlisted
            .into_iter()
            .filter(|pid| {
                self.tree
                    .get(pid)
                    .is_none_or(|wait| wait.next.is_none_or(|next| now >= next))
            })
            .collect();
        app_due || !self.pending.is_empty()
    }

    /// Records a rebuild at `now` whose filter lists the app or does not, and
    /// starts the next wait of each pid under the root that was due.
    fn tried(&mut self, now: Instant, lists_app: bool) {
        if lists_app {
            self.next = None;
            self.backoff = REBUILD_FIRST_BACKOFF;
        } else {
            self.next = Some(now + self.backoff);
            self.backoff = (self.backoff * 2).min(REBUILD_MAX_BACKOFF);
        }
        for pid in std::mem::take(&mut self.pending) {
            let wait = self.tree.entry(pid).or_insert(TreeWait {
                next: None,
                backoff: REBUILD_FIRST_BACKOFF,
            });
            wait.next = Some(now + wait.backoff);
            wait.backoff = (wait.backoff * 2).min(REBUILD_MAX_BACKOFF);
        }
    }
}

/// The pids of the running applications, and those of them that are Halvo's.
fn running_pids(halvo: &HalvoApp) -> (Vec<libc::pid_t>, Vec<libc::pid_t>) {
    objc2::rc::autoreleasepool(|_| {
        let mut running = Vec::new();
        let mut app = Vec::new();
        for item in NSWorkspace::sharedWorkspace().runningApplications().iter() {
            let pid = item.processIdentifier();
            running.push(pid);
            app.extend(halvo_pid_of(halvo, pid, item.bundleIdentifier().as_deref()));
        }
        (running, app)
    })
}

/// `pid` when the running application `pid` with the bundle id `bundle_id` is
/// Halvo's; the id is read through `bundle_identifier`, which never aborts.
fn halvo_pid_of(
    halvo: &HalvoApp,
    pid: libc::pid_t,
    bundle_id: Option<&NSString>,
) -> Option<libc::pid_t> {
    let bundle_id = bundle_id.map(bundle_identifier).unwrap_or_default();
    halvo
        .selects(&bundle_id, executable_path(pid).as_deref())
        .then_some(pid)
}

/// What the rebuild's timer works on. It lives in a box whose address the
/// timer holds, and only that timer's callout reads it.
struct RebuildCell {
    stream: Retained<SCStream>,
    display: u32,
    selection: Selection,
    foreground: ForegroundReader,
    facts: Arc<StreamFacts>,
    plan: RebuildPlan,
}

impl RebuildCell {
    fn tick(&mut self, now: Instant) {
        let (running, app) = running_pids(&self.selection.halvo);
        let in_front = self
            .foreground
            .get()
            .is_some_and(|front| self.selection.halvo.selects_front(&front));
        let tree = RootTree {
            floor: &self.selection.floor,
            probe: &self.selection.probe,
        };
        if !self
            .plan
            .due(&self.facts.listed(), &app, in_front, &running, &tree, now)
        {
            return;
        }
        let lists_app = self.rebuild().unwrap_or(false);
        self.plan.tried(now, lists_app);
    }

    /// Builds the filter again against the stored display, or the display that
    /// the chooser takes when the stored one has left the list, and hands it to
    /// the stream. Whether the new filter lists Halvo's app.
    fn rebuild(&mut self) -> Result<bool, CaptureError> {
        let content = fetch_shareable_content()?;
        // SAFETY: `content` is a live, retained SCShareableContent, and
        // `displays` is a property getter that takes no argument.
        let displays = unsafe { content.displays() };
        let ids = display_ids(&displays);
        let index = match ids.iter().position(|&id| id == self.display) {
            Some(index) => index,
            None => chosen_display(&ids, main_display_id())
                .ok_or_else(|| CaptureError::Start("no display to capture".into()))?,
        };
        let display = displays.objectAtIndex(index);
        self.display = ids[index];
        let (filter, listed, lists_app) = filter_for(&content, &display, &self.selection);
        let facts = self.facts.clone();
        let handler = RcBlock::new(move |err: *mut NSError| {
            if err.is_null() {
                facts.filter(listed.clone(), lists_app);
            }
        });
        // SAFETY: the stream is live and runs; `filter` lives past the call, and
        // the handler has the completion signature `void (^)(NSError *)`.
        unsafe {
            self.stream
                .updateContentFilter_completionHandler(&filter, Some(&handler))
        };
        Ok(lists_app)
    }
}

/// A rebuild's cell with the timer that holds its address. Its Drop
/// invalidates the timer, and only then does the cell drop.
struct Rebuild {
    cell: Box<RebuildCell>,
    timer: Option<CFRunLoopTimer>,
}

impl Rebuild {
    /// Adds the timer to the run loop of the calling thread: the thread that
    /// began the source, which owns it, since a source is not Send.
    fn start(cell: RebuildCell) -> Self {
        let mut rebuild = Self {
            cell: Box::new(cell),
            timer: None,
        };
        let mut context = CFRunLoopTimerContext {
            version: 0,
            info: (&mut *rebuild.cell as *mut RebuildCell).cast::<c_void>(),
            retain: None,
            release: None,
            copyDescription: None,
        };
        let tick = REBUILD_TICK.as_secs_f64();
        let timer = CFRunLoopTimer::new(
            CFDate::now().abs_time() + tick,
            tick,
            0,
            0,
            rebuild_timer_fired,
            &mut context,
        );
        // SAFETY: `kCFRunLoopCommonModes` is a constant CFString that
        // CoreFoundation exports and never writes.
        CFRunLoop::get_current().add_timer(&timer, unsafe { kCFRunLoopCommonModes });
        rebuild.timer = Some(timer);
        rebuild
    }
}

impl Drop for Rebuild {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.take() {
            // SAFETY: `timer` is a live timer; invalidating it removes it from
            // every run loop, so its callout never runs again.
            unsafe { CFRunLoopTimerInvalidate(timer.as_concrete_TypeRef()) };
        }
    }
}

extern "C" fn rebuild_timer_fired(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    // SAFETY: `info` is the boxed RebuildCell of a Rebuild, whose box keeps it
    // at one address and whose Drop invalidates this timer before the box
    // drops. The timer fires on the run loop of the thread that owns the source,
    // which touches the cell only to drop it, so no other reference to the cell
    // is used while the callout runs.
    let cell = unsafe { &mut *info.cast::<RebuildCell>() };
    cell.tick(Instant::now());
}

type Opened = (
    Box<dyn StopStream>,
    Retained<ScreenOutput>,
    DispatchRetained<DispatchQueue>,
    Option<RebuildCell>,
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
    halvo: HalvoApp,
    probe: SystemProbe,
    facts: Arc<StreamFacts>,
    stream: Option<Box<dyn StopStream>>,
    output: Option<Retained<ScreenOutput>>,
    queue: Option<DispatchRetained<DispatchQueue>>,
    rebuild: Option<Rebuild>,
    failure: Option<CaptureError>,
    unstopped: Option<CaptureError>,
    off: bool,
    stopped: bool,
    #[cfg(test)]
    real: bool,
    _not_send: PhantomData<*const ()>,
}

impl ScreenSource {
    pub fn new(cap: FrameRateCap, foreground: ForegroundReader) -> Self {
        Self {
            cap,
            foreground,
            halvo: HalvoApp::beside_this_executable(),
            probe: SystemProbe::new(),
            facts: Arc::default(),
            stream: None,
            output: None,
            queue: None,
            rebuild: None,
            failure: None,
            unstopped: None,
            off: false,
            stopped: false,
            #[cfg(test)]
            real: false,
            _not_send: PhantomData,
        }
    }

    /// Whether the current filter of the stream lists Halvo's app, for the
    /// status field screen_filter_excludes_app (contract C10.2); `None` while no
    /// stream runs, which is the answer while the switch screen_activity is off.
    pub fn filter_excludes_app(&self) -> Option<bool> {
        self.stream.as_ref()?;
        self.facts.lists_app()
    }

    fn begin_with(
        &mut self,
        ctx: &SourceContext,
        open: impl FnOnce(FrameContext, Selection, FrameRateCap) -> Result<Opened, CaptureError>,
    ) -> Result<Started, CaptureError> {
        if self.stream.is_some() || self.unstopped.is_some() {
            return Err(CaptureError::AlreadyStarted);
        }
        if !ctx.floor().switches().is_on(Switch::ScreenActivity) {
            self.off = true;
            return Ok(Started::Off);
        }
        self.off = false;
        self.facts = Arc::default();
        let frames = FrameContext {
            sink: ctx.sink().clone(),
            rule: FrameRule {
                foreground: self.foreground.clone(),
                floor: ctx.floor().clone(),
                halvo: self.halvo.clone(),
                probe: self.probe,
            },
            facts: self.facts.clone(),
        };
        let selection = Selection {
            own: std::process::id() as libc::pid_t,
            halvo: self.halvo.clone(),
            floor: ctx.floor().clone(),
            probe: self.probe,
        };
        match open(frames, selection, self.cap) {
            Ok((stream, output, queue, rebuild)) => {
                self.stream = Some(stream);
                self.output = Some(output);
                self.queue = Some(queue);
                self.rebuild = rebuild.map(Rebuild::start);
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
    fn CGMainDisplayID() -> u32;
}

fn screen_access_denied() -> bool {
    // SAFETY: a parameterless predicate of the system with no side effect.
    !unsafe { CGPreflightScreenCaptureAccess() }
}

fn main_display_id() -> u32 {
    // SAFETY: a parameterless getter of the system with no side effect.
    unsafe { CGMainDisplayID() }
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
        tx.send(answer).ok();
    });
    // SAFETY: the handler has the completion signature
    // `void (^)(SCShareableContent *, NSError *)`. ScreenCaptureKit calls it on
    // a queue of its own, possibly after the wait below gave up. It holds
    // nothing but `tx`; a send to a receiver that is gone returns an error,
    // which the handler ignores, and the retained content drops with it. What
    // holds: the content is retained on ScreenCaptureKit's queue and crosses
    // to the calling thread once, through the channel, after the handler has
    // let go of it, so one thread at a time holds it and no two threads use it
    // at once. What does not hold as a proof: neither objc2, SCShareableContent.h
    // nor the framework's Swift interface says that the class may be sent
    // between threads, and nothing documents a tie of the object to the queue
    // that delivered it; this hand-over is a named limit (RR-macos-sources-20).
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&handler) };
    match rx.recv_timeout(SHAREABLE_CONTENT_WAIT) {
        Ok(Ok(content)) => Ok(content),
        Ok(Err(code)) => Err(content_refusal(screen_access_denied(), Ok(code))),
        Err(waited) => Err(content_refusal(screen_access_denied(), Err(waited))),
    }
}

fn open_stream(
    frames: FrameContext,
    selection: Selection,
    cap: FrameRateCap,
) -> Result<Opened, CaptureError> {
    let content = fetch_shareable_content()?;

    // SAFETY: `content` is a live, retained SCShareableContent, and
    // `displays` is a property getter that takes no argument.
    let displays = unsafe { content.displays() };
    let ids = display_ids(&displays);
    let index = chosen_display(&ids, main_display_id())
        .ok_or_else(|| CaptureError::Start("no display to capture".into()))?;
    let display = displays.objectAtIndex(index);

    let (filter, listed, lists_app) = filter_for(&content, &display, &selection);
    frames.facts.filter(listed, lists_app);

    // SAFETY: `display` is a live, retained SCDisplay, and `width` and
    // `height` are property getters that take no argument.
    let (width, height) = unsafe { (display.width() as usize, display.height() as usize) };
    let config = stream_configuration(width, height, cap);

    let foreground = frames.rule.foreground.clone();
    let facts = frames.facts.clone();
    let output = ScreenOutput::new(frames);
    let delegate: &ProtocolObject<dyn SCStreamDelegate> = ProtocolObject::from_ref(&*output);
    // SAFETY: the receiver is a fresh allocation of SCStream; `filter` and
    // `config` live past the call. The delegate is `output`, which the source
    // stores until the stream cannot run, or keeps for the life of the process.
    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &config,
            Some(delegate),
        )
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
        stx.send(code).ok();
    });
    // SAFETY: `stream` is live, and the handler has the completion signature
    // `void (^)(NSError *)`.
    unsafe { stream.startCaptureWithCompletionHandler(Some(&handler)) };
    match srx.recv_timeout(SCREEN_START_WAIT) {
        Ok(None) => {
            let rebuild = RebuildCell {
                stream: stream.clone(),
                display: ids[index],
                selection,
                foreground,
                facts,
                plan: RebuildPlan::new(),
            };
            Ok((Box::new(stream), output, queue, Some(rebuild)))
        }
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
        #[cfg(test)]
        {
            self.real = true;
        }
        self.begin_with(&ctx, open_stream)
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        self.rebuild = None;
        if self.stream.is_none() {
            return self.unstopped.clone().map_or(Ok(()), Err);
        }
        let answer = self.stop_stream();
        self.answered(answer)
    }

    fn health(&self) -> SourceHealth {
        let state = match (&self.stream, &self.failure) {
            (Some(_), _) => self.facts.state(),
            (None, Some(error)) => source_failure(error.clone()),
            (None, None) if self.stopped => SourceState::Stopped(StopCause::Request),
            (None, None) if self.off => SourceState::Off,
            (None, None) => SourceState::NotReported,
        };
        let state = self.unstopped.clone().map_or(state, source_failure);
        SourceHealth {
            state,
            counters: SourceCounters::default(),
        }
    }
}

impl ScreenSource {
    fn answered(&mut self, answer: Result<(), CaptureError>) -> Result<(), CaptureError> {
        self.unstopped = answer.clone().err();
        #[cfg(test)]
        crate::macos::foreground::note_shutdown_answer("screen", self.real, &answer);
        answer
    }

    fn stop_stream(&mut self) -> Result<(), CaptureError> {
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
}

impl Drop for ScreenSource {
    fn drop(&mut self) {
        if self.unstopped.is_some() {
            return;
        }
        self.shutdown().ok();
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
    use crate::floor::exclude::{Lineage, NoLineage, Parent};
    use crate::floor::Switches;
    use crate::macos::foreground::{foreground_state_for_test, Foreground};
    use crate::secure_input_macos::Session;
    use crate::sink::{self, SinkWriter};
    use core_foundation::base::{CFGetRetainCount, CFRelease};
    use core_foundation::number::CFNumber;
    use objc2::rc::{Allocated, Weak};
    use objc2_core_media::{CMSampleTimingInfo, CMTimeFlags};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    const SIBLING: &str = "/opt/halvo-test/halvo";

    fn switches_on() -> Switches {
        let mut switches = Switches::default();
        switches.set(Switch::ScreenActivity, true);
        switches
    }

    fn floor_on() -> FloorHandle {
        FloorHandle::new(None, Arc::new(NoLineage), switches_on())
    }

    fn context_on(handle: SinkHandle) -> SourceContext {
        SourceContext::with_floor(handle, floor_on())
    }

    fn at_console() -> Session {
        Session::AtConsole
    }

    fn not_at_console() -> Session {
        Session::NotAtConsole
    }

    fn byte_zero() -> u8 {
        0
    }

    fn byte_one() -> u8 {
        1
    }

    fn inactive() -> SystemProbe {
        SystemProbe::with_readers(at_console, byte_zero)
    }

    fn rule(foreground: ForegroundReader, floor: FloorHandle, probe: SystemProbe) -> FrameRule {
        FrameRule {
            foreground,
            floor,
            halvo: HalvoApp::with_sibling(SIBLING),
            probe,
        }
    }

    fn frames_of(sink: SinkHandle, foreground: ForegroundReader) -> FrameContext {
        FrameContext {
            sink,
            rule: rule(foreground, floor_on(), inactive()),
            facts: Arc::default(),
        }
    }

    fn front(bundle_id: &str, pid: libc::pid_t) -> Foreground {
        crate::macos::foreground::tests_support::front(bundle_id, pid)
    }

    fn in_front_for_test() -> (crate::macos::foreground::ForegroundWriter, ForegroundReader) {
        let (writer, reader) = foreground_state_for_test();
        writer.set_for_test(Some(front("com.example.editor", 41)));
        (writer, reader)
    }

    fn output(
        sink: SinkHandle,
    ) -> (
        Retained<ScreenOutput>,
        crate::macos::foreground::ForegroundWriter,
    ) {
        let (writer, reader) = in_front_for_test();
        (ScreenOutput::new(frames_of(sink, reader)), writer)
    }

    fn screen_ivars(
        probe: SystemProbe,
    ) -> (
        ScreenIvars,
        SinkWriter,
        crate::macos::foreground::ForegroundWriter,
    ) {
        let (handle, writer) = sink::channel(8);
        let (fg_writer, reader) = in_front_for_test();
        let ivars = ScreenIvars {
            sink: Mutex::new(Some(handle)),
            rule: rule(reader, floor_on(), probe),
            facts: Arc::default(),
        };
        (ivars, writer, fg_writer)
    }

    fn kinds_until_closed(writer: &SinkWriter) -> Vec<Modality> {
        std::iter::from_fn(|| writer.recv_timeout(Duration::from_secs(1)).ok())
            .map(|received| received.event().modality)
            .collect()
    }

    fn records(ivars: ScreenIvars, writer: &SinkWriter) -> usize {
        drop(ivars);
        kinds_until_closed(writer).len()
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

    #[test]
    fn the_frame_cap_is_the_key_screen_frames_max_per_s_of_the_bounds_file() {
        let bounds = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/baseline/r1-gate-bounds.txt"
        ))
        .expect("the bounds file reads");
        let values: Vec<&str> = bounds
            .lines()
            .filter_map(|line| line.strip_prefix("screen_frames_max_per_s="))
            .collect();
        assert_eq!(values.len(), 1, "the bounds file holds the key once");
        assert_eq!(
            values[0].trim().parse::<i32>(),
            Ok(FrameRateCap::MAX_PER_SECOND)
        );
        assert_eq!(SCREEN_FRAME_CAP.per_second(), FrameRateCap::MAX_PER_SECOND);
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
        fn CMSampleBufferCreate(
            allocator: *const c_void,
            data: *const c_void,
            data_ready: u8,
            make_ready: *const c_void,
            refcon: *const c_void,
            format: *const c_void,
            samples: isize,
            timing_entries: isize,
            timing: *const CMSampleTimingInfo,
            size_entries: isize,
            sizes: *const usize,
            sample: *mut *mut c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFDictionarySetValue(
            dictionary: *const c_void,
            key: *const c_void,
            value: *const c_void,
        );
    }

    const PIXEL_FORMAT_BGRA: u32 = 0x4247_5241;

    fn timing() -> CMSampleTimingInfo {
        let invalid = CMTime {
            value: 0,
            timescale: 0,
            flags: CMTimeFlags(0),
            epoch: 0,
        };
        CMSampleTimingInfo {
            duration: invalid,
            presentationTimeStamp: CMTime {
                value: 0,
                timescale: 2,
                flags: CMTimeFlags::Valid,
                epoch: 0,
            },
            decodeTimeStamp: invalid,
        }
    }

    struct Frame {
        image: Option<*mut c_void>,
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
            let timing = timing();
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
            Self {
                image: Some(image),
                sample,
            }
        }

        fn imageless() -> Self {
            let timing = timing();
            let mut sample = std::ptr::null_mut();
            // SAFETY: no data buffer and no format are passed, with data ready,
            // one sample and one timing entry that lives past the call;
            // `sample` is a valid place for the new sample buffer.
            let status = unsafe {
                CMSampleBufferCreate(
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                    1,
                    &timing,
                    0,
                    std::ptr::null(),
                    &mut sample,
                )
            };
            assert_eq!(status, 0, "CMSampleBufferCreate");
            Self {
                image: None,
                sample,
            }
        }

        fn with_status(self, status: NSInteger) -> Self {
            // SAFETY: `sample` is a live sample buffer that this frame owns;
            // with create set to 1 the getter makes the array of attachments
            // that the buffer then owns, and returns it at +0.
            let array = unsafe { CMSampleBufferGetSampleAttachmentsArray(self.sample, 1) };
            assert!(!array.is_null(), "the sample has no attachments array");
            // SAFETY: `array` is live and holds one mutable dictionary for the
            // one sample.
            let attachments = unsafe { CFArrayGetValueAtIndex(array, 0) };
            let number = CFNumber::from(status as i64);
            // SAFETY: SCStreamFrameInfoStatus is a constant NSString that
            // ScreenCaptureKit exports and never writes.
            let key: &NSString = unsafe { SCStreamFrameInfoStatus };
            // SAFETY: `attachments` is a live mutable dictionary of the sample;
            // the dictionary retains the key and the value that it is given.
            unsafe {
                CFDictionarySetValue(
                    attachments,
                    (key as *const NSString).cast(),
                    number.as_CFTypeRef(),
                )
            };
            self
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
            // SAFETY: both are +1 references that the constructors created and
            // that this frame owns alone.
            unsafe {
                CFRelease(self.sample);
                if let Some(image) = self.image {
                    CFRelease(image);
                }
            }
        }
    }

    fn complete() -> Frame {
        Frame::new(16, 16).with_status(SCFrameStatus::Complete.0)
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

    fn deliver_stop(output: &ScreenOutput, code: NSInteger) {
        let domain = NSString::from_str("SCStreamErrorDomain");
        // SAFETY: a class method that makes an error of the domain and code
        // given, with no user info.
        let error = unsafe { NSError::errorWithDomain_code_userInfo(&domain, code, None) };
        let stream = SCStream::alloc();
        let stream_ptr = Allocated::as_ptr(&stream);
        // SAFETY: ScreenOutput implements this delegate selector with these
        // argument types; the method never reads its stream argument, which
        // points to an allocated SCStream that stays allocated, and `error` is
        // live for the call.
        unsafe {
            let _: () = msg_send![output, stream: stream_ptr, didStopWithError: &*error];
        }
        std::mem::forget(stream);
    }

    #[test]
    fn each_frame_status_maps_to_its_action() {
        let cases = [
            (Some(SCFrameStatus::Complete.0), true, FrameAction::Record),
            (Some(SCFrameStatus::Started.0), true, FrameAction::Record),
            (Some(SCFrameStatus::Complete.0), false, FrameAction::Ignore),
            (Some(SCFrameStatus::Started.0), false, FrameAction::Ignore),
            (Some(SCFrameStatus::Idle.0), true, FrameAction::Quiet),
            (Some(SCFrameStatus::Idle.0), false, FrameAction::Quiet),
            (
                Some(SCFrameStatus::Blank.0),
                true,
                FrameAction::Impaired(HealthReason::StreamBlank),
            ),
            (
                Some(SCFrameStatus::Suspended.0),
                true,
                FrameAction::Impaired(HealthReason::StreamSuspended),
            ),
            (Some(SCFrameStatus::Stopped.0), true, FrameAction::Ignore),
            (Some(99), true, FrameAction::Ignore),
            (None, true, FrameAction::Ignore),
            (None, false, FrameAction::Ignore),
        ];
        for (status, image, action) in cases {
            assert_eq!(frame_action(status, image), action, "{status:?} {image}");
        }
    }

    #[test]
    fn with_screen_activity_on_only_a_complete_or_started_frame_with_an_image_gives_one_record() {
        let statuses = [
            (Some(SCFrameStatus::Complete.0), 1),
            (Some(SCFrameStatus::Started.0), 1),
            (Some(SCFrameStatus::Idle.0), 0),
            (Some(SCFrameStatus::Blank.0), 0),
            (Some(SCFrameStatus::Suspended.0), 0),
            (Some(SCFrameStatus::Stopped.0), 0),
            (None, 0),
        ];
        for (status, wanted) in statuses {
            let (ivars, writer, _front) = screen_ivars(inactive());
            let frame = match status {
                Some(status) => Frame::new(16, 16).with_status(status),
                None => Frame::new(16, 16),
            };
            assert_eq!(frame_status(frame.sample()), status, "the status read back");
            on_sample(&ivars, frame.sample(), SCStreamOutputType::Screen);
            assert_eq!(records(ivars, &writer), wanted, "{status:?}");
        }
    }

    #[test]
    fn a_complete_or_started_sample_with_no_image_buffer_gives_no_record() {
        for status in [SCFrameStatus::Complete.0, SCFrameStatus::Started.0] {
            let (ivars, writer, _front) = screen_ivars(inactive());
            let frame = Frame::imageless().with_status(status);
            assert!(!has_image(frame.sample()));
            on_sample(&ivars, frame.sample(), SCStreamOutputType::Screen);
            assert_eq!(records(ivars, &writer), 0, "{status}");
        }
    }

    #[test]
    fn a_blank_or_suspended_frame_reads_impaired_and_the_next_idle_frame_reads_running() {
        let (ivars, _writer, _front) = screen_ivars(inactive());
        let mut seen = Vec::new();
        for status in [
            SCFrameStatus::Blank.0,
            SCFrameStatus::Idle.0,
            SCFrameStatus::Suspended.0,
            SCFrameStatus::Stopped.0,
            SCFrameStatus::Complete.0,
        ] {
            let frame = Frame::new(16, 16).with_status(status);
            on_sample(&ivars, frame.sample(), SCStreamOutputType::Screen);
            seen.push(ivars.facts.state());
        }
        let impaired = |reason| SourceState::Impaired { reason };
        assert_eq!(
            seen,
            [
                impaired(HealthReason::StreamBlank),
                SourceState::Running,
                impaired(HealthReason::StreamSuspended),
                impaired(HealthReason::StreamSuspended),
                SourceState::Running,
            ]
        );
    }

    #[test]
    fn a_frame_while_secure_input_reads_active_gives_no_record_and_while_it_reads_unknown_one() {
        let active = SystemProbe::with_readers(at_console, byte_one);
        let (ivars, writer, _front) = screen_ivars(active);
        on_sample(&ivars, complete().sample(), SCStreamOutputType::Screen);
        assert_eq!(records(ivars, &writer), 0, "known active");
        let unknown = SystemProbe::with_readers(not_at_console, byte_one);
        let (ivars, writer, _front) = screen_ivars(unknown);
        on_sample(&ivars, complete().sample(), SCStreamOutputType::Screen);
        assert_eq!(records(ivars, &writer), 1, "unknown");
    }

    #[test]
    fn a_frame_is_dropped_while_nothing_is_known_in_front_halvo_or_an_excluded_app_is_in_front() {
        let (handle, writer) = sink::channel(16);
        let (fg_writer, reader) = foreground_state_for_test();
        let ivars = ScreenIvars {
            sink: Mutex::new(Some(handle)),
            rule: rule(reader, floor_on(), inactive()),
            facts: Arc::default(),
        };
        let frame = complete();
        let mut taken = Vec::new();
        let unbundled = Foreground {
            identity: crate::macos::foreground::tests_support::unbundled(SIBLING),
            pid: 77,
        };
        let fronts = [
            ("before the first poll", None),
            ("Halvo by bundle id", Some(front(HalvoApp::BUNDLE_ID, 70))),
            ("Halvo by its sibling executable", Some(unbundled)),
            ("a password manager", Some(front("com.apple.Passwords", 71))),
            ("an editor", Some(front("com.example.editor", 72))),
            ("nothing in front", None),
            ("Halvo again", Some(front(HalvoApp::BUNDLE_ID, 70))),
            ("the editor again", Some(front("com.example.editor", 72))),
        ];
        for (what, now) in fronts {
            if what != "before the first poll" {
                fg_writer.set_for_test(now);
            }
            let before = writer.try_recv().is_ok();
            assert!(!before, "a record before {what}");
            on_sample(&ivars, frame.sample(), SCStreamOutputType::Screen);
            taken.push((what, writer.try_recv().is_ok()));
        }
        assert_eq!(
            taken,
            [
                ("before the first poll", false),
                ("Halvo by bundle id", false),
                ("Halvo by its sibling executable", false),
                ("a password manager", false),
                ("an editor", true),
                ("nothing in front", false),
                ("Halvo again", false),
                ("the editor again", true),
            ]
        );
    }

    mod adversary1 {
        use super::*;

        #[test]
        fn adv1_a_frame_after_the_front_became_a_password_manager_gives_no_record() {
            use crate::macos::foreground::tests_support::polled_source;
            use crate::macos::foreground::FOREGROUND_POLL;
            let system_front = Arc::new(Mutex::new(Some(front("com.example.editor", 72))));
            let (mut foreground, reader) = polled_source(system_front.clone());
            let (fg_handle, _fg_sink) = sink::channel(64);
            foreground
                .begin(SourceContext::new(fg_handle))
                .expect("the foreground source starts");
            let started = Instant::now();
            while reader.get().map(|front| front.pid) != Some(72) {
                assert!(started.elapsed() < Duration::from_secs(5), "no first poll");
                std::thread::sleep(Duration::from_millis(1));
            }
            let (handle, writer) = sink::channel(16);
            let ivars = ScreenIvars {
                sink: Mutex::new(Some(handle)),
                rule: rule(reader.clone(), floor_on(), inactive()),
                facts: Arc::default(),
            };
            *system_front.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(front("com.apple.Passwords", 71));
            let switched = Instant::now();
            on_sample(&ivars, complete().sample(), SCStreamOutputType::Screen);
            let first = writer.try_recv().ok().map(|received| {
                let floor = received.into_floor().expect("a floor value");
                (
                    floor.event().kind(),
                    floor.event().pid(),
                    floor.event().app().is_listed(&["com.example.editor"], &[]),
                )
            });
            let first_at = switched.elapsed();
            while reader.get().map(|front| front.pid) != Some(71) {
                assert!(switched.elapsed() < Duration::from_secs(5), "no later poll");
                std::thread::sleep(Duration::from_millis(1));
            }
            let seen_after = switched.elapsed();
            on_sample(&ivars, complete().sample(), SCStreamOutputType::Screen);
            let second = writer.try_recv().is_ok();
            let shut = foreground.shutdown();
            println!(
                "adversary1 stale-front: frame_at={first_at:?} first_record={first:?} \
                 reader_saw_the_password_manager_after={seen_after:?} second_record={second} \
                 FOREGROUND_POLL={FOREGROUND_POLL:?} foreground_shutdown={shut:?}"
            );
            assert_eq!(
                first, None,
                "a screen_frame record was made while the password manager was in front"
            );
        }
    }

    /// Starts a foreground source that polls `system_front`, waits for its first
    /// answer of `before`, makes `after` the system's app in front, and hands
    /// one complete frame at once to an output whose rule decides on `floor`:
    /// the pid of the record that the frame made, or None.
    fn a_frame_right_after_a_switch(
        before: Foreground,
        after: Foreground,
        floor: FloorHandle,
    ) -> Option<ProcessId> {
        use crate::macos::foreground::tests_support::polled_source;
        let pid = before.pid;
        let system_front = Arc::new(Mutex::new(Some(before)));
        let (mut foreground, reader) = polled_source(system_front.clone());
        let (fg_handle, _fg_sink) = sink::channel(64);
        foreground
            .begin(SourceContext::new(fg_handle))
            .expect("the foreground source starts");
        let started = Instant::now();
        while reader.get().map(|front| front.pid) != Some(pid) {
            assert!(started.elapsed() < Duration::from_secs(5), "no first poll");
            std::thread::sleep(Duration::from_millis(1));
        }
        let (handle, writer) = sink::channel(16);
        let ivars = ScreenIvars {
            sink: Mutex::new(Some(handle)),
            rule: rule(reader.clone(), floor, inactive()),
            facts: Arc::default(),
        };
        *system_front.lock().unwrap_or_else(PoisonError::into_inner) = Some(after);
        on_sample(&ivars, complete().sample(), SCStreamOutputType::Screen);
        let record = writer
            .try_recv()
            .ok()
            .map(|received| received.into_floor().expect("a floor value").event().pid());
        assert_eq!(
            reader.get().map(|front| front.pid),
            Some(pid),
            "the poll had not read the switch when the frame came"
        );
        assert_eq!(foreground.shutdown(), Ok(()));
        record
    }

    #[test]
    fn a_frame_right_after_a_switch_to_halvos_app_or_to_a_process_under_the_root_gives_no_record() {
        let editor = || front("com.example.editor", 72);
        let rooted = FloorHandle::new(Some(ProcessId::new(500)), Arc::new(DeepTree), switches_on());
        let sibling = Foreground {
            identity: crate::macos::foreground::tests_support::unbundled(SIBLING),
            pid: 77,
        };
        assert_eq!(
            a_frame_right_after_a_switch(editor(), front(HalvoApp::BUNDLE_ID, 70), floor_on()),
            None,
            "Halvo's app by its bundle id"
        );
        assert_eq!(
            a_frame_right_after_a_switch(editor(), sibling, floor_on()),
            None,
            "Halvo's app by its sibling executable"
        );
        assert_eq!(
            a_frame_right_after_a_switch(editor(), front("com.example.helper", 502), rooted),
            None,
            "a process under the root"
        );
    }

    #[test]
    fn the_record_comes_again_when_the_read_at_the_frame_names_an_allowed_program() {
        use crate::macos::foreground::tests_support::polled_source;
        let system_front = Arc::new(Mutex::new(Some(front("com.example.editor", 72))));
        let (_foreground, reader) = polled_source(system_front);
        let (handle, writer) = sink::channel(16);
        let ivars = ScreenIvars {
            sink: Mutex::new(Some(handle)),
            rule: rule(reader, floor_on(), inactive()),
            facts: Arc::default(),
        };
        on_sample(&ivars, complete().sample(), SCStreamOutputType::Screen);
        assert_eq!(
            records(ivars, &writer),
            0,
            "a frame before the poll has answered once, while the read names an editor"
        );
        assert_eq!(
            a_frame_right_after_a_switch(
                front("com.apple.Passwords", 71),
                front("com.example.editor", 72),
                floor_on()
            ),
            Some(ProcessId::new(72)),
            "the editor in front at the frame, the password manager at the last poll"
        );
    }

    #[test]
    fn a_record_carries_the_app_in_front_and_its_pid() {
        let (ivars, writer, _front) = screen_ivars(inactive());
        on_sample(&ivars, complete().sample(), SCStreamOutputType::Screen);
        drop(ivars);
        let received = writer
            .recv_timeout(Duration::from_secs(1))
            .expect("one record");
        let floor = received.into_floor().expect("a floor value");
        let event = floor.event();
        assert_eq!(event.kind(), Modality::ScreenFrame);
        assert_eq!(event.pid(), ProcessId::new(41));
        assert!(event.app().is_listed(&["com.example.editor"], &[]));
    }

    struct Tree;

    impl Lineage for Tree {
        fn parent_of(&self, pid: ProcessId) -> Parent {
            match pid.get() {
                501 => Parent::Of(ProcessId::new(500)),
                502 => Parent::Of(ProcessId::new(501)),
                _ => Parent::None,
            }
        }
    }

    fn selection_under(root: Option<u32>) -> Selection {
        Selection {
            own: std::process::id() as libc::pid_t,
            halvo: HalvoApp::with_sibling(SIBLING),
            floor: FloorHandle::new(root.map(ProcessId::new), Arc::new(Tree), switches_on()),
            probe: inactive(),
        }
    }

    type Entry = (libc::pid_t, &'static str, Option<&'static str>);

    fn entry(app: &Entry) -> AppEntry {
        AppEntry {
            pid: app.0,
            bundle_id: app.1.to_owned(),
            path: app.2.map(str::to_owned),
        }
    }

    #[test]
    fn the_selection_excludes_halvo_the_own_process_and_the_root_tree_and_keeps_every_other_entry()
    {
        let own = std::process::id() as libc::pid_t;
        let apps: [Entry; 9] = [
            (900, "", Some(SIBLING)),
            (
                902,
                "com.example.editor",
                Some("/Applications/Editor.app/Contents/MacOS/Editor"),
            ),
            (
                901,
                HalvoApp::BUNDLE_ID,
                Some("/Applications/Halvo.app/Contents/MacOS/Halvo"),
            ),
            (own, "", None),
            (501, "com.example.child", None),
            (903, "com.apple.Passwords", None),
            (502, "", Some("/opt/halvo-test/helper")),
            (904, "", Some("/opt/elsewhere/halvo")),
            (1, "com.apple.finder", None),
        ];
        let excluded: Vec<libc::pid_t> =
            excluded_applications(apps, entry, &selection_under(Some(500)))
                .iter()
                .map(|app| app.0)
                .collect();
        assert_eq!(excluded, [900, 901, own, 501, 502]);
        let without_root: Vec<libc::pid_t> =
            excluded_applications(apps, entry, &selection_under(None))
                .iter()
                .map(|app| app.0)
                .collect();
        assert_eq!(without_root, [900, 901, own]);
    }

    fn utf16_nsstring(units: &[u16]) -> core_foundation::string::CFString {
        use core_foundation::base::kCFAllocatorDefault;
        use core_foundation::string::{CFString, CFStringCreateWithCharacters};
        // SAFETY: `units` holds UTF-16 code units, an unpaired surrogate among
        // them, and the call copies them into a new string at +1.
        unsafe {
            CFString::wrap_under_create_rule(CFStringCreateWithCharacters(
                kCFAllocatorDefault,
                units.as_ptr(),
                units.len() as isize,
            ))
        }
    }

    fn as_ns(text: &core_foundation::string::CFString) -> &NSString {
        // SAFETY: a CFString is toll-free bridged with NSString, and the
        // reference borrows `text`, which stays alive.
        unsafe { &*(text.as_concrete_TypeRef() as *const NSString) }
    }

    #[test]
    fn a_bundle_id_with_an_unpaired_surrogate_is_read_lossily_at_both_call_sites_and_aborts_nothing(
    ) {
        let evil = utf16_nsstring(&[0x63, 0x6F, 0x6D, 0x2E, 0x65, 0x76, 0x69, 0x6C, 0xD800]);
        let own = std::process::id() as libc::pid_t;
        let entry = entry_from(own, as_ns(&evil));
        assert_eq!(
            entry.bundle_id, "com.evil\u{FFFD}",
            "the entry of the shareable content"
        );
        let halvo = HalvoApp::with_sibling(SIBLING);
        assert_eq!(
            halvo_pid_of(&halvo, own, Some(as_ns(&evil))),
            None,
            "a running app"
        );
        let ours = utf16_nsstring(&HalvoApp::BUNDLE_ID.encode_utf16().collect::<Vec<u16>>());
        assert_eq!(halvo_pid_of(&halvo, 77, Some(as_ns(&ours))), Some(77));
        assert_eq!(halvo_pid_of(&halvo, 77, None), None);
    }

    #[test]
    fn an_unbundled_halvo_in_front_whose_path_is_longer_than_the_cap_gives_no_record() {
        let long_sibling = format!("/{}/halvo", "a".repeat(1040));
        let (handle, writer) = sink::channel(8);
        let (fg_writer, fg_reader) = foreground_state_for_test();
        fg_writer.set_for_test(Some(Foreground {
            identity: crate::macos::foreground::tests_support::unbundled(&long_sibling),
            pid: 920,
        }));
        let ivars = ScreenIvars {
            sink: Mutex::new(Some(handle)),
            rule: FrameRule {
                foreground: fg_reader,
                floor: floor_on(),
                halvo: HalvoApp::with_sibling(&long_sibling),
                probe: inactive(),
            },
            facts: Arc::default(),
        };
        on_sample(&ivars, complete().sample(), SCStreamOutputType::Screen);
        assert_eq!(
            records(ivars, &writer),
            0,
            "a record while the long sibling was in front"
        );
    }

    #[test]
    fn the_chooser_takes_the_main_display_by_its_id_wherever_it_stands() {
        assert_eq!(chosen_display(&[7, 9], 7), Some(0), "main first");
        assert_eq!(chosen_display(&[9, 7], 7), Some(1), "main second");
        assert_eq!(chosen_display(&[9, 8], 7), Some(0), "main absent");
        assert_eq!(chosen_display(&[], 7), None, "no display");
    }

    #[test]
    fn a_rebuild_is_due_while_the_app_runs_unlisted_at_once_in_front_and_after_a_doubling_backoff()
    {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let floor = FloorHandle::default();
        let probe = inactive();
        let no_root = || RootTree {
            floor: &floor,
            probe: &probe,
        };
        let mut plan = RebuildPlan::new();
        assert!(
            !plan.due(&[], &[], false, &[], &no_root(), at(0)),
            "no app runs"
        );
        assert!(
            !plan.due(&[42], &[42], false, &[], &no_root(), at(0)),
            "the filter lists the app"
        );
        assert!(
            plan.due(&[], &[42], false, &[], &no_root(), at(0)),
            "the app runs unlisted"
        );
        plan.tried(at(0), false);
        assert!(
            !plan.due(&[], &[42], false, &[], &no_root(), at(999)),
            "inside the first second"
        );
        assert!(
            plan.due(&[], &[42], false, &[], &no_root(), at(1_000)),
            "after one second"
        );
        plan.tried(at(1_000), false);
        assert!(
            !plan.due(&[], &[42], false, &[], &no_root(), at(2_999)),
            "inside two seconds"
        );
        assert!(
            plan.due(&[], &[42], true, &[], &no_root(), at(1_001)),
            "the app comes in front"
        );
        assert!(
            !plan.due(&[], &[42], true, &[], &no_root(), at(1_002)),
            "the app stays in front"
        );
        assert!(
            plan.due(&[], &[42], true, &[], &no_root(), at(3_000)),
            "after two seconds"
        );
        let mut now = 3_000;
        for _ in 0..10 {
            plan.tried(at(now), false);
            now += 60_000;
        }
        assert!(
            !plan.due(&[], &[42], true, &[], &no_root(), at(now - 1)),
            "the wait stays at sixty seconds"
        );
        assert!(plan.due(&[], &[42], true, &[], &no_root(), at(now)));
        plan.tried(at(now), true);
        assert!(
            plan.due(&[42], &[42, 43], false, &[], &no_root(), at(now)),
            "a new pid after a relaunch"
        );
        assert_eq!(
            plan.backoff, REBUILD_FIRST_BACKOFF,
            "a listing resets the wait"
        );
    }

    struct DeepTree;

    impl Lineage for DeepTree {
        fn parent_of(&self, pid: ProcessId) -> Parent {
            match pid.get() {
                501 => Parent::Of(ProcessId::new(500)),
                502 => Parent::Of(ProcessId::new(501)),
                777 => Parent::Unknown,
                _ => Parent::None,
            }
        }
    }

    #[test]
    fn a_rebuild_is_due_while_a_process_under_the_root_runs_unlisted_with_a_wait_of_its_own() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let probe = inactive();
        let rooted = FloorHandle::new(Some(ProcessId::new(500)), Arc::new(DeepTree), switches_on());
        let tree = RootTree {
            floor: &rooted,
            probe: &probe,
        };
        let no_floor = FloorHandle::new(None, Arc::new(DeepTree), switches_on());
        let no_root = RootTree {
            floor: &no_floor,
            probe: &probe,
        };
        for pid in [500, 501, 502] {
            let mut plan = RebuildPlan::new();
            assert!(
                plan.due(&[], &[], false, &[pid, 903], &tree, at(0)),
                "{pid} unlisted"
            );
            let mut plan = RebuildPlan::new();
            assert!(
                !plan.due(&[pid], &[], false, &[pid, 903], &tree, at(0)),
                "{pid} listed"
            );
            let mut plan = RebuildPlan::new();
            assert!(
                !plan.due(&[], &[], false, &[pid], &no_root, at(0)),
                "{pid} with no root"
            );
        }
        let mut plan = RebuildPlan::new();
        assert!(
            !plan.due(&[], &[], false, &[903, 777], &tree, at(0)),
            "a pid outside the tree and a pid whose lineage reads unknown"
        );
        let mut plan = RebuildPlan::new();
        assert!(plan.due(&[], &[], false, &[501], &tree, at(0)));
        plan.tried(at(0), false);
        assert!(
            !plan.due(&[], &[], false, &[501], &tree, at(0)),
            "the wait of 1 s"
        );
        assert!(plan.due(&[], &[], false, &[501], &tree, at(1)), "after 1 s");
        plan.tried(at(1), false);
        assert!(
            !plan.due(&[], &[], false, &[501], &tree, at(2)),
            "the wait has doubled"
        );
        assert!(
            plan.due(&[], &[], false, &[501], &tree, at(3)),
            "after 2 s more"
        );
        plan.tried(at(3), false);
        let mut now = 3;
        let mut waits = Vec::new();
        for _ in 0..8 {
            let mut wait = 1;
            while !plan.due(&[], &[], false, &[501], &tree, at(now + wait)) {
                wait += 1;
            }
            waits.push(wait);
            now += wait;
            plan.tried(at(now), false);
        }
        assert_eq!(
            waits,
            [4, 8, 16, 32, 60, 60, 60, 60],
            "the waits after each rebuild"
        );
        assert!(
            plan.due(&[], &[], false, &[501, 502], &tree, at(now)),
            "a new unlisted pid is due at once"
        );
        plan.tried(at(now), false);
        assert!(
            !plan.due(&[501], &[], false, &[501, 502], &tree, at(now)),
            "501 listed, 502 waits its first second"
        );
        assert!(
            plan.due(&[], &[], false, &[501, 502], &tree, at(now)),
            "501 unlisted again is due at once"
        );
        plan.tried(at(now), false);
        assert!(
            !plan.due(&[], &[], false, &[501], &tree, at(now)),
            "the wait of 501 starts again at 1 s, and 502 no longer runs"
        );
        assert!(plan.due(&[], &[], false, &[501], &tree, at(now + 1)));
        plan.tried(at(now + 1), false);
        assert!(
            plan.due(&[], &[], false, &[501, 502], &tree, at(now + 1)),
            "502 runs again after it ran no more: due at once"
        );
        let mut halvo = RebuildPlan::new();
        assert!(halvo.due(&[], &[42], false, &[42, 501], &tree, at(0)));
        halvo.tried(at(0), true);
        assert!(
            !halvo.due(&[42], &[42], false, &[42, 501], &tree, at(0)),
            "Halvo listed: the reset of its own wait leaves the wait of 501 as it was"
        );
        assert!(halvo.due(&[42], &[42], false, &[42, 501], &tree, at(1)));
    }

    #[test]
    fn the_delegate_reads_stop_code_minus_3817_as_stopped_by_the_user_and_any_other_as_failed() {
        let (handle, _writer) = sink::channel(8);
        let (user, _front) = output(handle.clone());
        deliver_stop(&user, -3817);
        assert_eq!(
            user.ivars().facts.state(),
            SourceState::Stopped(StopCause::User)
        );
        let (other, _front) = output(handle);
        deliver_stop(&other, -3821);
        assert_eq!(
            other.ivars().facts.state(),
            SourceState::Failed {
                reason: HealthReason::OsError,
                error: CaptureError::Os {
                    stage: "stream_stopped",
                    domain: "ScreenCaptureKit",
                    code: -3821
                }
            }
        );
    }

    #[test]
    fn a_started_source_reads_the_stop_code_of_its_delegate() {
        let (mut source, _handle, _writer, _asks, _weak) = started(&[Some(StopReply::Stopped)]);
        assert_eq!(source.health().state, SourceState::Running);
        deliver_stop(
            source
                .output
                .as_ref()
                .expect("a started source holds its output"),
            -3817,
        );
        assert_eq!(source.health().state, SourceState::Stopped(StopCause::User));
        assert_eq!(source.shutdown(), Ok(()));
    }

    #[test]
    fn a_kept_output_stays_alive() {
        let (handle, _writer) = sink::channel(8);
        let (output, _front) = output(handle);
        let weak = Weak::from_retained(&output);
        keep_for_the_life_of_the_process(Some(output), None);
        assert!(weak.load().is_some(), "the kept output was released");
    }

    #[test]
    fn a_kept_output_lets_the_drain_end() {
        let (handle, writer) = sink::channel(8);
        let (output, _front) = output(handle);
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
        let (output, _front) = output(handle.clone());
        keep_for_the_life_of_the_process(Some(output.clone()), None);
        deliver(&output, complete().sample());
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
        let (output, _front) = output(handle);
        deliver(&output, complete().sample());
        drop(output);
        assert_eq!(kinds_until_closed(&writer), [Modality::ScreenFrame]);
    }

    #[test]
    fn the_stream_output_message_keeps_no_reference_to_the_sample() {
        let (handle, _sink_writer) = sink::channel(8);
        let (output, _front) = output(handle);
        let frame = complete();
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

    #[test]
    fn a_complete_frame_gives_exactly_one_screen_frame_event() {
        let (ivars, writer, _front) = screen_ivars(inactive());
        on_sample(&ivars, complete().sample(), SCStreamOutputType::Screen);
        drop(ivars);
        assert_eq!(kinds_until_closed(&writer), [Modality::ScreenFrame]);
    }

    #[test]
    fn the_frame_handler_keeps_no_reference_to_the_sample() {
        let (ivars, _writer, _front) = screen_ivars(inactive());
        let frame = complete();
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
                    reply.send(answer).ok();
                }
                None => self.held.borrow_mut().push(reply),
            }
        }
    }

    type Opener =
        Box<dyn FnOnce(FrameContext, Selection, FrameRateCap) -> Result<Opened, CaptureError>>;

    fn with_stream(stream: impl StopStream + 'static) -> Opener {
        Box::new(move |frames: FrameContext, _selection, _cap| {
            frames.facts.filter(Vec::new(), false);
            Ok((
                Box::new(stream) as Box<dyn StopStream>,
                ScreenOutput::new(frames),
                DispatchQueue::new("scripted", None),
                None,
            ))
        })
    }

    fn scripted(answers: &[Option<StopReply>], asks: &Rc<Cell<u32>>) -> Opener {
        with_stream(ScriptedStream {
            answers: Rc::new(RefCell::new(answers.iter().copied().collect())),
            asks: asks.clone(),
            held: RefCell::new(Vec::new()),
        })
    }

    fn source() -> ScreenSource {
        let (_writer, reader) = foreground_state_for_test();
        let mut source = ScreenSource::new(SCREEN_FRAME_CAP, reader);
        source.probe = inactive();
        source
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
        let mut source = source();
        let asks = Rc::new(Cell::new(0));
        let first = source.begin_with(&context_on(handle.clone()), scripted(answers, &asks));
        assert_eq!(first, Ok(Started::Running));
        let weak = Weak::from_retained(
            source
                .output
                .as_ref()
                .expect("a started source holds its output"),
        );
        (source, handle, writer, asks, weak)
    }

    fn counting_opener(opened: &Rc<Cell<u32>>) -> Opener {
        let opened = opened.clone();
        Box::new(move |_frames, _selection, _cap| {
            opened.set(opened.get() + 1);
            Err(CaptureError::Start(
                "the counting opener opens nothing".into(),
            ))
        })
    }

    #[test]
    fn a_source_with_no_stream_calls_its_opener_once() {
        let (handle, _writer) = sink::channel(8);
        let mut source = source();
        let opened = Rc::new(Cell::new(0));
        let begun = source.begin_with(&context_on(handle), counting_opener(&opened));
        assert_eq!(opened.get(), 1);
        assert_eq!(
            begun,
            Err(CaptureError::Start(
                "the counting opener opens nothing".into()
            ))
        );
    }

    #[test]
    fn with_the_switch_off_and_with_the_default_switches_begin_answers_off_and_opens_no_stream() {
        let mut off = switches_on();
        off.set(Switch::ScreenActivity, false);
        assert!(!Switches::default().is_on(Switch::ScreenActivity));
        for (what, switches) in [
            ("the switch off", Some(off)),
            ("core-2's default set", None),
        ] {
            let (handle, writer) = sink::channel(8);
            let context = match switches {
                Some(switches) => SourceContext::with_floor(
                    handle.clone(),
                    FloorHandle::new(None, Arc::new(NoLineage), switches),
                ),
                None => SourceContext::new(handle.clone()),
            };
            let mut source = source();
            let opened = Rc::new(Cell::new(0));
            let begun = source.begin_with(&context, counting_opener(&opened));
            drop(context);
            let health = source.health().state;
            let fact = source.filter_excludes_app();
            let shut = source.shutdown();
            drop(handle);
            assert_eq!(begun, Ok(Started::Off), "{what}");
            assert_eq!(opened.get(), 0, "{what}: a stream was opened");
            assert_eq!(health, SourceState::Off, "{what}");
            assert_eq!(fact, None, "{what}: no stream, no filter fact");
            assert_eq!(shut, Ok(()), "{what}");
            assert_eq!(kinds_until_closed(&writer), [], "{what}: a record");
        }
    }

    #[test]
    fn the_filter_fact_reads_no_stream_before_a_start_and_after_a_stop_and_else_the_filters_answer()
    {
        let (handle, _writer) = sink::channel(8);
        let mut source = source();
        assert_eq!(source.filter_excludes_app(), None, "before a start");
        let asks = Rc::new(Cell::new(0));
        source
            .begin_with(
                &context_on(handle),
                scripted(&[Some(StopReply::Stopped)], &asks),
            )
            .expect("the source starts");
        assert_eq!(
            source.filter_excludes_app(),
            Some(false),
            "a filter without the app"
        );
        source.facts.filter(vec![42], true);
        assert_eq!(
            source.filter_excludes_app(),
            Some(true),
            "a filter that lists the app"
        );
        assert_eq!(source.shutdown(), Ok(()));
        assert_eq!(source.filter_excludes_app(), None, "after a stop");
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
        let opened = Rc::new(Cell::new(0));
        let second = source.begin_with(&context_on(handle.clone()), counting_opener(&opened));
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
        let opened = Rc::new(Cell::new(0));
        assert_eq!(
            source.begin_with(&context_on(handle.clone()), counting_opener(&opened)),
            Err(CaptureError::AlreadyStarted)
        );
        assert_eq!(opened.get(), 0, "a begin after the timeout opened a stream");
        assert!(
            sink_closed(handle, &writer),
            "the kept output still holds the sink"
        );
        assert_eq!(
            source.health().state,
            SourceState::Failed {
                reason: crate::source::HealthReason::Timeout,
                error: CaptureError::Timeout {
                    stage: "stop_capture",
                    ms: 5000
                }
            }
        );
        let dropped = Instant::now();
        drop(source);
        let drop_took = dropped.elapsed();
        assert_eq!(asks.get(), 1, "the drop asked the stream to stop again");
        assert!(
            drop_took < Duration::from_millis(100),
            "the drop after a Timeout waited {drop_took:?}"
        );
    }

    #[test]
    fn a_stop_that_answered_an_error_reads_failed_os_error_and_its_drop_asks_nothing() {
        let (mut source, handle, writer, asks, weak) = started(&[
            Some(StopReply::Failed { code: -3808 }),
            Some(StopReply::Stopped),
        ]);
        let shut = source.shutdown();
        let state = source.health().state;
        let dropped = Instant::now();
        drop(source);
        let drop_took = dropped.elapsed();
        let error = CaptureError::Os {
            stage: "stop_capture",
            domain: "ScreenCaptureKit",
            code: -3808,
        };
        assert_eq!(shut, Err(error.clone()));
        assert_eq!(
            state,
            SourceState::Failed {
                reason: crate::source::HealthReason::OsError,
                error
            }
        );
        assert_eq!(asks.get(), 1, "the drop asked the stream to stop again");
        assert!(
            drop_took < Duration::from_millis(100),
            "the drop after a failed stop waited {drop_took:?}"
        );
        assert!(
            weak.load().is_some(),
            "the output of a failed stop was released"
        );
        assert!(sink_closed(handle, &writer));
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
        let mut source = source();
        source
            .begin_with(
                &context_on(handle),
                with_stream(SlowAskStream(RefCell::new(Vec::new()))),
            )
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
        let mut source = source();
        source
            .begin_with(&context_on(handle.clone()), with_stream(DroppingStream))
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
