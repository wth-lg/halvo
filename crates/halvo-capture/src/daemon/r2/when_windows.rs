//! The system's readings on Windows: the last input of the session against the tick count (`GetLastInputInfo`,
//! `GetTickCount`), and the power status (`GetSystemPowerStatus`): `ACLineStatus` for the mains and
//! `SystemStatusFlag` for the battery saver.

use std::time::Duration;

use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

use super::when::Reading;
use crate::windows::now_tick;

/// One reading of the system.
pub(super) fn read() -> Reading {
    let (mains, low_power) = power();
    Reading {
        idle: idle(),
        mains,
        low_power,
    }
}

/// The time since the last input of the session, in the milliseconds of the tick count, which wrap after 49.7 days.
fn idle() -> Option<Duration> {
    let mut info = LASTINPUTINFO {
        cbSize: core::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: the call writes the structure that `info` is, whose size `cbSize` names.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return None;
    }
    let since = now_tick().wrapping_sub(info.dwTime);
    Some(Duration::from_millis(u64::from(since)))
}

/// Whether the computer is on the mains (`ACLineStatus` 1) and whether the battery saver is on (`SystemStatusFlag`
/// 1); none for a value that is neither 0 nor 1, and both none where the call fails.
fn power() -> (Option<bool>, Option<bool>) {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: the call writes the structure that `status` is.
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        return (None, None);
    }
    let flag = |value: u8| match value {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    (flag(status.ACLineStatus), flag(status.SystemStatusFlag))
}
