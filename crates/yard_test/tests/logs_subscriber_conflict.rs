use std::{any::Any, panic::catch_unwind};
use yard_test::capture_trace;

struct ExistingSubscriber;

impl tracing::Subscriber for ExistingSubscriber {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, _: &tracing::Event<'_>) {}

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

fn panic_text(payload: &Box<dyn Any + Send>) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("")
}

#[test]
fn existing_global_tracing_subscriber_fails_consistently_without_poisoning() {
    tracing::subscriber::set_global_default(ExistingSubscriber)
        .expect("install existing global tracing subscriber");

    let first = catch_unwind(|| capture_trace(|_| {}))
        .expect_err("first capture_trace must panic when a global tracing subscriber exists");
    let first_message = panic_text(&first).to_owned();
    assert!(
        first_message.to_lowercase().contains("subscriber"),
        "first capture_trace panic must name the existing subscriber, got: {first_message:?}"
    );

    let second = catch_unwind(|| capture_trace(|_| {}))
        .expect_err("second capture_trace must also panic when a global tracing subscriber exists");
    let second_message = panic_text(&second).to_owned();
    assert!(
        !second_message.to_lowercase().contains("poisoned"),
        "second capture_trace must not panic with a poisoned Once, got: {second_message:?}"
    );
    assert_eq!(
        second_message, first_message,
        "second capture_trace must repeat the same installation failure message"
    );
}
