//! The labelled sample of the workflow run (contracts.md C9 to C12): the readers and writers of a body (the
//! tier manifest, the label rows, the script manifest and the script with its rules), the functions of C12.4,
//! and the commands `compose`, `split`, `build`, `check-labels`, `input-copy`, `leak` and `mask-layers`.
#![forbid(unsafe_code)]

pub mod body;
pub mod form;
pub mod labels;
pub mod layers;
pub mod manifest;
pub mod rng;
pub mod script;
pub mod text;
