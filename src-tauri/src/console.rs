//! The launch form `--version` in a console.

/// Prints the version on standard output. Where the app has no standard output, as for the
/// windows-subsystem build started from a console, it first attaches to the console of its
/// parent, so that the line reaches that console; where standard output is given, it attaches to
/// nothing.
pub fn print_version(version: &str) {
    attach_where_no_output();
    println!("{version}");
}

#[cfg(windows)]
fn attach_where_no_output() {
    use std::os::windows::io::AsRawHandle;
    if !std::io::stdout().as_raw_handle().is_null() {
        return;
    }
    windows_core::link!("kernel32.dll" "system" fn AttachConsole(process: u32) -> i32);
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: AttachConsole takes a process number by value and reads no memory of the caller.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_where_no_output() {}

#[cfg(all(test, windows))]
mod tests {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};

    type Handle = *mut c_void;

    const MODE: &str = "HALVO_CONSOLE_TEST_MODE";
    const TEST: &str = "console::tests::the_version_reaches_the_pseudoconsole_of_the_parent_from_a_child_with_no_standard_handles";
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
    const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
    const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x0002_0016;
    const WAIT_OBJECT_0: u32 = 0;
    const CHILD_WAIT_MS: u32 = 60_000;
    const PARENT_WAIT_MS: u32 = 120_000;

    #[repr(C)]
    pub struct Coord {
        x: i16,
        y: i16,
    }

    #[repr(C)]
    pub struct StartupInfo {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        columns: u32,
        rows: u32,
        fill: u32,
        flags: u32,
        show: u16,
        reserved2_size: u16,
        reserved2: *mut u8,
        input: Handle,
        output: Handle,
        error: Handle,
    }

    #[repr(C)]
    pub struct StartupInfoEx {
        info: StartupInfo,
        attributes: *mut c_void,
    }

    #[repr(C)]
    pub struct ProcessInformation {
        process: Handle,
        thread: Handle,
        process_id: u32,
        thread_id: u32,
    }

    windows_core::link!("kernel32.dll" "system" fn CreatePipe(read: *mut Handle, write: *mut Handle, attributes: *const c_void, size: u32) -> i32);
    windows_core::link!("kernel32.dll" "system" fn CreatePseudoConsole(size: Coord, input: Handle, output: Handle, flags: u32, console: *mut Handle) -> i32);
    windows_core::link!("kernel32.dll" "system" fn ClosePseudoConsole(console: Handle));
    windows_core::link!("kernel32.dll" "system" fn InitializeProcThreadAttributeList(list: *mut c_void, count: u32, flags: u32, size: *mut usize) -> i32);
    windows_core::link!("kernel32.dll" "system" fn UpdateProcThreadAttribute(list: *mut c_void, flags: u32, attribute: usize, value: *const c_void, size: usize, previous: *mut c_void, returned: *mut usize) -> i32);
    windows_core::link!("kernel32.dll" "system" fn DeleteProcThreadAttributeList(list: *mut c_void));
    windows_core::link!("kernel32.dll" "system" fn CreateProcessW(application: *const u16, command_line: *mut u16, process_attributes: *const c_void, thread_attributes: *const c_void, inherit: i32, flags: u32, environment: *const c_void, directory: *const u16, startup: *const StartupInfo, information: *mut ProcessInformation) -> i32);
    windows_core::link!("kernel32.dll" "system" fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32);
    windows_core::link!("kernel32.dll" "system" fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32);
    windows_core::link!("kernel32.dll" "system" fn TerminateProcess(process: Handle, code: u32) -> i32);
    windows_core::link!("kernel32.dll" "system" fn CloseHandle(handle: Handle) -> i32);
    windows_core::link!("kernel32.dll" "system" fn ReadFile(file: Handle, buffer: *mut u8, size: u32, read: *mut u32, overlapped: *mut c_void) -> i32);

    /// A start record of `cb` bytes with `flags`, every handle null and every other field 0.
    fn blank(cb: usize, flags: u32) -> StartupInfo {
        StartupInfo {
            cb: u32::try_from(cb).expect("the record's size fits"),
            reserved: null_mut(),
            desktop: null_mut(),
            title: null_mut(),
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            columns: 0,
            rows: 0,
            fill: 0,
            flags,
            show: 0,
            reserved2_size: 0,
            reserved2: null_mut(),
            input: null_mut(),
            output: null_mut(),
            error: null_mut(),
        }
    }

    /// This process's environment with `MODE` set to `mode`, as the system's block of
    /// UTF-16 lines.
    fn environment(mode: &str) -> Vec<u16> {
        let mut block = Vec::new();
        for (key, value) in std::env::vars_os() {
            if key.eq_ignore_ascii_case(MODE) {
                continue;
            }
            block.extend(key.encode_wide());
            block.push(u16::from(b'='));
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.extend(format!("{MODE}={mode}").encode_utf16());
        block.extend([0, 0]);
        block
    }

    /// Starts this test again in this test binary, with `MODE` set to `mode`, the creation flags
    /// `flags` and the start record `startup`.
    fn start(mode: &str, flags: u32, startup: *const StartupInfo) -> ProcessInformation {
        let exe = std::env::current_exe().expect("the test binary has a path");
        let line = format!(
            "\"{}\" --exact {TEST} --nocapture --test-threads=1",
            exe.display()
        );
        let mut line: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
        let environment = environment(mode);
        let mut information = ProcessInformation {
            process: null_mut(),
            thread: null_mut(),
            process_id: 0,
            thread_id: 0,
        };
        // SAFETY: the command line and the environment are NUL-ended buffers of this function that
        // live through the call, `startup` points at a filled record of its stated size that the
        // caller keeps alive through the call, and the call alone writes `information`.
        let made = unsafe {
            CreateProcessW(
                null(),
                line.as_mut_ptr(),
                null(),
                null(),
                0,
                flags | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                null(),
                startup,
                &mut information,
            )
        };
        assert_ne!(
            made,
            0,
            "the test binary starts again: {}",
            std::io::Error::last_os_error()
        );
        information
    }

    /// The exit code of the process once it ends within `wait_ms`, or none when it had to be
    /// ended; its handles are closed.
    fn ended(information: &ProcessInformation, wait_ms: u32) -> Option<u32> {
        // SAFETY: the two handles are this test's own from CreateProcessW and stay open until the
        // two CloseHandle calls at the end of this block; `code` is a local.
        unsafe {
            let mut code = 0;
            let ended = if WaitForSingleObject(information.process, wait_ms) == WAIT_OBJECT_0
                && GetExitCodeProcess(information.process, &mut code) != 0
            {
                Some(code)
            } else {
                TerminateProcess(information.process, 1);
                WaitForSingleObject(information.process, wait_ms);
                None
            };
            CloseHandle(information.thread);
            CloseHandle(information.process);
            ended
        }
    }

    /// Everything that comes out of the read end `pipe` until it is closed; the end is closed
    /// after.
    fn read_to_end(pipe: usize) -> Vec<u8> {
        let mut all = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let mut read = 0u32;
            // SAFETY: the buffer and `read` are this function's own, and the read end stays open
            // until the CloseHandle after this loop.
            let ok = unsafe {
                ReadFile(
                    pipe as Handle,
                    buffer.as_mut_ptr(),
                    u32::try_from(buffer.len()).expect("the buffer's size fits"),
                    &mut read,
                    null_mut(),
                )
            };
            if ok == 0 || read == 0 {
                break;
            }
            all.extend_from_slice(&buffer[..read as usize]);
        }
        // SAFETY: the read end was handed to this function alone, and nothing reads it after.
        unsafe {
            CloseHandle(pipe as Handle);
        }
        all
    }

    /// The text of the pseudoconsole without its control sequences.
    fn plain(bytes: &[u8]) -> String {
        let text = String::from_utf8_lossy(bytes);
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\u{1b}' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The parent inside the pseudoconsole: it starts the child detached, with no console and
    /// three null standard handles, and waits for its end.
    fn in_the_parent() {
        let startup = blank(size_of::<StartupInfo>(), STARTF_USESTDHANDLES);
        let child = start("child", DETACHED_PROCESS, &startup);
        assert_eq!(
            ended(&child, CHILD_WAIT_MS),
            Some(0),
            "the child ends with 0"
        );
    }

    #[test]
    fn the_version_reaches_the_pseudoconsole_of_the_parent_from_a_child_with_no_standard_handles() {
        match std::env::var(MODE).as_deref() {
            Ok("parent") => return in_the_parent(),
            Ok("child") => return super::print_version(crate::app::VERSION),
            _ => {}
        }
        let (mut input_read, mut input_write) = (null_mut(), null_mut());
        let (mut output_read, mut output_write) = (null_mut(), null_mut());
        let mut console = null_mut();
        // SAFETY: each pointer names a local of this test; the pseudoconsole takes copies of the
        // two pipe ends that it is given, so this test closes its own after the call.
        unsafe {
            assert_ne!(CreatePipe(&mut input_read, &mut input_write, null(), 0), 0);
            assert_ne!(
                CreatePipe(&mut output_read, &mut output_write, null(), 0),
                0
            );
            let size = Coord { x: 120, y: 30 };
            assert_eq!(
                CreatePseudoConsole(size, input_read, output_write, 0, &mut console),
                0
            );
            CloseHandle(input_read);
            CloseHandle(output_write);
        }
        let output = output_read as usize;
        let reader = std::thread::spawn(move || read_to_end(output));
        let mut size = 0usize;
        // SAFETY: with a null list the call writes only the size that a list of one needs.
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        }
        let mut list = vec![0usize; size / size_of::<usize>() + 1];
        let list = list.as_mut_ptr().cast::<c_void>();
        // SAFETY: the list has room for `size` bytes at a pointer's alignment and outlives its
        // use; the pseudoconsole's handle stays open until ClosePseudoConsole below.
        unsafe {
            assert_ne!(InitializeProcThreadAttributeList(list, 1, 0, &mut size), 0);
            assert_ne!(
                UpdateProcThreadAttribute(
                    list,
                    0,
                    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                    console.cast_const(),
                    size_of::<Handle>(),
                    null_mut(),
                    null_mut(),
                ),
                0
            );
        }
        let startup = StartupInfoEx {
            info: blank(size_of::<StartupInfoEx>(), STARTF_USESTDHANDLES),
            attributes: list,
        };
        let parent = start("parent", EXTENDED_STARTUPINFO_PRESENT, &startup.info);
        let parent_ended = ended(&parent, PARENT_WAIT_MS);
        // SAFETY: the pseudoconsole, the list and the input's write end are this test's own, and
        // no process uses them after the parent's end.
        unsafe {
            ClosePseudoConsole(console);
            DeleteProcThreadAttributeList(list);
            CloseHandle(input_write);
        }
        let shown = plain(&reader.join().expect("the reader ends"));
        assert_eq!(parent_ended, Some(0), "the parent ends with 0: {shown}");
        assert!(shown.contains(crate::app::VERSION), "{shown}");
    }
}
