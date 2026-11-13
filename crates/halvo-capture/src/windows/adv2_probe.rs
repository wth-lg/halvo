//! Probes of interrogate-win2-adversary-2 (scratch only, nothing here ships).
//! A probe does nothing unless HALVO_ADV2_PROBE names it, so the crate's own
//! test run passes each of them by; the scratch workflow runs each one in a
//! process of its own.

use std::sync::{mpsc, Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PeekMessageW, RegisterClassW,
    EVENT_SYSTEM_FOREGROUND, MSG, PM_REMOVE, WINDOW_EX_STYLE, WINDOW_STYLE, WINEVENT_INCONTEXT,
    WM_QUIT, WNDCLASSW,
};

use super::foreground::{set_win_event_hook, ForegroundAppSource, HOOK_FLAGS};
use super::input::{keystroke_packet, KeyStroke, RawInputSource};
use super::screen::ScreenSource;
use super::window_title::WindowTitleSource;
use super::{
    ensure_message_queue, now_tick, pump_messages, spawn_pump, Lifecycle, Packet, Producer,
    PumpThreadId, INPUT_STOP_DEADLINE,
};
use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureSource, SourceContext};

#[repr(C)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    thread_id: u32,
    owner_process_id: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut core::ffi::c_void;
    fn GetCurrentProcessId() -> u32;
    fn GetProcessHandleCount(process: *mut core::ffi::c_void, count: *mut u32) -> i32;
    fn GetProcessTimes(
        process: *mut core::ffi::c_void,
        creation: *mut u64,
        exit: *mut u64,
        kernel: *mut u64,
        user: *mut u64,
    ) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut core::ffi::c_void;
    fn Thread32First(snapshot: *mut core::ffi::c_void, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: *mut core::ffi::c_void, entry: *mut ThreadEntry) -> i32;
    fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    fn SetLastError(code: u32);
    fn GetLastError() -> u32;
}

#[link(name = "user32")]
extern "system" {
    fn GetGuiResources(process: *mut core::ffi::c_void, flags: u32) -> u32;
}

fn enabled(name: &'static str, limit: Duration) -> bool {
    if std::env::var("HALVO_ADV2_PROBE").map_or(true, |named| named != name) {
        return false;
    }
    std::thread::spawn(move || {
        std::thread::sleep(limit);
        println!("ADV2 {name} time_limit_reached_s={}", limit.as_secs());
        std::process::exit(3);
    });
    true
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    wake: Condvar,
}

impl Held {
    fn line(self) -> String {
        format!(
            "handles={},user_objects={},threads={}",
            self.handles, self.user_objects, self.threads
        )
    }
}

