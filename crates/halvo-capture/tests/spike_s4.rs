//! Spike S4 (run 20261108-halvo-r1-capture): what makes the Windows capture
//! session fault. Not product code and never merged.
//!
//! Each test is ignored and runs by name in its own process. The CI loop
//! starts the binary 30 times per variant and reads the exit code. Lines start
//! with `S4 ` and hold counts, sizes, booleans, exception codes and the file
//! names of system modules only.
#![cfg(target_os = "windows")]

use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_capture::event::{CaptureStreamEvent, Modality};
use halvo_capture::sink::{channel, SinkWriter};
use halvo_capture::source::CaptureSource;
use halvo_capture::windows::screen::{exclude_window_from_capture, ScreenSource};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
    GetSystemMetrics, PeekMessageW, RegisterClassW, SetWindowPos, TranslateMessage,
    UnregisterClassW, MSG, PM_REMOVE, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_NOZORDER, WNDCLASSW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut ExceptionRecord,
    address: *mut c_void,
    params: u32,
    info: [usize; 15],
}

#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut c_void,
}

type VectoredHandler = unsafe extern "system" fn(*mut ExceptionPointers) -> i32;

#[link(name = "kernel32")]
extern "system" {
    fn AddVectoredExceptionHandler(first: u32, handler: VectoredHandler) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    fn GetStdHandle(which: u32) -> *mut c_void;
    fn WriteFile(
        file: *mut c_void,
        buffer: *const u8,
        len: u32,
        written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn GetCurrentThreadId() -> u32;
    fn GetThreadId(thread: *mut c_void) -> u32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoIncrementMTAUsage(cookie: *mut *mut c_void) -> i32;
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn PostThreadMessageW(thread: u32, msg: u32, wparam: usize, lparam: isize) -> i32;
}

const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
const FROM_ADDRESS: u32 = 0x4;
const UNCHANGED_REFCOUNT: u32 = 0x2;
const COINIT_APARTMENTTHREADED: u32 = 0x2;
const WM_QUIT: u32 = 0x0012;
const MAGENTA: u32 = 0x00FF_00FF;
const GREEN: u32 = 0x0000_FF00;
const WINDOW_WIDTH: i32 = 360;
const WINDOW_HEIGHT: i32 = 240;
const VARIANT_ENV: &str = "S4_VARIANT";

static FIRST_CHANCE_PRINTED: AtomicU32 = AtomicU32::new(0);

struct Line {
    buf: [u8; 320],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            buf: [0; 320],
            len: 0,
        }
    }
    fn text(&mut self, s: &[u8]) {
        for &b in s {
            if self.len < self.buf.len() - 1 {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }
    fn hex(&mut self, mut v: usize) {
        let mut digits = [0u8; 16];
        let mut n = 0;
        loop {
            let d = (v & 0xF) as u8;
            digits[n] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
            n += 1;
            v >>= 4;
            if v == 0 {
                break;
            }
        }
        self.text(b"0x");
        while n > 0 {
            n -= 1;
            self.text(&digits[n..n + 1]);
        }
    }
    fn emit(&mut self) {
        self.text(b"\n");
        let mut written = 0u32;
        unsafe {
            WriteFile(
                GetStdHandle(STD_OUTPUT_HANDLE),
                self.buf.as_ptr(),
                self.len as u32,
                &mut written,
                core::ptr::null_mut(),
            );
        }
    }
}

/// Print the first five first-chance exceptions of the fault classes with the
/// file name of the module that holds the faulting address. It allocates
/// nothing, because the heap may be the thing that is broken.
unsafe extern "system" fn on_exception(pointers: *mut ExceptionPointers) -> i32 {
    let record = unsafe { &*(*pointers).record };
    let watched = matches!(
        record.code,
        0xC000_0005 | 0xC000_0374 | 0xC000_0409 | 0xC000_041D
    );
    if !watched || FIRST_CHANCE_PRINTED.fetch_add(1, Ordering::SeqCst) >= 5 {
        return 0;
    }
    let mut line = Line::new();
    line.text(b"S4 fault first_chance=true code=");
    line.hex(record.code as usize);
    let mut module: *mut c_void = core::ptr::null_mut();
    let found = unsafe {
        GetModuleHandleExW(
            FROM_ADDRESS | UNCHANGED_REFCOUNT,
            record.address as *const u16,
            &mut module,
        )
    } != 0;
    if found {
        let mut name = [0u16; 260];
        let n = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), 260) } as usize;
        let start = name[..n]
            .iter()
            .rposition(|&c| c == b'\\' as u16)
            .map_or(0, |i| i + 1);
        line.text(b" module=");
        for &c in &name[start..n] {
            let b = if c < 128 { c as u8 } else { b'?' };
            line.text(&[b]);
        }
        line.text(b" offset=");
        line.hex(record.address as usize - module as usize);
    } else {
        line.text(b" module=<none> address=");
        line.hex(record.address as usize);
    }
    if record.code == 0xC000_0005 && record.params >= 2 {
        line.text(b" access=");
        line.hex(record.info[0]);
        line.text(b" target_is_address=");
        line.text(if record.info[1] == record.address as usize {
            b"true"
        } else {
            b"false"
        });
    }
    line.text(b" thread=");
    line.hex(unsafe { GetCurrentThreadId() } as usize);
    line.emit();
    0
}

