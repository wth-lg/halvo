use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeID, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    ptr::null,
};

type P = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementGetTypeID() -> CFTypeID;
    fn AXUIElementCreateApplication(pid: i32) -> P;
    fn AXUIElementSetMessagingTimeout(e: P, seconds: f32) -> i32;
    fn AXUIElementCopyAttributeValue(e: P, attr: CFStringRef, value: *mut CFTypeRef) -> i32;
    fn AXUIElementSetAttributeValue(e: P, attr: CFStringRef, value: CFTypeRef) -> i32;
    fn AXUIElementCopyAttributeNames(e: P, names: *mut CFArrayRef) -> i32;
    fn AXUIElementPerformAction(e: P, action: CFStringRef) -> i32;
}
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayPixelsWide(d: u32) -> usize;
    fn CGDisplayPixelsHigh(d: u32) -> usize;
    fn CGDisplayCopyAllDisplayModes(d: u32, options: P) -> CFArrayRef;
    fn CGDisplayModeGetWidth(m: P) -> usize;
    fn CGDisplayModeGetHeight(m: P) -> usize;
    fn CGDisplayModeGetPixelWidth(m: P) -> usize;
    fn CGDisplayModeGetPixelHeight(m: P) -> usize;
    fn CGDisplaySetDisplayMode(d: u32, m: P, options: P) -> i32;
    fn CGPreflightListenEventAccess() -> bool;
}
extern "C" {
    fn mach_timebase_info(info: *mut [u32; 2]) -> i32;
}

#[derive(Clone)]
pub struct Element(CFType);

fn copy(e: &Element, name: &str) -> Option<CFType> {
    let (key, mut v) = (CFString::new(name), null());
    let err = unsafe {
        AXUIElementCopyAttributeValue(e.0.as_CFTypeRef(), key.as_concrete_TypeRef(), &mut v)
    };
    (err == 0 && !v.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(v) })
}
fn element(v: &CFType) -> Option<Element> {
    (v.type_of() == unsafe { AXUIElementGetTypeID() }).then(|| Element(v.clone()))
}
fn elements(e: &Element, name: &str) -> Vec<Element> {
    let Some(v) = copy(e, name).filter(|v| v.instance_of::<CFArray>()) else {
        return Vec::new();
    };
    let a: CFArray<CFType> =
        unsafe { CFArray::wrap_under_get_rule(v.as_CFTypeRef() as CFArrayRef) };
    a.iter().filter_map(|i| element(&i)).collect()
}
pub fn ax_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}
pub fn app(pid: i32) -> Element {
    let raw = unsafe { AXUIElementCreateApplication(pid) };
    assert!(!raw.is_null());
    unsafe { AXUIElementSetMessagingTimeout(raw, 2.0) };
    Element(unsafe { CFType::wrap_under_create_rule(raw) })
}
pub fn attr_bool(e: &Element, name: &str) -> Option<bool> {
    let v = copy(e, name)?;
    if let Some(b) = v.downcast::<CFBoolean>() {
        return Some(b.into());
    }
    v.downcast::<CFNumber>()?.to_i64().map(|n| n != 0)
}
pub fn set_attr_bool(e: &Element, name: &str, v: bool) -> i32 {
    let key = CFString::new(name);
    let b = if v {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    unsafe {
        AXUIElementSetAttributeValue(
            e.0.as_CFTypeRef(),
            key.as_concrete_TypeRef(),
            b.as_CFTypeRef(),
        )
    }
}
pub fn attr_string(e: &Element, name: &str) -> Option<String> {
    let v = copy(e, name)?;
    if let Some(s) = v.downcast::<CFString>() {
        return Some(s.to_string());
    }
    if let Some(b) = v.downcast::<CFBoolean>() {
        return Some(bool::from(b).to_string());
    }
    let n = v.downcast::<CFNumber>()?;
    n.to_i64()
        .map(|i| i.to_string())
        .or_else(|| n.to_f64().map(|f| f.to_string()))
}
pub fn attr_element(e: &Element, name: &str) -> Option<Element> {
    element(&copy(e, name)?)
}
pub fn children(e: &Element) -> Vec<Element> {
    elements(e, "AXChildren")
}
pub fn windows(app: &Element) -> Vec<Element> {
    elements(app, "AXWindows")
}
pub fn attr_names(e: &Element) -> Vec<String> {
    let mut names: CFArrayRef = null();
    if unsafe { AXUIElementCopyAttributeNames(e.0.as_CFTypeRef(), &mut names) } != 0
        || names.is_null()
    {
        return Vec::new();
    }
    let a: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(names) };
    a.iter()
        .filter_map(|i| i.downcast::<CFString>().map(|s| s.to_string()))
        .collect()
}
pub fn perform(e: &Element, action: &str) -> i32 {
    let key = CFString::new(action);
    unsafe { AXUIElementPerformAction(e.0.as_CFTypeRef(), key.as_concrete_TypeRef()) }
}
pub fn display_size() -> (usize, usize) {
    unsafe {
        (
            CGDisplayPixelsWide(CGMainDisplayID()),
            CGDisplayPixelsHigh(CGMainDisplayID()),
        )
    }
}
pub fn set_display_size(w: usize, h: usize) -> Result<(usize, usize), String> {
    let d = unsafe { CGMainDisplayID() };
    let raw = unsafe { CGDisplayCopyAllDisplayModes(d, null()) };
    if raw.is_null() {
        return Err("no display modes".into());
    }
    let modes: CFArray<P> = unsafe { CFArray::wrap_under_create_rule(raw) };
    let (mut pick, mut seen) = (None, Vec::new());
    for m in modes.iter() {
        let m: P = *m;
        let (mw, mh) = unsafe { (CGDisplayModeGetWidth(m), CGDisplayModeGetHeight(m)) };
        let (pw, ph) = unsafe {
            (
                CGDisplayModeGetPixelWidth(m),
                CGDisplayModeGetPixelHeight(m),
            )
        };
        seen.push(format!("{mw}x{mh}@{pw}x{ph}"));
        if mw == w && mh == h && (pick.is_none() || (pw == w && ph == h)) {
            pick = Some(m);
        }
    }
    let mode = pick.ok_or_else(|| format!("no {w}x{h} mode in: {}", seen.join(" ")))?;
    match unsafe { CGDisplaySetDisplayMode(d, mode, null()) } {
        0 => Ok(display_size()),
        err => Err(format!("CGDisplaySetDisplayMode: CGError {err}")),
    }
}
pub fn listen_event_preflight() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

