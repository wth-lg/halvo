//! Where the account helper keeps its one secret, the credential of the person's account,
//! through the system's own store and nothing else.

#[cfg(not(any(target_os = "macos", windows)))]
compile_error!("halvo-secrets keeps its secret on macOS and on Windows only");

mod answer;
mod directory;
#[cfg(windows)]
mod dpapi;
#[cfg_attr(not(all(target_os = "macos", feature = "keychain")), allow(dead_code))]
mod keychain;
mod name;
mod place;
mod secret;
#[cfg(windows)]
#[cfg_attr(not(feature = "hosted-store-tests"), allow(dead_code))]
mod security;
mod store;
#[cfg(target_os = "macos")]
mod unix;

pub use answer::{Delete, Fault, Locked, Read, StoreKind, Write};
pub use directory::Directory;
pub use name::Name;
pub use secret::{Secret, MAX_SECRET_LEN};
pub use store::Store;

/// Test builds on Windows only: the owner and each access entry of the directory at `path`,
/// one line each, for the hosted test that records what the runner's directories carry.
#[cfg(all(windows, feature = "hosted-store-tests"))]
#[doc(hidden)]
pub fn access_list_lines(path: &std::path::Path) -> std::io::Result<Vec<String>> {
    security::access_list_lines(path)
}