fn install_fault_line() {
    unsafe {
        AddVectoredExceptionHandler(1, on_exception);
    }
}

fn say(test: &str, message: impl AsRef<str>) {
    println!("S4 {test} {}", message.as_ref());
}

fn show<T, E: std::fmt::Display>(result: &Result<T, E>) -> String {
    match result {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("error({e})"),
    }
}

fn variant() -> String {
    std::env::var(VARIANT_ENV).unwrap_or_else(|_| "baseline".to_string())
}

/// Run `body` on its own thread, as c8b did, and fail if it does not finish.
fn with_deadline<F>(test: &'static str, limit: Duration, body: F)
where
    F: FnOnce() + Send + 'static,
{
    let (done, finished) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name(format!("s4-{test}"))
        .spawn(move || {
            body();
            let _ = done.send(());
        })
        .expect("spawn the test body");
    match finished.recv_timeout(limit) {
        Ok(()) => worker.join().expect("test body finished"),
        Err(RecvTimeoutError::Timeout) => {
            say(
                test,
                format!("result=TIMEOUT limit_secs={}", limit.as_secs()),
            );
            panic!("{test} did not finish within {limit:?}");
        }
        Err(RecvTimeoutError::Disconnected) => {
            if let Err(payload) = worker.join() {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

fn pump_thread(window: Duration) {
    let deadline = Instant::now() + window;
    let mut msg = MSG::default();
    loop {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn drain_for(writer: &SinkWriter, window: Duration) -> Vec<CaptureStreamEvent> {
    let deadline = Instant::now() + window;
    let mut events = Vec::new();
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match writer.receiver().recv_timeout(deadline - now) {
            Ok(event) => events.push(event),
            Err(_) => break,
        }
    }
    events
}

fn count_frames(events: &[CaptureStreamEvent]) -> usize {
    events
        .iter()
        .filter(|e| e.modality == Modality::ScreenFrame)
        .count()
}

unsafe extern "system" fn test_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A visible topmost popup owned by the calling thread, filled with one color.
struct TestWindow {
    hwnd: HWND,
    class_name: HSTRING,
    hinstance: HINSTANCE,
    destroyed: bool,
}

impl TestWindow {
    fn create(class: &str, fill: u32, x: i32, y: i32, w: i32, h: i32) -> Result<Self, String> {
        let class_name = HSTRING::from(class);
        let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .map_err(|e| format!("GetModuleHandleW: {e}"))?
            .into();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(test_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hbrBackground: unsafe { CreateSolidBrush(COLORREF(fill)) },
            ..Default::default()
        };
        if unsafe { RegisterClassW(&wc) } == 0 {
            return Err(format!(
                "RegisterClassW: {}",
                windows::core::Error::from_thread()
            ));
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                w,
                h,
                None,
                None,
                Some(hinstance),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW: {e}"))?;
        Ok(Self {
            hwnd,
            class_name,
            hinstance,
            destroyed: false,
        })
    }

    fn magenta_popup(class: &str) -> Result<Self, String> {
        Self::create(class, MAGENTA, 100, 100, WINDOW_WIDTH, WINDOW_HEIGHT)
    }

    fn destroy(&mut self) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = UnregisterClassW(PCWSTR(self.class_name.as_ptr()), Some(self.hinstance));
        }
        pump_thread(Duration::from_millis(50));
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

struct PixelProbeFlags {
    samples: mpsc::Sender<Result<FrameSample, String>>,
    x: u32,
    y: u32,
    read_pixels: bool,
}

struct FrameSample {
    width: u32,
    height: u32,
    bgra: [u8; 4],
}

/// c8b's pixel-reading handler, with one switch: `read_pixels` false sends the
/// frame's size and calls no `frame.buffer()`.
struct PixelProbe {
    flags: PixelProbeFlags,
}

impl GraphicsCaptureApiHandler for PixelProbe {
    type Flags = PixelProbeFlags;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { flags: ctx.flags })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let (width, height) = (frame.width(), frame.height());
        let sample = if !self.flags.read_pixels {
            Ok(FrameSample {
                width,
                height,
                bgra: [0; 4],
            })
        } else {
            frame
                .buffer()
                .map_err(|e| format!("frame buffer: {e}"))
                .and_then(|mut buffer| {
                    let pitch = buffer.row_pitch() as usize;
                    let offset = self.flags.y as usize * pitch + self.flags.x as usize * 4;
                    buffer
                        .as_raw_buffer()
                        .get(offset..offset + 4)
                        .map(|px| FrameSample {
                            width,
                            height,
                            bgra: [px[0], px[1], px[2], px[3]],
                        })
                        .ok_or_else(|| "the probe pixel is outside the frame".to_string())
                })
        };
        let _ = self.flags.samples.send(sample);
        Ok(())
    }
}

/// End a capture the way the variant says: `stop()` posts WM_QUIT and joins;
/// `no-join` sets the halt flag, posts WM_QUIT to the thread and detaches it.
fn end_capture<T: GraphicsCaptureApiHandler + Send + 'static, E>(
    control: CaptureControl<T, E>,
    join: bool,
) -> String {
    if join {
        return match control.stop() {
            Ok(()) => "joined".to_string(),
            Err(_) => "stop_error".to_string(),
        };
    }
    control.halt_handle().store(true, Ordering::Relaxed);
    let handle = control.into_thread_handle();
    let thread = unsafe { GetThreadId(handle.as_raw_handle()) };
    let posted = if thread == 0 {
        false
    } else {
        let mut ok = false;
        for _ in 0..200 {
            if unsafe { PostThreadMessageW(thread, WM_QUIT, 0, 0) } != 0 {
                ok = true;
                break;
            }
            if handle.is_finished() {
                break;
            }
            std::thread::yield_now();
        }
        ok
    };
    drop(handle);
    format!("detached posted={posted}")
}

/// Capture the primary monitor and return the last sample of the pixel at
/// (`x`, `y`) seen within three seconds of the first frame. `before_stop` runs
/// after the samples are taken and before the capture ends.
fn capture_pixel(
    x: u32,
    y: u32,
    read_pixels: bool,
    join: bool,
    before_stop: impl FnOnce(),
) -> Result<FrameSample, String> {
    let (samples, received) = mpsc::channel();
    let monitor = Monitor::primary().map_err(|e| format!("primary monitor: {e}"))?;
    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        PixelProbeFlags {
            samples,
            x,
            y,
            read_pixels,
        },
    );
    let control =
        PixelProbe::start_free_threaded(settings).map_err(|e| format!("capture start: {e}"))?;
    let mut last = None;
    let mut frames = 0u32;
    let mut deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(sample) = received.try_recv() {
            if last.is_none() {
                deadline = Instant::now() + Duration::from_secs(3);
            }
            frames += 1;
            last = Some(sample);
        }
        pump_thread(Duration::from_millis(20));
    }
    before_stop();
    let ended = end_capture(control, join);
    say("probe", format!("capture frames={frames} end={ended}"));
    last.unwrap_or_else(|| Err("no frame within 10 seconds".to_string()))
}

fn is_magenta(bgra: [u8; 4]) -> bool {
    bgra[0] > 200 && bgra[1] < 60 && bgra[2] > 200
}

fn product_capture(label: &str) {
    let (sink, writer) = channel(256);
    let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
    let start = screen.start(sink.clone());
    let mut frames = 0;
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(3) {
        pump_thread(Duration::from_millis(50));
        frames += count_frames(&drain_for(&writer, Duration::from_millis(50)));
    }
    screen.stop();
    say(
        "c8b",
        format!(
            "capture={label} product_source start={} frames={frames}",
            show(&start)
        ),
    );
}

fn take_mta_reference(test: &str) {
    let mut cookie: *mut c_void = core::ptr::null_mut();
    let hr = unsafe { CoIncrementMTAUsage(&mut cookie) };
    say(
        test,
        format!("mta_reference hr={hr:#x} held_until_exit=true"),
    );
}

/// c8b of verifier-windows-functional-1 with one variable changed by
/// `S4_VARIANT` (see claims.md of the S4 evidence).
#[test]
#[ignore = "spike S4: needs an interactive Windows desktop"]
fn s4_c8b() {
    install_fault_line();
    let variant = variant();
    say("c8b", format!("begin variant={variant}"));
    if variant == "mta-ref" {
        take_mta_reference("c8b");
    }
    let v = variant.clone();
    with_deadline("c8b", Duration::from_secs(120), move || {
        if v == "sta-test-thread" {
            let hr = unsafe { CoInitializeEx(core::ptr::null_mut(), COINIT_APARTMENTTHREADED) };
            say("c8b", format!("test_thread_coinit_sta hr={hr:#x}"));
        }
        let read_pixels = v != "no-pixel-read";
        let join = v != "no-join";
        let mut window = TestWindow::magenta_popup("S4ExcludePixels").expect("window");
        pump_thread(Duration::from_millis(500));
        let (x, y) = (
            100 + WINDOW_WIDTH as u32 / 2,
            100 + WINDOW_HEIGHT as u32 / 2,
        );
        if v == "product-source" {
            product_capture("first");
        } else {
            let shown = capture_pixel(x, y, read_pixels, join, || {});
            say(
                "c8b",
                format!(
                    "phase=window-shown result={} is_window_color={}",
                    show(&shown.as_ref().map(|_| ())),
                    shown.as_ref().map(|s| is_magenta(s.bgra)).unwrap_or(false)
                ),
            );
        }
        let exclude = exclude_window_from_capture(window.hwnd);
        say(
            "c8b",
            format!("exclude_window_from_capture={}", show(&exclude)),
        );
        pump_thread(Duration::from_millis(500));
        if v == "product-source" {
            product_capture("second");
        } else {
            let window_first = v == "window-first";
            let hwnd_slot = &mut window;
            let excluded = capture_pixel(x, y, read_pixels, join, || {
                if window_first {
                    hwnd_slot.destroy();
                    say("c8b", "window_destroyed_before_stop=true");
                }
            });
            match &excluded {
                Ok(s) => say(
                    "c8b",
                    format!(
                        "phase=window-excluded frame={}x{} is_window_color={}",
                        s.width,
                        s.height,
                        is_magenta(s.bgra)
                    ),
                ),
                Err(e) => say("c8b", format!("phase=window-excluded capture_error={e}")),
            }
        }
        window.destroy();
        say("c8b", "teardown window_destroyed=true");
        std::thread::sleep(Duration::from_secs(2));
        say("c8b", "teardown idle_2s_after=true");
    });
    say("c8b", format!("end variant={variant}"));
}

/// x12 of verifier-windows-functional-1: the product's screen source started
/// and stopped fifteen times in one process.
#[test]
#[ignore = "spike S4: needs an interactive Windows desktop"]
fn s4_x12() {
    install_fault_line();
    let variant = variant();
    say("x12", format!("begin variant={variant}"));
    if variant == "mta-ref" {
        take_mta_reference("x12");
    }
    with_deadline("x12", Duration::from_secs(180), || {
        let last_app = Arc::new(Mutex::new(None));
        let mut frames_total = 0;
        let mut started = 0;
        for _cycle in 1..=15 {
            let (sink, writer) = channel(256);
            let mut screen = ScreenSource::new(Arc::clone(&last_app));
            let start = screen.start(sink.clone());
            if start.is_ok() {
                started += 1;
            }
            frames_total += count_frames(&drain_for(&writer, Duration::from_millis(700)));
            screen.stop();
        }
        std::thread::sleep(Duration::from_secs(3));
        say(
            "x12",
            format!("cycles=15 started={started} frames_total={frames_total} idle_3s_after=true"),
        );
    });
    say("x12", format!("end variant={variant}"));
}

struct DirtyFlags {
    counts: Arc<Mutex<DirtyCounts>>,
}

#[derive(Default, Clone)]
struct DirtyCounts {
    frames: u32,
    frames_with_region: u32,
    regions: u64,
    region_area: u64,
    region_errors: u32,
}

struct DirtyCounter {
    counts: Arc<Mutex<DirtyCounts>>,
}

impl GraphicsCaptureApiHandler for DirtyCounter {
    type Flags = DirtyFlags;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            counts: ctx.flags.counts,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let regions = frame.dirty_regions();
        if let Ok(mut c) = self.counts.lock() {
            c.frames += 1;
            match regions {
                Ok(list) => {
                    if !list.is_empty() {
                        c.frames_with_region += 1;
                    }
                    c.regions += list.len() as u64;
                    for r in list {
                        c.region_area += (r.width.max(0) as u64) * (r.height.max(0) as u64);
                    }
                }
                Err(_) => c.region_errors += 1,
            }
        }
        Ok(())
    }
}

fn dirty_line(phase: &str, c: &DirtyCounts, secs: u64) {
    say(
        "dirty",
        format!(
            "phase={phase} secs={secs} frames={} frames_with_region={} regions={} region_area_px={} region_errors={}",
            c.frames, c.frames_with_region, c.regions, c.region_area, c.region_errors
        ),
    );
}

/// The changed-region report on a static screen: the product's settings
/// (500 ms interval, no cursor, default border) with
/// `DirtyRegionSettings::ReportOnly`. Phase `static`: a topmost window covers
/// the screen for 8 s. Phase `moving`: a small window moves every 250 ms on
/// top of it for 8 s, the positive control.
#[test]
#[ignore = "spike S4: needs an interactive Windows desktop"]
fn s4_dirty() {
    install_fault_line();
    with_deadline("dirty", Duration::from_secs(90), || {
        let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        let cover = TestWindow::create("S4StaticCover", MAGENTA, 0, 0, cx, cy).expect("cover");
        pump_thread(Duration::from_secs(1));
        let counts = Arc::new(Mutex::new(DirtyCounts::default()));
        let monitor = Monitor::primary().expect("primary monitor");
        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Custom(Duration::from_millis(500)),
            DirtyRegionSettings::ReportOnly,
            ColorFormat::Bgra8,
            DirtyFlags {
                counts: Arc::clone(&counts),
            },
        );
        let control = DirtyCounter::start_free_threaded(settings);
        say(
            "dirty",
            format!("start={} screen={cx}x{cy}", show(&control)),
        );
        let Ok(control) = control else {
            return;
        };
        // Let the first frames of the start pass, then count a static screen.
        pump_thread(Duration::from_secs(2));
        let warmup = counts.lock().map(|c| c.clone()).unwrap_or_default();
        dirty_line("warmup", &warmup, 2);
        let base = warmup;
        pump_thread(Duration::from_secs(8));
        let after_static = counts.lock().map(|c| c.clone()).unwrap_or_default();
        let diff = |a: &DirtyCounts, b: &DirtyCounts| DirtyCounts {
            frames: a.frames - b.frames,
            frames_with_region: a.frames_with_region - b.frames_with_region,
            regions: a.regions - b.regions,
            region_area: a.region_area - b.region_area,
            region_errors: a.region_errors - b.region_errors,
        };
        dirty_line("static", &diff(&after_static, &base), 8);
        let cover_on_top = unsafe { GetForegroundWindow() } == cover.hwnd;
        say("dirty", format!("cover_was_foreground={cover_on_top}"));
        let mover = TestWindow::create("S4Mover", GREEN, 200, 200, 120, 120).expect("mover");
        let began = Instant::now();
        let mut step = 0;
        while began.elapsed() < Duration::from_secs(8) {
            step += 1;
            let x = 200 + (step % 2) * 300;
            unsafe {
                let _ = SetWindowPos(
                    mover.hwnd,
                    None,
                    x,
                    200,
                    0,
                    0,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSIZE,
                );
            }
            pump_thread(Duration::from_millis(250));
        }
        let after_moving = counts.lock().map(|c| c.clone()).unwrap_or_default();
        dirty_line("moving", &diff(&after_moving, &after_static), 8);
        let ended = end_capture(control, true);
        say("dirty", format!("end={ended} mover_steps={step}"));
        drop(mover);
        drop(cover);
    });
    say("dirty", "end");
}