impl Gate {
    fn wait(&self) {
        let mut open = lock(&self.open);
        while !*open {
            open = self.wake.wait(open).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn open(&self) {
        *lock(&self.open) = true;
        self.wake.notify_all();
    }
}

enum Said {
    Id(u32),
    Quit { before: bool, after: bool },
}

fn saw_quit(window: Duration) -> bool {
    let began = Instant::now();
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills.
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                return true;
            }
        }
        if began.elapsed() >= window {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn bystander(target: u32, gate: &Gate, said: &mpsc::Sender<Said>) {
    let id = PumpThreadId::current().get();
    if id != target {
        let _ = said.send(Said::Id(id));
        gate.wait();
        return;
    }
    ensure_message_queue();
    let before = saw_quit(Duration::from_millis(200));
    let _ = said.send(Said::Id(id));
    let after = saw_quit(Duration::from_secs(3));
    let _ = said.send(Said::Quit { before, after });
    gate.wait();
}

struct Hunt {
    made: u32,
    found_at: Option<u32>,
    quit: Option<(bool, bool)>,
}

fn hunt(target: u32, waves: u32, per_wave: u32, on_match: impl FnOnce()) -> Hunt {
    let mut on_match = Some(on_match);
    let mut made = 0;
    let mut found_at = None;
    let mut quit = None;
    for _ in 0..waves {
        let gate = Arc::new(Gate::default());
        let (said, heard) = mpsc::channel();
        let mut joins = Vec::new();
        let mut in_wave = 0;
        while in_wave < per_wave && found_at.is_none() {
            let (gate, said) = (Arc::clone(&gate), said.clone());
            let spawned = std::thread::Builder::new()
                .stack_size(64 * 1024)
                .spawn(move || bystander(target, &gate, &said));
            let Ok(join) = spawned else {
                break;
            };
            joins.push(join);
            in_wave += 1;
            made += 1;
            match heard.recv_timeout(Duration::from_secs(5)) {
                Ok(Said::Id(id)) if id == target => found_at = Some(made),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        if found_at.is_some() {
            if let Some(on_match) = on_match.take() {
                on_match();
            }
            if let Ok(Said::Quit { before, after }) = heard.recv_timeout(Duration::from_secs(8)) {
                quit = Some((before, after));
            }
        }
        gate.open();
        for join in joins {
            let _ = join.join();
        }
        if found_at.is_some() {
            break;
        }
    }
    Hunt {
        made,
        found_at,
        quit,
    }
}

fn idle_pump() -> Result<super::PumpHandle, crate::source::CaptureError> {
    spawn_pump("halvo-adv2-pump".to_string(), |ready, signal| {
        ensure_message_queue();
        ready.report(Ok(()));
        pump_messages(&signal);
    })
}

#[test]
fn quit_post_after_join() {
    if !enabled("quit_post_after_join", Duration::from_secs(280)) {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let ended = std::thread::spawn(move || {
        let _ = tx.send(PumpThreadId::current().get());
    });
    let held_id = rx.recv().expect("the id of the control thread");
    while !ended.is_finished() {
        std::thread::sleep(Duration::from_millis(1));
    }
    let held = hunt(held_id, 10, 2_000, || {});
    println!(
        "ADV2 quit_post control=join_handle_held id={held_id} threads_made={} id_given_again_at={:?}",
        held.made, held.found_at
    );
    drop(ended);
    let closed = hunt(held_id, 25, 2_000, || {});
    println!(
        "ADV2 quit_post control=join_handle_closed id={held_id} threads_made={} id_given_again_at={:?}",
        closed.made, closed.found_at
    );

    let mut hit = false;
    let mut rounds = 0;
    for round in 1..=3 {
        rounds = round;
        let mut handle = match idle_pump() {
            Ok(handle) => handle,
            Err(error) => {
                println!("ADV2 quit_post round={round} spawn_pump={error:?}");
                break;
            }
        };
        let target = handle.id.get();
        let stopped = handle.stop(INPUT_STOP_DEADLINE);
        let mut slot = Some(handle);
        let found = hunt(target, 25, 2_000, || drop(slot.take()));
        let dropped_on_a_match = slot.is_none();
        drop(slot);
        println!(
            "ADV2 quit_post round={round} pump_id={target} stop={stopped:?} threads_made={} id_given_again_at={:?} stopped_handle_dropped_while_another_thread_had_the_id={dropped_on_a_match} wm_quit_seen_before_the_drop_and_after={:?}",
            found.made, found.found_at, found.quit
        );
        if found.quit.is_some_and(|(_, after)| after) {
            hit = true;
            break;
        }
    }
    println!("ADV2 quit_post summary rounds={rounds} another_thread_got_wm_quit={hit}");
}

unsafe extern "system" fn probe_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn make_and_destroy_a_window() -> Result<(), String> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    static CLASS: Mutex<bool> = Mutex::new(false);
    let name = windows::core::w!("HalvoAdv2Probe");
    // SAFETY: GetModuleHandleW with no name reads the handle of this
    // process's executable and takes no pointer.
    let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None) }
        .map_err(|e| format!("GetModuleHandleW {e:?}"))?
        .into();
    let mut registered = lock(&CLASS);
    if !*registered {
        let class = WNDCLASSW {
            lpfnWndProc: Some(probe_wnd_proc),
            hInstance: hinstance,
            lpszClassName: name,
            ..Default::default()
        };
        // SAFETY: `class` names a static wide string and a window procedure
        // with the WNDPROC signature.
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err("RegisterClassW".to_string());
        }
        *registered = true;
    }
    drop(registered);
    // SAFETY: the class name is a static wide string of a registered class;
    // no other pointer is passed.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            name,
            name,
            WINDOW_STYLE(0),
            0,
            0,
            64,
            64,
            None,
            None,
            Some(hinstance),
            None,
        )
    }
    .map_err(|e| format!("CreateWindowExW {e:?}"))?;
    let _ = saw_quit(Duration::from_millis(100));
    // SAFETY: `hwnd` is the window that this thread created above.
    unsafe { DestroyWindow(hwnd) }.map_err(|e| format!("DestroyWindow {e:?}"))?;
    let _ = saw_quit(Duration::from_millis(300));
    Ok(())
}

