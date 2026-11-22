//! One way to start a child on Windows with every handle that it inherits named
//! (contracts.md C15.9).

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::time::Duration;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    GetHandleInformation, SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
};

use crate::client_windows::os_error;

/// The three standard handles of a child.
#[derive(Debug, Clone, Copy)]
pub struct StdHandles<'a> {
    pub input: BorrowedHandle<'a>,
    pub output: BorrowedHandle<'a>,
    pub error: BorrowedHandle<'a>,
}

/// A child that [`start_with_handles`] started: the handle of its process and its id.
#[derive(Debug)]
pub struct Child {
    process: OwnedHandle,
    id: u32,
}

impl Child {
    /// The process id of the child.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Waits at most `bound` for the child to end: its exit code once it has ended, and `None`
    /// while it runs.
    pub fn wait(&self, bound: Duration) -> io::Result<Option<u32>> {
        let process = HANDLE(self.process.as_raw_handle());
        let millis =
            u32::try_from(bound.as_millis()).map_or(INFINITE - 1, |ms| ms.min(INFINITE - 1));
        // SAFETY: the process handle is open; the call takes no pointer.
        let waited = unsafe { WaitForSingleObject(process, millis) };
        if waited == WAIT_TIMEOUT {
            return Ok(None);
        }
        if waited != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0u32;
        // SAFETY: the process handle is open and `code` is a live local.
        unsafe { GetExitCodeProcess(process, &mut code) }.map_err(os_error)?;
        Ok(Some(code))
    }
}

impl AsHandle for Child {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.process.as_handle()
    }
}

/// A child that [`start_with_lifeline`] started, and the write end of the pipe on its standard
/// input, which this process alone holds while the child lives.
#[derive(Debug)]
pub struct Lifelined {
    child: Child,
    lifeline: Option<std::fs::File>,
}

impl Lifelined {
    /// The process id of the child.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// The child's exit code once it has ended, none while it runs; it does not wait.
    pub fn ended(&self) -> io::Result<Option<u32>> {
        self.child.wait(Duration::ZERO)
    }

    /// Closes the write end of the pipe, so that the child reads the end of its standard input.
    pub fn close_lifeline(&mut self) {
        self.lifeline = None;
    }

    /// Ends the child and waits for its end; a child that has ended already is left as it is.
    pub fn kill(&mut self) -> io::Result<()> {
        if self.ended()?.is_some() {
            return Ok(());
        }
        // SAFETY: the process handle is open and names this process's own child; the call takes
        // no pointer.
        let ended = unsafe { TerminateProcess(HANDLE(self.child.as_handle().as_raw_handle()), 1) };
        if let Err(error) = ended {
            if self.ended()?.is_none() {
                return Err(os_error(error));
            }
        }
        self.child.wait(Duration::MAX).map(|_| ())
    }
}

/// A new anonymous pipe, its read end first; neither end is inheritable.
fn new_pipe() -> io::Result<(std::fs::File, std::fs::File)> {
    let mut reads = HANDLE::default();
    let mut writes = HANDLE::default();
    // SAFETY: the call writes two new handles, not inheritable, into the live locals.
    unsafe { CreatePipe(&mut reads, &mut writes, None, 0) }.map_err(os_error)?;
    // SAFETY: each handle is new, and each File owns its own once.
    unsafe {
        Ok((
            std::fs::File::from_raw_handle(reads.0),
            std::fs::File::from_raw_handle(writes.0),
        ))
    }
}

/// Starts `program` with the arguments `args` through [`start_with_handles`], with no console
/// window: its standard input is the read end of a new pipe, its standard output the null
/// device and its standard error `error`, and it inherits these three handles and no other.
/// The write end of the pipe, which no child inherits, is handed back with the child, and this
/// process keeps no copy of the read end.
pub fn start_with_lifeline(
    program: &Path,
    args: &[&OsStr],
    error: BorrowedHandle<'_>,
) -> io::Result<Lifelined> {
    let (reads, writes) = new_pipe()?;
    let null = std::fs::OpenOptions::new().write(true).open("NUL")?;
    let standard = StdHandles {
        input: reads.as_handle(),
        output: null.as_handle(),
        error,
    };
    let child = start_with_handles(program, args, standard, &[])?;
    Ok(Lifelined {
        child,
        lifeline: Some(writes),
    })
}

