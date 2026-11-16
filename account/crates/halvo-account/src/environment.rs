use crate::origin::Backend;
use crate::outcome::EnvironmentFault;
use halvo_secrets::Store;
#[cfg(debug_assertions)]
use halvo_secrets::{Directory, Name};
use std::ffi::OsString;
#[cfg(debug_assertions)]
use std::path::PathBuf;

/// The committed list of the environment variables that a debug build reads: the backend's
/// origin and the directory of the store. A release build reads none.
#[cfg(debug_assertions)]
pub const VARIABLES: [&str; 2] = ["HALVO_ACCOUNT_BACKEND", "HALVO_ACCOUNT_STORE_DIR"];

/// The committed list of the environment variables that a release build reads: none.
#[cfg(not(debug_assertions))]
pub const VARIABLES: [&str; 0] = [];

/// The name of the item that holds the sign-in in the store directory.
pub const ITEM: &str = "account";

/// The values of the variables of the committed list.
pub struct Environment {
    pub backend: Option<OsString>,
    pub store_dir: Option<OsString>,
}

impl Environment {
    /// Reads the variables of the committed list from the process, and no other.
    pub fn of_process() -> Environment {
        #[cfg(debug_assertions)]
        {
            Environment {
                backend: std::env::var_os(VARIABLES[0]),
                store_dir: std::env::var_os(VARIABLES[1]),
            }
        }
        #[cfg(not(debug_assertions))]
        {
            Environment {
                backend: None,
                store_dir: None,
            }
        }
    }
}

/// The backend of this build: in a debug build the value of `HALVO_ACCOUNT_BACKEND` as
/// `Backend::from_address` reads it, and in a release build the origin of its compile time.
pub fn backend(environment: &Environment) -> Result<Backend, EnvironmentFault> {
    #[cfg(debug_assertions)]
    {
        let value = environment
            .backend
            .as_ref()
            .ok_or(EnvironmentFault::NoBackend)?;
        let text = value.to_str().ok_or(EnvironmentFault::BadBackend)?;
        Backend::from_address(text).ok_or(EnvironmentFault::BadBackend)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = environment;
        Ok(Backend::compiled())
    }
}

/// The store of this build. A debug build keeps the sign-in as the item `account` in the
/// directory that `HALVO_ACCOUNT_STORE_DIR` names, through `Store::in_directory`, whatever
/// the features; with the variable unset it has no store, so it stores and reads nothing,
/// and it builds no other store. A release build takes its directory from the path of unit
/// 8.2, which is not in this crate yet, so a release build does not compile.
pub fn store(environment: &Environment) -> Result<Store, EnvironmentFault> {
    #[cfg(debug_assertions)]
    {
        let value = environment
            .store_dir
            .as_ref()
            .ok_or(EnvironmentFault::NoStoreDirectory)?;
        let directory =
            Directory::new(PathBuf::from(value)).ok_or(EnvironmentFault::BadStoreDirectory)?;
        let name = Name::new(ITEM).ok_or(EnvironmentFault::BadStoreDirectory)?;
        Ok(Store::in_directory(directory, name))
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = environment;
        compile_error!(
            "a release build of halvo-account has no store directory: the release path of unit 8.2 is not in the helper yet"
        )
    }
}
