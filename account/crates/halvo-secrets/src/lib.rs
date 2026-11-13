//! Where the account helper keeps its one secret, the credential of the person's account,
//! through the system's own store and nothing else.

#![deny(clippy::undocumented_unsafe_blocks)]

#[cfg(not(any(target_os = "macos", windows)))]
compile_error!("halvo-secrets keeps its secret on macOS and on Windows only");

#[cfg(all(feature = "hosted-store-tests", not(debug_assertions)))]
compile_error!(
    "the feature hosted-store-tests holds test hooks (Store::with_move_fault, MoveFault, access_list_lines) that make the store fail or read other bytes; it builds only with debug assertions, so that no release build carries them"
);

mod answer;
mod directory;
#[cfg(windows)]
mod dpapi;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod keychain;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod mac;
mod name;
mod place;
mod secret;
#[cfg(windows)]
#[cfg_attr(not(feature = "hosted-store-tests"), allow(dead_code))]
mod security;
mod store;
#[cfg(target_os = "macos")]
mod unix;
#[cfg_attr(not(windows), allow(dead_code))]
mod windows_place;

pub use answer::{Delete, Fault, Locked, Read, StoreKind, Write};
pub use directory::Directory;
#[cfg(all(
    target_os = "macos",
    feature = "hosted-store-tests",
    feature = "keychain"
))]
#[doc(hidden)]
pub use keychain::failing::MoveFault;
pub use name::Name;
pub use secret::{Secret, MAX_SECRET_LEN};
pub use store::Store;

/// Builds with debug assertions and the feature `hosted-store-tests`, on Windows only: the
/// owner and each access entry of the directory at `path`,
/// one line each, for the hosted test that records what the runner's directories carry.
#[cfg(all(windows, feature = "hosted-store-tests"))]
#[doc(hidden)]
pub fn access_list_lines(path: &std::path::Path) -> std::io::Result<Vec<String>> {
    security::access_list_lines(path)
}
