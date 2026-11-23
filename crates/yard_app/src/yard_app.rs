//! Application startup for YARD.

#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub use dev::*;
