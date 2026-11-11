//! The wire contract of the account helper, read from the files that `account/schema/`
//! vendors from the backend: the types of `schema.v1.json`, its three bounds, the app
//! routes of `routes.v1.json` and the builder of their requests.

pub mod request;
#[rustfmt::skip]
pub mod types;

include!(concat!(env!("OUT_DIR"), "/bounds.rs"));
