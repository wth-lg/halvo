//! The system's readings on macOS: the seconds since the last input event of the HID system state
//! (`CGEventSourceSecondsSinceLastEventType`), the power source that provides the power
//! (`IOPSGetProvidingPowerSourceType`), and Low Power Mode (`lowPowerModeEnabled` of `NSProcessInfo`).

use std::ffi::c_void;
use std::time::Duration;

use core_foundation::base::{CFType, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use objc2::runtime::NSObjectProtocol;
use objc2::sel;
use objc2_foundation::NSProcessInfo;

use super::when::Reading;

/// `kCGEventSourceStateHIDSystemState`: the state of the hardware's input events.
const HID_SYSTEM_STATE: i32 = 1;

/// `kCGAnyInputEventType`: every kind of input event.
const ANY_INPUT_EVENT: u32 = !0;

/// `kIOPMACPowerKey`, `kIOPMBatteryPowerKey` and `kIOPMUPSPowerKey`: the answers of
/// `IOPSGetProvidingPowerSourceType`.
const AC_POWER: &str = "AC Power";
const BATTERY_POWER: &str = "Battery Power";
const UPS_POWER: &str = "UPS Power";

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> *const c_void;
    fn IOPSGetProvidingPowerSourceType(snapshot: *const c_void) -> CFStringRef;
}

/// One reading of the system.
pub(super) fn read() -> Reading {
    Reading {
        idle: idle(),
        mains: mains(),
        low_power: low_power(),
    }
}

/// The time since the last input event of the HID system state; none for a figure that is not a time.
fn idle() -> Option<Duration> {
    // SAFETY: a query of the system that takes two values and touches no memory of the caller.
    let seconds =
        unsafe { CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT_EVENT) };
    Duration::try_from_secs_f64(seconds).ok()
}

/// Whether the providing power source is the mains: yes for `AC Power`, no for the battery and for a UPS, none where
/// the system answers nothing or something else.
fn mains() -> Option<bool> {
    // SAFETY: takes no argument; the answer is null or a new reference, which the caller releases.
    let snapshot = unsafe { IOPSCopyPowerSourcesInfo() };
    if snapshot.is_null() {
        return None;
    }
    // SAFETY: the snapshot is a new reference that nothing else releases; the CFType releases it at its drop.
    let snapshot = unsafe { CFType::wrap_under_create_rule(snapshot) };
    // SAFETY: the argument is the snapshot of IOPSCopyPowerSourcesInfo, which lives to the end of this function; the
    // answer is null or a string that the snapshot holds.
    let kind = unsafe { IOPSGetProvidingPowerSourceType(snapshot.as_CFTypeRef()) };
    if kind.is_null() {
        return None;
    }
    // SAFETY: a string that the snapshot holds; the wrap retains it, so it outlives no reference.
    let kind = unsafe { CFString::wrap_under_get_rule(kind) }.to_string();
    match kind.as_str() {
        AC_POWER => Some(true),
        BATTERY_POWER | UPS_POWER => Some(false),
        _ => None,
    }
}

/// Whether Low Power Mode is on; none on a system that has no such mode to read.
fn low_power() -> Option<bool> {
    let info = NSProcessInfo::processInfo();
    if !info.respondsToSelector(sel!(isLowPowerModeEnabled)) {
        return None;
    }
    Some(info.isLowPowerModeEnabled())
}
