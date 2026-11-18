mod shell;
pub use shell::*;

// Compile the shared session source against this module's shell registry.
#[allow(clippy::duplicate_mod)]
#[path = "session.rs"]
mod session;
pub use session::*;

pub use crate::{saved_window_bounds, track_window_bounds};
