//! The window shell and extension component registry.

#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub use dev::*;

mod file_dialog;
mod notification;

mod instance;
pub use instance::*;
mod prompt;
pub use prompt::*;
mod shell;
pub use shell::*;

mod session;
pub use session::*;

mod window_bounds;
pub use window_bounds::*;
