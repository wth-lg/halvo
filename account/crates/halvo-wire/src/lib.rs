//! The wire contract of the account helper, read from the files that `account/schema/`
//! vendors from the backend: the types of `schema.v1.json` and its three bounds.

#[rustfmt::skip]
pub mod types;

include!(concat!(env!("OUT_DIR"), "/bounds.rs"));
