use crate::windows_place::store_dir_sddl;
use std::ffi::c_void;
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetAce, GetSecurityDescriptorControl, GetTokenInformation, TokenUser, ACCESS_ALLOWED_ACE,
    ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    PSID, SECURITY_ATTRIBUTES, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, READ_CONTROL,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const ALLOWED: u8 = 0;
const DENIED: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccessEntry {
    pub(crate) kind: u8,
    pub(crate) flags: u8,
    pub(crate) mask: u32,
    pub(crate) sid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Security {
    pub(crate) owner: Option<String>,
    pub(crate) protected: bool,
    pub(crate) entries: Option<Vec<AccessEntry>>,
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain([0]).collect()
}

pub(crate) fn open_dir_following(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

pub(crate) fn owner_of(dir: &File) -> io::Result<Option<String>> {
    let mut owner: PSID = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle is open for as long as `dir` is borrowed here; the owner that the
    // call returns points into `descriptor`, which is read below and then freed once.
    let code = unsafe {
        GetSecurityInfo(
            dir.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let text = sid_text(owner);
    // SAFETY: `descriptor` came from GetSecurityInfo, the owner's text is copied out by now,
    // and this is its one release.
    unsafe { LocalFree(descriptor) };
    Ok(text)
}

pub(crate) fn user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo handle of this process needs no release, and the call writes the
    // local `token`, which CloseHandle releases below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0u32;
    // SAFETY: a call with no buffer only writes the needed size into the local `len`.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut len) };
    let mut info = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `info` holds at least `len` bytes, aligned for the pointers of TOKEN_USER, and
    // lives until the SID inside it has been turned into text below.
    let ok =
        unsafe { GetTokenInformation(token, TokenUser, info.as_mut_ptr().cast(), len, &mut len) };
    let error = io::Error::last_os_error();
    // SAFETY: `token` was opened above and is closed once, here.
    unsafe { CloseHandle(token) };
    if ok == 0 {
        return Err(error);
    }
    // SAFETY: on success the buffer begins with a TOKEN_USER whose SID points into `info`.
    let user = unsafe { &*info.as_ptr().cast::<TOKEN_USER>() };
    sid_text(user.User.Sid).ok_or_else(io::Error::last_os_error)
}

pub(crate) fn make_store_dir(path: &Path, user: &str) -> io::Result<()> {
    let sddl = wide(OsStr::new(&store_dir_sddl(user)));
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `sddl` is a NUL-terminated wide string that lives through the call, which writes
    // the local `descriptor` with a buffer that LocalFree releases below.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let name = wide(path.as_os_str());
    // SAFETY: `name` is a NUL-terminated wide string and `attributes` points at the live
    // descriptor; the call copies both and keeps neither.
    let made = unsafe { CreateDirectoryW(name.as_ptr(), &attributes) };
    let error = io::Error::last_os_error();
    // SAFETY: `descriptor` came from the conversion above, the call no longer needs it, and
    // this is its one release.
    unsafe { LocalFree(descriptor) };
    if made != 0 || error.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) {
        Ok(())
    } else {
        Err(error)
    }
}

pub(crate) fn open_dir(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn sid_text(sid: PSID) -> Option<String> {
    if sid.is_null() {
        return None;
    }
    let mut text = ptr::null_mut();
    // SAFETY: `sid` points into a security descriptor that the caller keeps alive through
    // this call; the call writes the local `text` with a string that LocalFree releases below.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return None;
    }
    let mut len = 0;
    // SAFETY: the string that the call returned is NUL-terminated, so every index up to and
    // including the terminator lies inside it.
    while unsafe { *text.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: the `len` characters before the terminator are initialized and stay valid until
    // the LocalFree below.
    let units = unsafe { std::slice::from_raw_parts(text, len) };
    let owned = String::from_utf16_lossy(units);
    // SAFETY: the string came from ConvertSidToStringSidW, whose release is LocalFree, and
    // nothing reads it after this line.
    unsafe { LocalFree(text.cast()) };
    Some(owned)
}

pub(crate) fn security_of(dir: &File) -> io::Result<Security> {
    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle is open for as long as `dir` is borrowed here; the call writes four
    // locals, and the owner and list it returns point into `descriptor`, which LocalFree
    // releases once at the end.
    let code = unsafe {
        GetSecurityInfo(
            dir.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let owner_text = sid_text(owner);
    let mut control = 0u16;
    let mut revision = 0u32;
    // SAFETY: `descriptor` is the live one from GetSecurityInfo; the call writes two locals.
    let read = unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
    let protected = read != 0 && control & SE_DACL_PROTECTED != 0;
    let mut entries = None;
    if !dacl.is_null() {
        // SAFETY: a non-null list from GetSecurityInfo points into the live `descriptor`.
        let count = unsafe { (*dacl).AceCount };
        let mut read = Vec::with_capacity(usize::from(count));
        for index in 0..u32::from(count) {
            let mut ace: *mut c_void = ptr::null_mut();
            // SAFETY: `index` is below the list's own count, and the call writes the local
            // `ace` with a pointer into the live `descriptor`.
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                let error = io::Error::last_os_error();
                // SAFETY: `descriptor` came from GetSecurityInfo and is freed once, here,
                // before the early return.
                unsafe { LocalFree(descriptor) };
                return Err(error);
            }
            // SAFETY: every entry of a list begins with an ACE_HEADER, which GetAce pointed at.
            let header = unsafe { *ace.cast::<ACE_HEADER>() };
            let body = ace.cast::<ACCESS_ALLOWED_ACE>();
            // SAFETY: every access entry kind keeps its mask right after the header, inside
            // the entry's own AceSize bytes.
            let mask = unsafe { (*body).Mask };
            let sid = if header.AceType == ALLOWED || header.AceType == DENIED {
                // SAFETY: an allow or deny entry stores its SID from SidStart onward, inside
                // the live descriptor; only the address is taken here.
                let start = unsafe { ptr::addr_of_mut!((*body).SidStart) };
                sid_text(start.cast())
            } else {
                None
            };
            read.push(AccessEntry {
                kind: header.AceType,
                flags: header.AceFlags,
                mask,
                sid,
            });
        }
        entries = Some(read);
    }
    // SAFETY: `descriptor` came from GetSecurityInfo, every pointer into it is dead by now,
    // and this is its one release.
    unsafe { LocalFree(descriptor) };
    Ok(Security {
        owner: owner_text,
        protected,
        entries,
    })
}

#[cfg(feature = "hosted-store-tests")]
pub(crate) fn access_list_lines(path: &Path) -> io::Result<Vec<String>> {
    let security = security_of(&open_dir(path)?)?;
    let mut lines = vec![
        format!("owner {}", security.owner.as_deref().unwrap_or("none")),
        format!(
            "protected {}",
            if security.protected { "yes" } else { "no" }
        ),
    ];
    match security.entries {
        None => lines.push("no access list: every right for everyone".to_string()),
        Some(entries) => {
            for entry in entries {
                let kind = match entry.kind {
                    ALLOWED => "allow".to_string(),
                    DENIED => "deny".to_string(),
                    other => format!("kind {other}"),
                };
                lines.push(format!(
                    "{kind} {} mask {:#010x} flags {:#04x}",
                    entry.sid.as_deref().unwrap_or("-"),
                    entry.mask,
                    entry.flags
                ));
            }
        }
    }
    Ok(lines)
}
