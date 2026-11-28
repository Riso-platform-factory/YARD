//! The window shell and extension component registry.

mod autostart;
pub use autostart::*;

mod close_guard;
pub use close_guard::*;

mod data;
pub use data::*;

mod external_open;
pub use external_open::*;

mod file_dialog;
mod notification;

mod http;
pub use http::*;
pub use yard_http;

mod instance;
pub use instance::*;
mod prompt;
pub use prompt::*;

mod secrets;
pub use secrets::*;

mod shell;
pub use shell::*;

mod session;
pub use session::*;

mod tray;
pub use tray::*;

mod window_bounds;
pub use window_bounds::*;