/// Starts `program` with the arguments `args`, the standard handles `standard` and no console
/// window.
/// The child inherits the three standard handles and each handle of `inherited`, at the values
/// that they have in this process, and no other handle (PROC_THREAD_ATTRIBUTE_HANDLE_LIST). Each
/// of them is made inheritable for the call alone and gets its own inherit flag back after it, so
/// a start that names no handle list on another thread of this process while the call runs can
/// inherit them too. The child takes this process's environment and current directory.
pub fn start_with_handles(
    program: &Path,
    args: &[&OsStr],
    standard: StdHandles<'_>,
    inherited: &[BorrowedHandle<'_>],
) -> io::Result<Child> {
    let mut listed: Vec<HANDLE> = Vec::new();
    for handle in [standard.input, standard.output, standard.error]
        .iter()
        .chain(inherited)
    {
        let handle = HANDLE(handle.as_raw_handle());
        if !listed.contains(&handle) {
            listed.push(handle);
        }
    }
    let mut flags = Vec::with_capacity(listed.len());
    for &handle in &listed {
        let mut flag = 0u32;
        // SAFETY: the handle is open, borrowed for this call, and `flag` is a live local.
        unsafe { GetHandleInformation(handle, &mut flag) }.map_err(os_error)?;
        flags.push(flag);
    }
    let restore = |count: usize| {
        for (&handle, &flag) in listed.iter().zip(&flags).take(count) {
            // SAFETY: the handle is open, borrowed for this call; the call takes no pointer.
            let _ = unsafe {
                SetHandleInformation(
                    handle,
                    HANDLE_FLAG_INHERIT.0,
                    HANDLE_FLAGS(flag & HANDLE_FLAG_INHERIT.0),
                )
            };
        }
    };
    for (made, &handle) in listed.iter().enumerate() {
        // SAFETY: the handle is open, borrowed for this call; the call takes no pointer.
        let set =
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) };
        if let Err(error) = set {
            restore(made);
            return Err(os_error(error));
        }
    }
    let started = create(program, args, standard, &listed);
    restore(listed.len());
    started
}

fn push_quoted(line: &mut Vec<u16>, arg: &OsStr) {
    let arg: Vec<u16> = arg.encode_wide().collect();
    let plain = !arg.is_empty()
        && !arg.iter().any(|&c| {
            [b' ', b'\t', b'\n', 0x0b, b'"']
                .iter()
                .any(|&b| c == u16::from(b))
        });
    if plain {
        line.extend_from_slice(&arg);
        return;
    }
    let backslash = u16::from(b'\\');
    let quote = u16::from(b'"');
    line.push(quote);
    let mut backslashes = 0usize;
    for &c in &arg {
        if c == backslash {
            backslashes += 1;
        } else {
            if c == quote {
                line.extend(std::iter::repeat_n(backslash, backslashes + 1));
            }
            backslashes = 0;
        }
        line.push(c);
    }
    line.extend(std::iter::repeat_n(backslash, backslashes));
    line.push(quote);
}

fn create(
    program: &Path,
    args: &[&OsStr],
    standard: StdHandles<'_>,
    listed: &[HANDLE],
) -> io::Result<Child> {
    let application: Vec<u16> = program
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut line = Vec::new();
    push_quoted(&mut line, program.as_os_str());
    for arg in args {
        line.push(u16::from(b' '));
        push_quoted(&mut line, arg);
    }
    line.push(0);
    let mut size = 0usize;
    // SAFETY: with no list the call writes only the size that a list of one attribute needs,
    // into the live local `size`.
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut size) };
    let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>()).max(1)];
    let list = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr().cast());
    // SAFETY: `buffer` holds at least `size` bytes, aligned for the list, and lives until the
    // list is deleted below.
    unsafe { InitializeProcThreadAttributeList(Some(list), 1, None, &mut size) }
        .map_err(os_error)?;
    let started = create_listed(&application, &mut line, standard, listed, list);
    // SAFETY: the list was initialised above and is deleted once, before `buffer` goes.
    unsafe { DeleteProcThreadAttributeList(list) };
    started
}

