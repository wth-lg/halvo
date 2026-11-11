// Throwaway probes of spikes S5 to S9 for run 20261108-halvo-r1-capture (lane spike-win-b).
// Scratch branches only; never merged. Output: counts, sizes, times and booleans.
mod procs;
mod s5;
mod s8;
mod util;

use std::time::Duration;

use util::*;
use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::*;

const S6_EVENT: &str = "Local\\R1SpikeS6Stop";

/// S6 stand-in of halvo-capture.exe: hold until `ctl stop` sets the named event.
fn s6_hold(args: &[String]) {
    let dir = arg(args, "--dir").unwrap_or_else(|| ".".into());
    let ignore = flag(args, "--ignore-stop");
    let marker = format!("{dir}\\holder.txt");
    let ev = if ignore { None } else { unsafe { CreateEventW(None, true, false, PCWSTR(wide(S6_EVENT).as_ptr())).ok() } };
    append_line(&marker, &format!("holder_event=started ignore_stop={ignore} t_us={}", now_us()));
    match ev {
        Some(e) => {
            let r = unsafe { WaitForSingleObject(e, 600_000) };
            append_line(&marker, &format!("holder_event=stopped_by_ctl signalled={} t_us={}", r == WAIT_OBJECT_0, now_us()));
        }
        None => std::thread::sleep(Duration::from_secs(600)),
    }
}

/// `ctl stop`: exit 69 when no holder listens, as `ctl` does when it cannot reach a daemon.
fn ctl(args: &[String]) -> i32 {
    if args.get(2).map(String::as_str) != Some("stop") {
        return 64;
    }
    match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(wide(S6_EVENT).as_ptr())) } {
        Ok(e) => {
            let ok = unsafe { SetEvent(e).is_ok() };
            unsafe {
                let _ = CloseHandle(e);
            }
            if ok {
                std::thread::sleep(Duration::from_millis(500));
                0
            } else {
                1
            }
        }
        Err(_) => 69,
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    // `--out FILE` sends standard output to FILE (a process started under another account).
    if let Some(i) = args.iter().position(|a| a == "--out") {
        if let Some(path) = args.get(i + 1).cloned() {
            use windows::Win32::Storage::FileSystem::*;
            use windows::Win32::System::Console::{SetStdHandle, STD_OUTPUT_HANDLE};
            if let Ok(h) = unsafe { CreateFileW(PCWSTR(wide(&path).as_ptr()), FILE_GENERIC_WRITE.0, FILE_SHARE_READ, None, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, None) } {
                unsafe {
                    let _ = SetStdHandle(STD_OUTPUT_HANDLE, h);
                }
            }
            args.drain(i..i + 2);
        }
    }
    let code = match args.get(1).map(String::as_str) {
        Some("child") => {
            s5::child(&args);
            0
        }
        Some("sleep") => {
            s5::sleep(&args);
            0
        }
        Some("lifetime") => {
            s5::lifetime(&args);
            0
        }
        Some("runvalue") => {
            s5::runvalue(&args);
            0
        }
        Some("runvalue-set") => {
            s5::runvalue_set(&args);
            0
        }
        Some("startup-state") => {
            s5::startup_state(&args);
            0
        }
        Some("startup-toggle") => {
            s8::startup_toggle(&args);
            0
        }
        Some("session-end-settings") => {
            s5::session_end_settings();
            0
        }
        Some("pipe-server") => {
            s5::pipe_server(&args);
            0
        }
        Some("pipe-client") => {
            s5::pipe_client(&args);
            0
        }
        Some("whoami") => {
            let t = self_token();
            println!(
                "PROBE whoami elevated={:?} integrity_rid=0x{:x}",
                t.as_ref().and_then(|t| token_elevated(t.0)),
                t.as_ref().and_then(|t| token_integrity_rid(t.0)).unwrap_or(0)
            );
            0
        }
        Some("s6-hold") => {
            s6_hold(&args);
            0
        }
        Some("ctl") => ctl(&args),
        Some("app-run") => {
            procs::app_run(&args);
            0
        }
        Some("peak") => {
            procs::peak(&args);
            0
        }
        Some("site") => {
            s8::site(&args);
            0
        }
        Some("--version") => {
            println!("0.0.0");
            0
        }
        _ => {
            eprintln!("usage: r1probe <child|sleep|lifetime|runvalue|runvalue-set|startup-state|startup-toggle|session-end-settings|pipe-server|pipe-client|whoami|s6-hold|ctl|app-run|peak|site>");
            64
        }
    };
    std::process::exit(code);
}
