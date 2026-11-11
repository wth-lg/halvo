pub mod frame;
pub mod pages;
pub mod suite;
pub mod sys;
pub mod text;
pub mod tree;
pub mod trial;

/// The commit that this build of the probes comes from, with `-dirty` when src/,
/// Cargo.toml, Cargo.lock or build.rs differed from it, and `unknown` without git.
pub const BUILD_COMMIT: &str = env!("R2_PROBES_COMMIT");
