//! The application's build config, `product.toml`, read and checked into
//! Rust types at build time, without GPUI.

#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub use dev::*;