fn create_listed(
    application: &[u16],
    line: &mut [u16],
    standard: StdHandles<'_>,
    listed: &[HANDLE],
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
) -> io::Result<Child> {
    // SAFETY: `list` was initialised for one attribute, and `listed` holds open inheritable
    // handles and outlives the list, which the caller deletes after this call.
    unsafe {
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(listed.as_ptr().cast()),
            std::mem::size_of_val(listed),
            None,
            None,
        )
    }
    .map_err(os_error)?;
    let info = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: HANDLE(standard.input.as_raw_handle()),
            hStdOutput: HANDLE(standard.output.as_raw_handle()),
            hStdError: HANDLE(standard.error.as_raw_handle()),
            ..Default::default()
        },
        lpAttributeList: list,
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: `application` and `line` end in a NUL and live across the call, `line` is writable
    // as the call asks, `info` holds the list that the caller initialised, and `process` is a
    // live local that receives two new handles.
    unsafe {
        CreateProcessW(
            PCWSTR(application.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            true,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &info.StartupInfo,
            &mut process,
        )
    }
    .map_err(os_error)?;
    // SAFETY: the call made the thread handle for this process alone, and it is owned here once.
    drop(unsafe { OwnedHandle::from_raw_handle(process.hThread.0) });
    // SAFETY: the call made the process handle for this process alone, and it is owned here once.
    let handle = unsafe { OwnedHandle::from_raw_handle(process.hProcess.0) };
    Ok(Child {
        process: handle,
        id: process.dwProcessId,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{self, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};
    use std::ffi::OsString;
    use std::os::windows::io::RawHandle;
    use windows::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE};
    use windows::Win32::System::Pipes::CreatePipe;
    use windows::Win32::System::Threading::TerminateProcess;

    const CHILD: &str = "halvo-child-handles=";

    fn pipe() -> (std::fs::File, std::fs::File) {
        let mut reads = HANDLE::default();
        let mut writes = HANDLE::default();
        // SAFETY: the call writes two new handles, not inheritable, into the live locals.
        unsafe { CreatePipe(&mut reads, &mut writes, None, 0) }.unwrap();
        // SAFETY: each handle is new, and each File owns its own once.
        unsafe {
            (
                std::fs::File::from_raw_handle(reads.0),
                std::fs::File::from_raw_handle(writes.0),
            )
        }
    }

    fn is_a_pipe(value: usize) -> bool {
        // SAFETY: the call reads the type of whatever the value names in this process and takes
        // no other pointer; a value that names no handle reads as FILE_TYPE_UNKNOWN.
        let kind = unsafe { GetFileType(HANDLE(value as RawHandle)) };
        kind == FILE_TYPE_PIPE
    }

    fn be_the_child(values: &str) {
        let values: Vec<usize> = values.split(',').map(|v| v.parse().unwrap()).collect();
        let [reads, writes, off_list] = values[..] else {
            panic!("three handle values: {values:?}")
        };
        // SAFETY: the parent listed this value, so this process inherited an open handle there,
        // which nothing else here owns.
        let mut from_parent = unsafe { std::fs::File::from_raw_handle(reads as RawHandle) };
        // SAFETY: as above, for the second listed value.
        let mut to_parent = unsafe { std::fs::File::from_raw_handle(writes as RawHandle) };
        let mut reply = wire::read_frame(&mut from_parent, REQUEST_MAX_BYTES).unwrap();
        let off_list: &[u8] = if is_a_pipe(off_list) {
            b" off-list=pipe"
        } else {
            b" off-list=none"
        };
        reply.extend_from_slice(off_list);
        wire::write_frame(&mut to_parent, &reply, REPLY_MAX_BYTES).unwrap();
    }

    #[test]
    fn a_child_inherits_the_listed_handles_and_no_other() {
        if let Some(values) =
            std::env::args().find_map(|arg| arg.strip_prefix(CHILD).map(str::to_owned))
        {
            be_the_child(&values);
            return;
        }
        let (child_reads, mut to_child) = pipe();
        let (mut from_child, child_writes) = pipe();
        let (_off_list_reads, off_list) = pipe();
        let off = HANDLE(off_list.as_raw_handle());
        // SAFETY: the handle is open and owned by `off_list`; the call takes no pointer.
        unsafe { SetHandleInformation(off, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) }.unwrap();
        let root = tempfile::tempdir().unwrap();
        let printed = root.path().join("child.txt");
        let output = std::fs::File::create(&printed).unwrap();
        let input = std::fs::File::open("NUL").unwrap();
        let module = module_path!().split_once("::").unwrap().1;
        let args: Vec<OsString> = vec![
            format!("{module}::a_child_inherits_the_listed_handles_and_no_other").into(),
            "--exact".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
            format!(
                "{CHILD}{},{},{}",
                child_reads.as_raw_handle() as usize,
                child_writes.as_raw_handle() as usize,
                off.0 as usize
            )
            .into(),
        ];
        let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
        let child = start_with_handles(
            &std::env::current_exe().unwrap(),
            &args,
            StdHandles {
                input: input.as_handle(),
                output: output.as_handle(),
                error: output.as_handle(),
            },
            &[child_reads.as_handle(), child_writes.as_handle()],
        )
        .unwrap();
        drop(child_reads);
        drop(child_writes);
        wire::write_frame(&mut to_child, b"one framed request", REQUEST_MAX_BYTES).unwrap();
        drop(to_child);
        let ended = child.wait(Duration::from_secs(60)).unwrap();
        if ended.is_none() {
            // SAFETY: the handle is open and names this test's own child, which has not ended.
            let _ = unsafe { TerminateProcess(HANDLE(child.as_handle().as_raw_handle()), 1) };
        }
        let text = std::fs::read_to_string(&printed).unwrap_or_default();
        assert_eq!(ended, Some(0), "the child printed: {text}");
        let reply = wire::read_frame(&mut from_child, REPLY_MAX_BYTES).unwrap();
        assert_eq!(
            reply, b"one framed request off-list=none",
            "the child printed: {text}"
        );
        assert!(
            is_a_pipe(off.0 as usize),
            "the off-list handle is no pipe here"
        );
        let mut flag = 0u32;
        // SAFETY: the handle is open and owned by `off_list`, and `flag` is a live local.
        unsafe { GetHandleInformation(off, &mut flag) }.unwrap();
        assert_eq!(
            flag & HANDLE_FLAG_INHERIT.0,
            HANDLE_FLAG_INHERIT.0,
            "the start changed the inherit flag of a handle off its list"
        );
    }
}
