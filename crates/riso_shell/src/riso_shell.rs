//! The window shell and extension component registry.

mod prompt;
pub use prompt::*;
mod shell;
pub use shell::*;

mod session;
pub use session::*;

mod window_bounds;
pub use window_bounds::*;

#[cfg(feature = "dev")]
pub mod dev;
