#![cfg(feature = "dev")]

use riso_state::dev::{decode, encode};
use std::sync::{Arc, Mutex};
use tracing_subscriber::{Layer, layer::SubscriberExt, registry::LookupSpan};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<(&'static str, tracing::Level)>>>);

impl<S: tracing::Subscriber + for<'span> LookupSpan<'span>> Layer<S> for Capture {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if let Some(span) = context.event_span(event) {
            self.0
                .lock()
                .unwrap()
                .push((span.name(), *event.metadata().level()));
        }
    }
}

#[test]
fn format_operations_log_within_their_groups() {
    let capture = Capture::default();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()), || {
        assert_eq!(decode(&encode(&[])).unwrap().len(), 0);
    });
    assert_eq!(
        *capture.0.lock().unwrap(),
        [
            ("state-encode", tracing::Level::DEBUG),
            ("state-decode", tracing::Level::DEBUG),
        ]
    );
}