pub fn all_pids() -> Vec<i32> {
    let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) }.max(0) as usize;
    let mut buf = vec![0i32; n + 64];
    let got = unsafe { libc::proc_listallpids(buf.as_mut_ptr().cast(), (buf.len() * 4) as i32) };
    buf.truncate(got.max(0) as usize);
    buf
}
pub fn parent_pid(pid: i32) -> Option<i32> {
    let mut info: libc::proc_bsdinfo = unsafe { zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as i32;
    let p = (&mut info as *mut libc::proc_bsdinfo).cast();
    let r = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, p, size) };
    (r == size).then_some(info.pbi_ppid as i32)
}
pub fn descendants(root: i32) -> Vec<i32> {
    let pairs: Vec<(i32, i32)> = all_pids()
        .into_iter()
        .filter_map(|p| Some((p, parent_pid(p)?)))
        .collect();
    let (mut out, mut todo) = (Vec::new(), vec![root]);
    while let Some(parent) = todo.pop() {
        for &(p, pp) in &pairs {
            if pp == parent && p != root && !out.contains(&p) {
                out.push(p);
                todo.push(p);
            }
        }
    }
    out
}
pub struct ProcessUsage {
    pub cpu_ns: u64,
    pub footprint: u64,
    pub peak_footprint: u64,
}
pub fn path_of(pid: i32) -> String {
    let mut buf = vec![0u8; 4096];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    String::from_utf8_lossy(&buf[..n.max(0) as usize]).to_string()
}
pub fn usage(pid: i32) -> Option<ProcessUsage> {
    let mut ri: libc::rusage_info_v4 = unsafe { zeroed() };
    let p = (&mut ri as *mut libc::rusage_info_v4).cast::<libc::rusage_info_t>();
    if unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, p) } != 0 {
        return None;
    }
    let mut tb = [0u32; 2];
    unsafe { mach_timebase_info(&mut tb) };
    let ns = |t: u64| (t as u128 * tb[0] as u128 / tb[1].max(1) as u128) as u64;
    Some(ProcessUsage {
        cpu_ns: ns(ri.ri_user_time) + ns(ri.ri_system_time),
        footprint: ri.ri_phys_footprint,
        peak_footprint: ri.ri_lifetime_max_phys_footprint,
    })
}
