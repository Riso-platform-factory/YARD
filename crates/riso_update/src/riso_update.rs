//! Checks the product's update address for a newer version.

mod check;
pub use check::*;

#[cfg(feature = "dev")]
pub mod dev;
