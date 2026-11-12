//! Where the account helper keeps its one secret, the credential of the person's account,
//! through the system's own store and nothing else.

mod name;
mod secret;

pub use name::Name;
pub use secret::{Secret, MAX_SECRET_LEN};
