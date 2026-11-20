//! The repository's own tasks, run as `cargo xtask <task>`, as in Zed.

#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub use dev::*;
