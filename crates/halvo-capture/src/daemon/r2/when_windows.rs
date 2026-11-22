//! The system's readings on Windows: the last input of the session against the tick count (`GetLastInputInfo`,
//! `GetTickCount`), and the power status (`GetSystemPowerStatus`): `ACLineStatus` for the mains and
//! `SystemStatusFlag` for the battery saver. `GetLastInputInfo` answers for the session of the calling process alone.

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
    Some(Duration::from_millis(u64::from(idle_ms(
        now_tick(),
        info.dwTime,
    ))))
}

/// The milliseconds from the last input at the tick `last` to the tick count `now`, both of which wrap after 49.7
/// days. A difference of more than half the range is a `last` that lies after `now`, as a last input that the system
/// stamped after the tick count was read does, and is 0, not a long idle time; so an idle time of more than 24.8 days
/// reads as 0 too.
fn idle_ms(now: u32, last: u32) -> u32 {
    let since = now.wrapping_sub(last);
    if since > i32::MAX as u32 {
        0
    } else {
        since
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Rule 6: a last input whose tick lies after the tick count is idle 0, and a tick count that wrapped after the
    /// last input counts on across the wrap.
    #[test]
    fn a_last_input_after_the_tick_count_is_idle_zero() {
        assert_eq!(idle_ms(1_000, 1_016), 0);
        assert_eq!(idle_ms(5, u32::MAX - 4), 10);
    }

    /// One line of the three readings.
    fn line(reading: &Reading) -> String {
        let known =
            |value: Option<bool>| value.map_or_else(|| "unknown".to_owned(), |v| v.to_string());
        let idle = reading
            .idle
            .map_or_else(|| "unknown".to_owned(), |idle| idle.as_millis().to_string());
        format!(
            "r2-readings idle_ms={idle} mains={} low_power={}",
            known(reading.mains),
            known(reading.low_power)
        )
    }

    /// Presses and lets go of one key, F24, which no program of the runner reads.
    fn inject_one_key() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
            KEYEVENTF_KEYUP, VK_F24,
        };
        let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_F24,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
        // SAFETY: the call reads the two structures of `inputs`, whose size it is given.
        let sent = unsafe { SendInput(&inputs, core::mem::size_of::<INPUT>() as i32) };
        assert_eq!(sent, 2, "SendInput took {sent} of 2 inputs");
    }

    /// Rule 7, on a hosted runner alone: the three readings answer, the idle time falls below 5 s after one
    /// injected key, and a machine with no battery reads as on the mains with the battery saver off. It prints one
    /// line of the readings before the key and one after it.
    #[test]
    #[ignore = "it injects one key, on a hosted runner alone; a step of the windows job runs it by name"]
    fn the_readings_answer_on_this_machine() {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
            return;
        }
        let before = read();
        println!("{}", line(&before));
        inject_one_key();
        std::thread::sleep(Duration::from_millis(200));
        let after = read();
        println!("{}", line(&after));
        assert!(before.idle.is_some(), "no idle time before the key");
        let idle = after.idle.expect("no idle time after the key");
        assert!(
            idle < Duration::from_secs(5),
            "the idle time is {idle:?} after the key"
        );
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: the call writes the structure that `status` is.
        unsafe { GetSystemPowerStatus(&mut status) }.unwrap();
        let battery = status.BatteryFlag;
        assert_eq!(after.mains, Some(true), "the mains, BatteryFlag {battery}");
        assert_eq!(
            after.low_power,
            Some(false),
            "the battery saver, BatteryFlag {battery}"
        );
    }
}
