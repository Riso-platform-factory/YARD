use std::{any::Any, panic::catch_unwind, sync::Mutex};
use yard_test::capture_logs;

static OWN_LOG_MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static OWN_LOGGER: OwnLogger = OwnLogger;

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

struct OwnLogger;

impl log::Log for OwnLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.target() == "logs_capture_logs_subscriber_conflict::own" {
            OWN_LOG_MESSAGES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

fn recorded_own_logs() -> Vec<String> {
    OWN_LOG_MESSAGES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn panic_text(payload: &Box<dyn Any + Send>) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("")
}

#[test]
fn existing_global_tracing_subscriber_fails_capture_logs_without_taking_logger_slot() {
    tracing::subscriber::set_global_default(ExistingSubscriber)
        .expect("install existing global tracing subscriber");

    let first = catch_unwind(|| capture_logs(|_| {}))
        .expect_err("first capture_logs must panic when a global tracing subscriber exists");
    let first_message = panic_text(&first).to_owned();
    assert!(
        first_message.to_lowercase().contains("subscriber"),
        "first capture_logs panic must name the existing subscriber, got: {first_message:?}"
    );

    let second = catch_unwind(|| capture_logs(|_| {}))
        .expect_err("second capture_logs must also panic when a global tracing subscriber exists");
    let second_message = panic_text(&second).to_owned();
    assert!(
        !second_message.to_lowercase().contains("poisoned"),
        "second capture_logs must not panic with a poisoned Once, got: {second_message:?}"
    );
    assert_eq!(
        second_message, first_message,
        "second capture_logs must repeat the same installation failure message"
    );

    log::set_logger(&OWN_LOGGER)
        .expect("failed capture_logs must not take the global log logger slot");
    log::set_max_level(log::LevelFilter::Warn);
    log::warn!(
        target: "logs_capture_logs_subscriber_conflict::own",
        "own logger works"
    );
    assert_eq!(
        recorded_own_logs(),
        vec!["own logger works"],
        "the test's own logger must receive log records after taking the untouched slot"
    );
}
