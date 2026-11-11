//! The probe of the list of what the reading of the cap does not count (the crate's
//! documentation): `hold <way>` holds 134,217,728 bytes in that way for 2 s, writes a pattern
//! and reads it back, and prints `hold way=<way> held_bytes=<n>`, the bytes that it read
//! back intact. Run it under `measure-peak --limit 67108864 --runs 1 -- <this program> <way>`.
//! Ways on macOS: child-process, exec, mapped-read, mapped-written, purgeable, reusable,
//! kernel-message, socket-buffer. Ways on Windows: child-process, mapped-file, section,
//! broker-process.

use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

const BYTES: usize = 134_217_728;
const PARTS: usize = 4;
const HOLD: Duration = Duration::from_millis(2000);

fn word(index: usize, seed: u64) -> u64 {
    ((index as u64) ^ seed).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1
}

fn write_words(base: *mut u64, from: usize, to: usize, seed: u64) {
    for k in from..to {
        unsafe { base.add(k).write_volatile(word(k, seed)) };
    }
}

fn intact_bytes(base: *const u64, from: usize, to: usize, seed: u64) -> usize {
    (from..to)
        .filter(|&k| unsafe { base.add(k).read_volatile() } == word(k, seed))
        .count()
        * 8
}

fn fail(call: &str, error: std::io::Error) -> ! {
    println!(
        "hold error call={call} code={}",
        error.raw_os_error().unwrap_or(-1)
    );
    std::process::exit(3);
}

fn part(bytes: usize) -> usize {
    let words = bytes / 8;
    let mut held: Vec<u64> = Vec::new();
    if held.try_reserve_exact(words).is_err() {
        return 0;
    }
    held.resize(words, 0);
    let base = held.as_mut_ptr();
    write_words(base, 0, words, 7);
    std::thread::sleep(HOLD);
    let intact = intact_bytes(base, 0, words, 7);
    std::hint::black_box(&held);
    intact
}

fn child_process() -> usize {
    let exe = std::env::current_exe().unwrap_or_else(|e| fail("current_exe", e));
    let size = (BYTES / PARTS).to_string();
    let children: Vec<_> = (0..PARTS)
        .map(|_| {
            Command::new(&exe)
                .args(["part", &size])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap_or_else(|e| fail("spawn", e))
        })
        .collect();
    children
        .into_iter()
        .map(|child| {
            let output = child.wait_with_output().unwrap_or_else(|e| fail("wait", e));
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .find_map(|l| l.strip_prefix("part held_bytes="))
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0)
        })
        .sum()
}

#[cfg(target_os = "macos")]
mod way {
    use super::*;
    use std::ffi::{c_int, c_void};
    use std::io::Write;
    use std::os::fd::AsRawFd;

    const MIB: usize = 1_048_576;
    const VM_FLAGS_ANYWHERE: c_int = 0x0001;
    const VM_FLAGS_PURGABLE: c_int = 0x0002;