#[test]
fn screen_sessions_begun_on_sta_threads() {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    if !enabled(
        "screen_sessions_begun_on_sta_threads",
        Duration::from_secs(240),
    ) {
        return;
    }
    for (session, stop_on_the_sta_thread) in [(1_u32, true), (2, false), (3, true)] {
        let (sink, writer) = crate::sink::channel(256);
        let left = std::thread::spawn(move || {
            // SAFETY: CoInitializeEx with no reserved pointer makes this
            // thread's apartment; CoUninitialize below pairs it when it
            // succeeded.
            let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            let mut screen = ScreenSource::new(Arc::default());
            let begun = screen.begin(SourceContext::new(sink));
            std::thread::sleep(Duration::from_millis(700));
            let stopped = stop_on_the_sta_thread.then(|| screen.shutdown());
            println!(
                "ADV2 sta session={session} coinit={:#x} begin={begun:?} shutdown_on_the_sta_thread={stopped:?}",
                init.0
            );
            let keep = if stop_on_the_sta_thread {
                drop(screen);
                None
            } else {
                Some(screen)
            };
            if init.is_ok() {
                // SAFETY: pairs the CoInitializeEx above, on the same thread.
                unsafe { CoUninitialize() };
            }
            keep
        })
        .join();
        match left {
            Ok(Some(mut screen)) => {
                let stopped = screen.shutdown();
                println!(
                    "ADV2 sta session={session} shutdown_after_its_sta_thread_ended={stopped:?}"
                );
            }
            Ok(None) => {}
            Err(_) => println!("ADV2 sta session={session} sta_thread_panicked=true"),
        }
        let mut frames = 0;
        while writer.try_recv().is_ok() {
            frames += 1;
        }
        println!("ADV2 sta session={session} frames={frames}");
        let window = make_and_destroy_a_window();
        println!("ADV2 sta session={session} window_made_and_destroyed={window:?}");
    }
    std::thread::sleep(Duration::from_secs(2));
    println!("ADV2 sta done=true");
}

#[derive(Clone, Copy)]
struct Held {
    handles: u32,
    user_objects: u32,
    threads: u32,
}

fn held() -> Held {
    const TH32CS_SNAPTHREAD: u32 = 4;
    let mut handles = 0;
    // SAFETY: the pseudo handle of this process needs no close, and
    // `handles` is a live u32 that the call writes.
    unsafe {
        GetProcessHandleCount(GetCurrentProcess(), &mut handles);
    }
    // SAFETY: the pseudo handle of this process and a plain flag; 1 asks for
    // the count of USER objects.
    let user_objects = unsafe { GetGuiResources(GetCurrentProcess(), 1) };
    let mut threads = 0;
    // SAFETY: GetCurrentProcessId takes no argument.
    let own = unsafe { GetCurrentProcessId() };
    // SAFETY: a snapshot of the system's threads takes two plain values.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot as isize != -1 {
        let mut entry = ThreadEntry {
            size: core::mem::size_of::<ThreadEntry>() as u32,
            usage: 0,
            thread_id: 0,
            owner_process_id: 0,
            base_priority: 0,
            delta_priority: 0,
            flags: 0,
        };
        // SAFETY: `snapshot` is the live snapshot from above and `entry` is a
        // live THREADENTRY32 whose size field is set.
        let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while more {
            if entry.owner_process_id == own {
                threads += 1;
            }
            // SAFETY: as for Thread32First.
            more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        // SAFETY: `snapshot` is open and closed once, here.
        unsafe {
            CloseHandle(snapshot);
        }
    }
    Held {
        handles,
        user_objects,
        threads,
    }
}

fn cpu_ms() -> u64 {
    let (mut creation, mut exit, mut kernel, mut user) = (0_u64, 0_u64, 0_u64, 0_u64);
    // SAFETY: the pseudo handle of this process and four live 8-byte values
    // that the call writes as FILETIMEs.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        );
    }
    (kernel + user) / 10_000
}

