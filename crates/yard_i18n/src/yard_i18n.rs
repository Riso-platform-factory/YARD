//! App-local translation catalogs and language settings.

#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub use dev::*;
