use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetAce, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, READ_CONTROL,
};

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
    pub(crate) entries: Option<Vec<AccessEntry>>,
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
        entries,
    })
}

#[cfg(feature = "hosted-store-tests")]
pub(crate) fn access_list_lines(path: &Path) -> io::Result<Vec<String>> {
    let security = security_of(&open_dir(path)?)?;
    let mut lines = vec![format!(
        "owner {}",
        security.owner.as_deref().unwrap_or("none")
    )];
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