fn no_hook(
    _event: u32,
    _callback: windows::Win32::UI::Accessibility::WINEVENTPROC,
) -> HWINEVENTHOOK {
    HWINEVENTHOOK::default()
}

struct Cycles {
    odd: u32,
    slowest_stop: Duration,
}

fn one_cycle<S: CaptureSource>(mut source: S, sink: &SinkHandle, starts: bool, seen: &mut Cycles) {
    let begun = source.begin(SourceContext::new(sink.clone()));
    let began = Instant::now();
    let stopped = source.shutdown();
    seen.slowest_stop = seen.slowest_stop.max(began.elapsed());
    if begun.is_ok() != starts || stopped.is_err() {
        seen.odd += 1;
        if seen.odd <= 5 {
            println!("ADV2 cycles odd begin={begun:?} shutdown={stopped:?}");
        }
    }
}

fn cycles(sink: &SinkHandle, rounds: u32, seen: &mut Cycles) {
    for _ in 0..rounds {
        one_cycle(RawInputSource::keyboard(Arc::default()), sink, true, seen);
        one_cycle(RawInputSource::pointer(Arc::default()), sink, true, seen);
        one_cycle(ForegroundAppSource::new(), sink, true, seen);
        one_cycle(WindowTitleSource::new(), sink, true, seen);
        one_cycle(
            ForegroundAppSource::with_installer(no_hook),
            sink,
            false,
            seen,
        );
        one_cycle(
            WindowTitleSource::with_installer(no_hook),
            sink,
            false,
            seen,
        );
    }
}

#[test]
fn start_stop_cycles_and_idle_pumps() {
    if !enabled("start_stop_cycles_and_idle_pumps", Duration::from_secs(240)) {
        return;
    }
    let (sink, writer) = crate::sink::channel(4_096);
    let reader = std::thread::spawn(move || {
        let mut events = 0_u64;
        while writer.recv().is_some() {
            events += 1;
        }
        events
    });
    let mut seen = Cycles {
        odd: 0,
        slowest_stop: Duration::ZERO,
    };
    cycles(&sink, 5, &mut seen);
    std::thread::sleep(Duration::from_millis(500));
    let before = held();
    cycles(&sink, 100, &mut seen);
    std::thread::sleep(Duration::from_millis(500));
    let after = held();
    println!(
        "ADV2 cycles rounds=100 starts=400 failed_starts=200 odd={} slowest_stop_ms={} before={} after={}",
        seen.odd,
        seen.slowest_stop.as_millis(),
        before.line(),
        after.line()
    );

    let mut keyboard = RawInputSource::keyboard(Arc::default());
    let mut pointer = RawInputSource::pointer(Arc::default());
    let mut foreground = ForegroundAppSource::new();
    let mut title = WindowTitleSource::new();
    let begun = [
        keyboard.begin(SourceContext::new(sink.clone())).is_ok(),
        pointer.begin(SourceContext::new(sink.clone())).is_ok(),
        foreground.begin(SourceContext::new(sink.clone())).is_ok(),
        title.begin(SourceContext::new(sink.clone())).is_ok(),
    ];
    let cpu_before = cpu_ms();
    std::thread::sleep(Duration::from_secs(4));
    let cpu_after = cpu_ms();
    let stopped = [
        keyboard.shutdown().is_ok(),
        pointer.shutdown().is_ok(),
        foreground.shutdown().is_ok(),
        title.shutdown().is_ok(),
    ];
    drop((keyboard, pointer, foreground, title, sink));
    let events = reader.join().unwrap_or(0);
    println!(
        "ADV2 idle four_sources_begun={begun:?} wall_ms=4000 process_cpu_ms={} stopped={stopped:?} events_in_the_whole_probe={events}",
        cpu_after - cpu_before
    );
}

unsafe extern "system" fn noop_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
}

