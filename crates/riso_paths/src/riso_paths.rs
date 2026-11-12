//! The application's config, data, and logs directories.

mod directories;
pub use directories::*;

#[cfg(feature = "dev")]
pub mod dev;
