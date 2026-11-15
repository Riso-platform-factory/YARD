//! The window shell and extension component registry.

mod prompt;
pub use prompt::*;
mod shell;
pub use shell::*;

#[cfg(feature = "dev")]
pub mod dev;
