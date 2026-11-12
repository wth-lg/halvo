//! Where the account helper keeps its one secret, the credential of the person's account,
//! through the system's own store and nothing else.

#[cfg(not(any(target_os = "macos", windows)))]
compile_error!("halvo-secrets keeps its secret on macOS and on Windows only");

mod answer;
mod directory;
mod name;
mod place;
mod secret;
mod store;
#[cfg(target_os = "macos")]
mod unix;

pub use answer::{Delete, Fault, Locked, Read, StoreKind, Write};
pub use directory::Directory;
pub use name::Name;
pub use secret::{Secret, MAX_SECRET_LEN};
pub use store::Store;
