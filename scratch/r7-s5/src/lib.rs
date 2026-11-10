//! R7 spikes S5 (credential custody across builds) and S6 (helper start).
//! Scratch code on a scratch branch, never merged.

pub mod keystore;
pub mod procstat;

#[cfg(feature = "net")]
pub mod client;
#[cfg(feature = "net")]
pub mod stats;
#[cfg(feature = "net")]
pub mod stub;

use std::time::Duration;

/// The build stamp makes two builds of the same source differ, as two versions of an
/// app differ. Without it a rebuild can be byte-identical and would not test an update.
pub const BUILD_STAMP: &str = match option_env!("S5_BUILD_STAMP") {
    Some(stamp) => stamp,
    None => "unstamped",
};

/// The key-store commands write to the login key store of whoever runs them, so they
/// run only inside a hosted runner job and never on a developer's machine.
pub fn require_hosted_runner() {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        eprintln!("refusing to touch the key store outside a hosted runner job");
        std::process::exit(2);
    }
}

/// Ends the process if it is still alive after `limit`. A key-store call that waits
/// for a dialog would otherwise hang the job, because nobody is there to answer it.
pub fn exit_after(limit: Duration, exit_code: i32) {
    std::thread::spawn(move || {
        std::thread::sleep(limit);
        println!(
            "no answer after {} s: a key-store dialog may be waiting",
            limit.as_secs()
        );
        std::process::exit(exit_code);
    });
}
