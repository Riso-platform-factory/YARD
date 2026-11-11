//! The window shell and extension component registry.

#[cfg(not(feature = "dev"))]
mod shell;
#[cfg(not(feature = "dev"))]
pub use shell::*;
#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub use dev::*;
