use std::{cell::Cell, fmt::Display};

/// One diagnostic per input until its text changes, with every occurrence
/// retained in the current run's lifecycle trace.
#[derive(Default)]
pub(super) struct Failure(Cell<bool>);

impl Failure {
    pub(super) fn changed(&self) {
        self.0.set(false);
    }

    pub(super) fn report(&self, operation: &str, reason: impl Display) {
        tracing::trace!(operation, %reason, "text input rendering failed");
        if !self.0.replace(true) {
            log::error!("could not {operation} text input: {reason}");
        }
    }
}
