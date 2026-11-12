//! The application's config, data, and logs directories.

mod directories;
mod layout_file;
mod settings_file;
mod write_atomic;
pub use directories::*;
pub use write_atomic::*;