    fn byte(offset: usize, seed: u64) -> u8 {
        word(offset / 8, seed).to_ne_bytes()[offset % 8]
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Header {
        bits: u32,
        size: u32,
        remote: u32,
        local: u32,
        voucher: u32,
        id: i32,
    }

    #[repr(C, packed(4))]
    #[derive(Clone, Copy)]
    struct OutOfLine {
        address: *mut c_void,
        flags: u32,
        size: u32,
    }

    #[repr(C, packed(4))]
    struct Sent {
        header: Header,
        count: u32,
        data: OutOfLine,
    }

    #[repr(C, packed(4))]
    struct Received {
        header: Header,
        count: u32,
        data: OutOfLine,
        trailer: [u32; 2],
    }

    extern "C" {
        static mach_task_self_: libc::mach_port_t;
        fn vm_purgable_control(
            task: libc::vm_map_t,
            address: libc::vm_address_t,
            control: c_int,
            state: *mut c_int,
        ) -> libc::kern_return_t;
        fn mach_port_allocate(
            task: libc::mach_port_t,
            right: u32,
            name: *mut libc::mach_port_t,
        ) -> libc::kern_return_t;
        fn mach_port_insert_right(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
            right: libc::mach_port_t,
            kind: u32,
        ) -> libc::kern_return_t;
        fn mach_port_set_attributes(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
            flavor: c_int,
            info: *mut c_int,
            count: u32,
        ) -> libc::kern_return_t;
        fn mach_msg(
            message: *mut Header,
            option: c_int,
            send_size: u32,
            receive_size: u32,
            receive_name: libc::mach_port_t,
            timeout: u32,
            notify: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }

    fn kern(call: &str, kr: libc::kern_return_t) {
        if kr != 0 {
            println!("hold error call={call} code={kr}");
            std::process::exit(3);
        }
    }

    fn anonymous(bytes: usize) -> *mut u64 {
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANON | libc::MAP_PRIVATE,
                -1,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            fail("mmap", std::io::Error::last_os_error());
        }
        p.cast()
    }

    pub fn run(way: &str) -> Option<usize> {
        Some(match way {
            "child-process" => child_process(),
            "exec" => exec(),
            "mapped-read" => mapped(false),
            "mapped-written" => mapped(true),
            "purgeable" => purgeable(),
            "reusable" => reusable(),
            "kernel-message" => kernel_message(),
            "socket-buffer" => socket_buffer(),
            _ => return None,
        })
    }

    fn exec() -> usize {
        use std::os::unix::process::CommandExt;
        let base = anonymous(BYTES);
        write_words(base, 0, BYTES / 8, 11);
        let held = intact_bytes(base, 0, BYTES / 8, 11);
        let exe = std::env::current_exe().unwrap_or_else(|e| fail("current_exe", e));
        let error = Command::new(exe)
            .args(["after-exec", &held.to_string()])
            .exec();
        fail("execv", error)
    }

    fn mapped(through_the_mapping: bool) -> usize {
        let path = std::env::temp_dir().join(format!("halvo-cap-hold-{}.bin", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap_or_else(|e| fail("open", e));
        let seed = 13;
        if through_the_mapping {
            file.set_len(BYTES as u64)
                .unwrap_or_else(|e| fail("set_len", e));
        } else {
            let mut chunk = vec![0u8; MIB];
            for c in 0..BYTES / MIB {
                for (j, b) in chunk.iter_mut().enumerate() {
                    *b = byte(c * MIB + j, seed);
                }
                file.write_all(&chunk).unwrap_or_else(|e| fail("write", e));
            }
            file.sync_all().unwrap_or_else(|e| fail("fsync", e));
        }
        let protection = if through_the_mapping {
            libc::PROT_READ | libc::PROT_WRITE
        } else {
            libc::PROT_READ
        };
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                BYTES,
                protection,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if p == libc::MAP_FAILED {
            fail("mmap", std::io::Error::last_os_error());
        }
        let base = p.cast::<u64>();
        if through_the_mapping {
            write_words(base, 0, BYTES / 8, seed);
        } else {
            std::hint::black_box(intact_bytes(base, 0, BYTES / 8, seed));
        }
        std::thread::sleep(HOLD);
        let held = intact_bytes(base, 0, BYTES / 8, seed);
        unsafe { libc::munmap(p, BYTES) };
        let _ = file.set_len(0);
        held
    }

    fn purgeable() -> usize {
        let task = unsafe { mach_task_self_ };
        let mut address: libc::vm_address_t = 0;
        kern("vm_allocate", unsafe {
            libc::vm_allocate(
                task,
                &mut address,
                BYTES,
                VM_FLAGS_ANYWHERE | VM_FLAGS_PURGABLE,
            )
        });
        let mut volatile: c_int = 1;
        kern("vm_purgable_control", unsafe {
            vm_purgable_control(task, address, 0, &mut volatile)
        });
        let base = address as *mut u64;
        write_words(base, 0, BYTES / 8, 17);
        std::thread::sleep(HOLD);
        intact_bytes(base, 0, BYTES / 8, 17)
    }

    fn reusable() -> usize {
        let base = anonymous(BYTES);
        let words = MIB / 8;
        for c in 0..BYTES / MIB {
            write_words(base, c * words, (c + 1) * words, 19);
            let rc =
                unsafe { libc::madvise(base.add(c * words).cast(), MIB, libc::MADV_FREE_REUSABLE) };
            if rc != 0 {
                fail("madvise", std::io::Error::last_os_error());
            }
        }
        std::thread::sleep(HOLD);
        intact_bytes(base, 0, BYTES / 8, 19)
    }

    fn kernel_message() -> usize {
        let task = unsafe { mach_task_self_ };
        let mut port: libc::mach_port_t = 0;
        kern("mach_port_allocate", unsafe {
            mach_port_allocate(task, 1, &mut port)
        });
        kern("mach_port_insert_right", unsafe {
            mach_port_insert_right(task, port, port, 20)
        });
        let mut queue_limit: c_int = 1024;
        kern("mach_port_set_attributes", unsafe {
            mach_port_set_attributes(task, port, 1, &mut queue_limit, 1)
        });
        let mut buffer: libc::vm_address_t = 0;
        kern("vm_allocate", unsafe {
            libc::vm_allocate(task, &mut buffer, MIB, VM_FLAGS_ANYWHERE)
        });
        let messages = BYTES / MIB;
        let mut sent = 0;
        for k in 0..messages {
            write_words(buffer as *mut u64, 0, MIB / 8, 100 + k as u64);
            let mut message = Sent {
                header: Header {
                    bits: 19 | 0x8000_0000,
                    size: std::mem::size_of::<Sent>() as u32,
                    remote: port,
                    local: 0,
                    voucher: 0,
                    id: k as i32,
                },
                count: 1,
                data: OutOfLine {
                    address: buffer as *mut c_void,
                    flags: (1 << 8) | (1 << 24),
                    size: MIB as u32,
                },
            };
            let kr = unsafe {
                mach_msg(
                    std::ptr::addr_of_mut!(message.header),
                    0x1 | 0x10,
                    std::mem::size_of::<Sent>() as u32,
                    0,
                    0,
                    0,
                    0,
                )
            };
            if kr != 0 {
                println!("hold error call=mach_msg-send code={kr} sent={sent}");
                break;
            }
            sent += 1;
        }
        std::thread::sleep(HOLD);
        let mut held = 0;
        for _ in 0..sent {
            let mut message: Received = unsafe { std::mem::zeroed() };
            let kr = unsafe {
                mach_msg(
                    std::ptr::addr_of_mut!(message.header),
                    0x2 | 0x100,
                    0,
                    std::mem::size_of::<Received>() as u32,
                    port,
                    1000,
                    0,
                )
            };
            if kr != 0 {
                println!("hold error call=mach_msg-receive code={kr}");
                break;
            }
            let header = message.header;
            let data = message.data;
            let words = data.size as usize / 8;
            held += intact_bytes(data.address as *const u64, 0, words, 100 + header.id as u64);
            unsafe {
                libc::vm_deallocate(task, data.address as libc::vm_address_t, data.size as usize)
            };
        }
        held
    }

    fn socket_buffer() -> usize {
        let mut files = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut files) } == 0 {
            files.rlim_cur = files.rlim_max.min(4096);
            unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &files) };
        }
        let most = (files.rlim_cur as usize).saturating_sub(16) / 2;
        let mut pairs: Vec<[c_int; 2]> = Vec::new();
        let mut chunk = vec![0u8; 65_536];
        let mut written = 0usize;
        let mut lengths: Vec<usize> = Vec::new();
        while written < BYTES && pairs.len() < most {
            let mut pair = [0 as c_int; 2];
            if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, pair.as_mut_ptr()) }
                != 0
            {
                fail("socketpair", std::io::Error::last_os_error());
            }
            let size: c_int = 8 * MIB as c_int;
            for (fd, option) in [(pair[0], libc::SO_SNDBUF), (pair[1], libc::SO_RCVBUF)] {
                unsafe {
                    libc::setsockopt(
                        fd,
                        libc::SOL_SOCKET,
                        option,
                        std::ptr::addr_of!(size).cast(),
                        std::mem::size_of::<c_int>() as libc::socklen_t,
                    )
                };
                unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) };
            }
            let seed = 23 + pairs.len() as u64;
            let mut length = 0usize;
            while written < BYTES {
                let n = chunk.len().min(BYTES - written);
                for (j, b) in chunk[..n].iter_mut().enumerate() {
                    *b = byte(length + j, seed);
                }
                let w = unsafe { libc::write(pair[0], chunk.as_ptr().cast(), n) };
                if w <= 0 {
                    break;
                }
                length += w as usize;
                written += w as usize;
            }
            pairs.push(pair);
            lengths.push(length);
        }
        println!("hold socket pairs={}", pairs.len());
        std::thread::sleep(HOLD);
        let mut held = 0;
        for (i, pair) in pairs.iter().enumerate() {
            let seed = 23 + i as u64;
            let mut offset = 0usize;
            while offset < lengths[i] {
                let r = unsafe { libc::read(pair[1], chunk.as_mut_ptr().cast(), chunk.len()) };
                if r <= 0 {
                    break;
                }
                held += chunk[..r as usize]
                    .iter()
                    .enumerate()
                    .filter(|&(j, &b)| b == byte(offset + j, seed))
                    .count();
                offset += r as usize;
            }
        }
        held
    }
}

