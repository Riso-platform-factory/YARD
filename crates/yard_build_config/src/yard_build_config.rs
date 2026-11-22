//! The application's build config, `product.toml`, read and checked into
//! Rust types at build time, without GPUI.

pub mod build_script;
mod config;
pub use config::*;