#[test]
fn hook_failure_codes() {
    if !enabled("hook_failure_codes", Duration::from_secs(60)) {
        return;
    }
    let cases = [
        (
            "event_range_reversed",
            EVENT_SYSTEM_FOREGROUND + 1,
            EVENT_SYSTEM_FOREGROUND,
            HOOK_FLAGS,
        ),
        (
            "in_context_without_a_module",
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            WINEVENT_INCONTEXT,
        ),
    ];
    let ran = std::thread::spawn(move || {
        for (case, min, max, flags) in cases {
            // SAFETY: SetLastError takes a plain value.
            unsafe { SetLastError(0) };
            // SAFETY: SetWinEventHook takes plain values, no module and a
            // callback with the WINEVENTPROC signature.
            let hook = unsafe { SetWinEventHook(min, max, None, Some(noop_event_proc), 0, 0, flags) };
            // SAFETY: GetLastError takes no argument.
            let code = unsafe { GetLastError() };
            println!(
                "ADV2 hook_codes case={case} hook_is_invalid={} last_error={code} reported={:?}",
                hook.is_invalid(),
                super::last_os_error("SetWinEventHook")
            );
            if !hook.is_invalid() {
                // SAFETY: `hook` is the valid handle that this thread installed.
                unsafe {
                    let _ = UnhookWinEvent(hook);
                }
            }
        }
        // SAFETY: SetLastError takes a plain value.
        unsafe { SetLastError(0) };
        let hook = set_win_event_hook(EVENT_SYSTEM_FOREGROUND, None);
        // SAFETY: GetLastError takes no argument.
        let code = unsafe { GetLastError() };
        println!(
            "ADV2 hook_codes case=no_callback_through_the_unit_installer hook_is_invalid={} last_error={code} reported={:?}",
            hook.is_invalid(),
            super::last_os_error("SetWinEventHook")
        );
        if !hook.is_invalid() {
            // SAFETY: `hook` is the valid handle that this thread installed.
            unsafe {
                let _ = UnhookWinEvent(hook);
            }
        }
        // SAFETY: SetLastError takes a plain value.
        unsafe { SetLastError(0) };
        println!(
            "ADV2 hook_codes case=last_error_is_zero reported={:?}",
            super::last_os_error("SetWinEventHook")
        );
    })
    .join();
    println!("ADV2 hook_codes done={}", ran.is_ok());
}

fn labelled(label: &str, n: u32) -> Packet {
    Packet {
        kind: Modality::Keystroke,
        app: format!("{label}-{n}"),
        byte_len: 2,
        tick: now_tick(),
    }
}

#[test]
fn packets_of_a_stopped_run_after_a_restart() {
    if !enabled(
        "packets_of_a_stopped_run_after_a_restart",
        Duration::from_secs(60),
    ) {
        return;
    }
    let (sink, writer) = crate::sink::channel(1);
    let mut life = Lifecycle::default();
    let slot: Arc<Mutex<Option<Producer>>> = Arc::default();
    let begin = |life: &mut Lifecycle| {
        let slot = Arc::clone(&slot);
        life.begin(
            &SourceContext::new(sink.clone()),
            "halvo-adv2-forward",
            move |producer| {
                *lock(&slot) = Some(producer);
                idle_pump()
            },
        )
    };
    let first = begin(&mut life);
    let old = lock(&slot).take().expect("the producer of the first run");
    for n in 0..6 {
        old.push(labelled("old-run", n));
    }
    std::thread::sleep(Duration::from_millis(300));
    let read_before_the_stop = writer
        .recv_timeout(Duration::from_millis(700))
        .map(|received| received.event().foreground_app.clone());
    let stopped = life.shutdown();
    let counts_at_the_stop = life.handoff_counts();
    for n in 6..9 {
        old.push(labelled("old-run-pushed-after-the-stop", n));
    }
    let lost_by_pushes_after_the_stop = life.handoff_counts().lost - counts_at_the_stop.lost;
    let barrier = writer.barrier();
    let second = begin(&mut life);
    let new = lock(&slot).take().expect("the producer of the second run");
    for n in 0..2 {
        new.push(labelled("new-run", n));
    }
    std::thread::sleep(Duration::from_millis(300));
    let mut order = Vec::new();
    while let Ok(received) = writer.recv_timeout(Duration::from_millis(700)) {
        order.push(format!(
            "{}:number={}:before_the_barrier={}",
            received.event().foreground_app,
            received.number().get(),
            received.is_before(barrier)
        ));
    }
    println!(
        "ADV2 old_run begin_1={first:?} read_before_the_stop={read_before_the_stop:?} shutdown={stopped:?} lost_at_the_stop={} lost_by_3_pushes_after_the_stop={lost_by_pushes_after_the_stop} barrier_drawn_after_the_stop={barrier:?} begin_2={second:?}",
        counts_at_the_stop.lost
    );
    println!("ADV2 old_run received_in_order={}", order.join(" "));
    println!(
        "ADV2 old_run lost_at_the_end={} second_shutdown={:?}",
        life.handoff_counts().lost,
        life.shutdown()
    );
}

