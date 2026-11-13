//! The labelled sample of the workflow run (contracts.md C9 to C12): the readers and writers of a body (the
//! tier manifest, the label rows, the script manifest and the script with its rules), the functions of C12.4,
//! and the commands `compose`, `split`, `build`, `check-labels`, `input-copy`, `leak` and `mask-layers`.
#![forbid(unsafe_code)]

pub mod body;
pub mod build;
pub mod check;
pub mod compose;
pub mod form;
pub mod input_copy;
pub mod labels;
pub mod layers;
pub mod leak;
pub mod manifest;
pub mod mask_layers;
pub mod rng;
pub mod script;
pub mod site_data;
pub mod split;
pub mod text;
