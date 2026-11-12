//! The window shell and extension component registry.

mod shell;
pub use shell::*;
#[cfg(feature = "dev")]
pub mod dev;