#[test]
fn a_record_that_names_no_key() {
    if !enabled("a_record_that_names_no_key", Duration::from_secs(30)) {
        return;
    }
    let overrun = KeyStroke {
        vkey: 0xFF,
        released: false,
    };
    let packet = keystroke_packet(overrun, String::new(), 0);
    println!(
        "ADV2 no_key vkey=0xFF released=false makes_a_keystroke_packet={}",
        packet.is_some_and(|p| p.kind == Modality::Keystroke)
    );
}

fn keyboard_is_registered() -> Option<bool> {
    use windows::Win32::Devices::HumanInterfaceDevice::{
        HID_USAGE_GENERIC_KEYBOARD, HID_USAGE_PAGE_GENERIC,
    };
    use windows::Win32::UI::Input::{GetRegisteredRawInputDevices, RAWINPUTDEVICE};
    let mut devices = [RAWINPUTDEVICE::default(); 8];
    let mut count = devices.len() as u32;
    // SAFETY: `devices` holds `count` RAWINPUTDEVICE values that the call may
    // fill, and the size argument is the size of one of them.
    let got = unsafe {
        GetRegisteredRawInputDevices(
            Some(devices.as_mut_ptr()),
            &mut count,
            core::mem::size_of::<RAWINPUTDEVICE>() as u32,
        )
    };
    if got == u32::MAX {
        return None;
    }
    Some(devices.iter().take(got as usize).any(|device| {
        device.usUsagePage == HID_USAGE_PAGE_GENERIC
            && device.usUsage == HID_USAGE_GENERIC_KEYBOARD
            && !device.hwndTarget.0.is_null()
    }))
}

#[derive(Default)]
struct Raced {
    begun: u32,
    refused: u32,
    other_begin_errors: u32,
    running_without_a_registration: u32,
    registration_unreadable: u32,
    stop_errors: u32,
}

fn race_keyboard(sink: &SinkHandle, until: Instant) -> Raced {
    let mut seen = Raced::default();
    while Instant::now() < until {
        let mut source = RawInputSource::keyboard(Arc::default());
        match source.begin(SourceContext::new(sink.clone())) {
            Ok(_) => {
                seen.begun += 1;
                match keyboard_is_registered() {
                    Some(true) => {}
                    Some(false) => seen.running_without_a_registration += 1,
                    None => seen.registration_unreadable += 1,
                }
                if source.shutdown().is_err() {
                    seen.stop_errors += 1;
                }
            }
            Err(crate::source::CaptureError::AlreadyStarted) => seen.refused += 1,
            Err(error) => {
                seen.other_begin_errors += 1;
                if seen.other_begin_errors <= 3 {
                    println!("ADV2 race begin={error:?}");
                }
            }
        }
    }
    seen
}

#[test]
fn two_keyboard_sources_race_for_the_device() {
    if !enabled(
        "two_keyboard_sources_race_for_the_device",
        Duration::from_secs(60),
    ) {
        return;
    }
    let (sink, writer) = crate::sink::channel(4_096);
    let reader = std::thread::spawn(move || while writer.recv().is_some() {});
    let until = Instant::now() + Duration::from_secs(4);
    let other = {
        let sink = sink.clone();
        std::thread::spawn(move || race_keyboard(&sink, until))
    };
    let mine = race_keyboard(&sink, until);
    let theirs = other.join().unwrap_or_default();
    for (thread, seen) in [("a", mine), ("b", theirs)] {
        println!(
            "ADV2 race thread={thread} begun={} refused={} other_begin_errors={} running_without_a_registration={} registration_unreadable={} stop_errors={}",
            seen.begun,
            seen.refused,
            seen.other_begin_errors,
            seen.running_without_a_registration,
            seen.registration_unreadable,
            seen.stop_errors
        );
    }
    println!(
        "ADV2 race keyboard_registered_after_every_stop={:?}",
        keyboard_is_registered()
    );
    drop(sink);
    let _ = reader.join();
}
