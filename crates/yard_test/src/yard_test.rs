//! Headless application and logging helpers for extension tests.

#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub use dev::*;
