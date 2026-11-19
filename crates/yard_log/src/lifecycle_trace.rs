//! The lifecycle trace: groups that cover app-startup, app-running, and
//! app-shutdown with nested tracing spans, their records, the thread that
//! writes them, and the list of known errors.

mod known_errors;
mod lifecycle;
mod record;
pub(crate) mod writer;

pub(crate) use record::{Outcome, Record};

pub(crate) use known_errors::KNOWN_ERRORS;
pub(crate) use lifecycle::{Lifecycle, LifecycleLayer, Phase};
pub(crate) use writer::Message;