#[cfg(windows)]
mod way {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};

    const PAGE_READWRITE: u32 = 0x04;
    const FILE_MAP_ALL_ACCESS: u32 = 0x000f_001f;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileMappingW(
            file: HANDLE,
            attributes: *const c_void,
            protection: u32,
            size_high: u32,
            size_low: u32,
            name: *const u16,
        ) -> HANDLE;
        fn MapViewOfFile(
            mapping: HANDLE,
            access: u32,
            offset_high: u32,
            offset_low: u32,
            bytes: usize,
        ) -> *mut c_void;
        fn UnmapViewOfFile(base: *const c_void) -> i32;
    }

    pub fn run(way: &str) -> Option<usize> {
        Some(match way {
            "child-process" => child_process(),
            "mapped-file" => mapped_file(),
            "section" => section(INVALID_HANDLE_VALUE),
            "broker-process" => broker_process(),
            _ => return None,
        })
    }

    fn section(file: HANDLE) -> usize {
        let mapping = unsafe {
            CreateFileMappingW(
                file,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                BYTES as u32,
                std::ptr::null(),
            )
        };
        if mapping.is_null() {
            fail("CreateFileMappingW", std::io::Error::last_os_error());
        }
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, BYTES) };
        if view.is_null() {
            fail("MapViewOfFile", std::io::Error::last_os_error());
        }
        let base = view.cast::<u64>();
        write_words(base, 0, BYTES / 8, 29);
        std::thread::sleep(HOLD);
        let held = intact_bytes(base, 0, BYTES / 8, 29);
        unsafe { UnmapViewOfFile(view) };
        unsafe { CloseHandle(mapping) };
        held
    }

    fn mapped_file() -> usize {
        let path = std::env::temp_dir().join(format!("halvo-cap-hold-{}.bin", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap_or_else(|e| fail("open", e));
        file.set_len(BYTES as u64)
            .unwrap_or_else(|e| fail("set_len", e));
        let held = section(file.as_raw_handle() as HANDLE);
        let _ = file.set_len(0);
        held
    }

    fn broker_process() -> usize {
        let exe = std::env::current_exe().unwrap_or_else(|e| fail("current_exe", e));
        let result =
            std::env::temp_dir().join(format!("halvo-cap-hold-broker-{}.txt", std::process::id()));
        let line = format!("{} part {} {}", exe.display(), BYTES, result.display());
        if line.matches(' ').count() != 3 {
            println!("hold error call=schtasks-path-with-a-space code=-1");
            std::process::exit(3);
        }
        let name = format!("halvo-cap-hold-{}", std::process::id());
        let schtasks = |args: &[&str]| {
            Command::new("schtasks")
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.code().unwrap_or(-1))
                .unwrap_or(-1)
        };
        let created = schtasks(&[
            "/create", "/tn", &name, "/tr", &line, "/sc", "once", "/st", "23:59", "/f",
        ]);
        let ran = schtasks(&["/run", "/tn", &name]);
        let mut held = 0;
        for _ in 0..120 {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(n) = std::fs::read_to_string(&result).ok().and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("part held_bytes=")?.parse::<usize>().ok())
            }) {
                held = n;
                break;
            }
        }
        let deleted = schtasks(&["/delete", "/tn", &name, "/f"]);
        println!("hold schtasks create={created} run={ran} delete={deleted}");
        held
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod way {
    pub fn run(_way: &str) -> Option<usize> {
        None
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["part", bytes, rest @ ..] => {
            let held = bytes.parse().map(part).unwrap_or(0);
            let text = format!("part held_bytes={held}\n");
            match rest {
                [file] => {
                    let _ = std::fs::write(file, text);
                }
                _ => print!("{text}"),
            }
            ExitCode::SUCCESS
        }
        ["after-exec", held] => {
            println!("hold way=exec held_bytes={held}");
            ExitCode::SUCCESS
        }
        [way] => match way::run(way) {
            Some(held) => {
                println!("hold way={way} held_bytes={held}");
                ExitCode::SUCCESS
            }
            None => {
                println!("hold error call=unknown-way code=64");
                ExitCode::from(64)
            }
        },
        _ => {
            println!("hold error call=usage code=64");
            ExitCode::from(64)
        }
    }
}
