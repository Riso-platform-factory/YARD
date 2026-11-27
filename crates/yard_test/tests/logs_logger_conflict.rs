#![cfg(feature = "dev")]

use std::{any::Any, panic::catch_unwind, sync::Mutex};
use yard_test::{capture_logs, capture_trace};

static DIRECT_LOG_MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static EXISTING_LOGGER: ExistingLogger = ExistingLogger;

struct ExistingLogger;

impl log::Log for ExistingLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.target() == "logs_logger_conflict::direct" {
            DIRECT_LOG_MESSAGES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

fn recorded_direct_logs() -> Vec<String> {
    DIRECT_LOG_MESSAGES
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
fn existing_logger_fails_capture_logs_consistently_and_preserves_direct_logs_and_capture_trace() {
    log::set_logger(&EXISTING_LOGGER).expect("install existing log logger");
    log::set_max_level(log::LevelFilter::Warn);

    log::warn!(target: "logs_logger_conflict::direct", "before conflict");
    assert_eq!(recorded_direct_logs(), vec!["before conflict"]);

    let first = catch_unwind(|| capture_logs(|_| {}))
        .expect_err("first capture_logs must panic when a log logger is already installed");
    let first_message = panic_text(&first).to_owned();
    assert!(
        first_message.to_lowercase().contains("logger"),
        "first capture_logs panic must name the existing logger, got: {first_message:?}"
    );
    assert!(
        !tracing_core::dispatcher::has_been_set(),
        "failed capture_logs must fail before publishing the global tracing router"
    );

    let second = catch_unwind(|| capture_logs(|_| {}))
        .expect_err("second capture_logs must also panic when a log logger is already installed");
    let second_message = panic_text(&second).to_owned();
    assert!(
        !second_message.to_lowercase().contains("poisoned"),
        "second capture_logs must not panic with a poisoned Once, got: {second_message:?}"
    );
    assert_eq!(
        second_message, first_message,
        "second capture_logs must repeat the same installation failure message"
    );
    assert!(
        !tracing_core::dispatcher::has_been_set(),
        "repeated failed capture_logs must not publish the global tracing router"
    );

    assert_eq!(
        log::max_level(),
        log::LevelFilter::Warn,
        "failed capture_logs must leave log::max_level() unchanged"
    );
    log::warn!(target: "logs_logger_conflict::direct", "after conflict");
    assert_eq!(
        recorded_direct_logs(),
        vec!["before conflict", "after conflict"],
        "existing logger still receives direct log records after failed capture_logs"
    );

    capture_trace(|trace| {
        tracing::info!(
            target: "logs_logger_conflict::trace",
            "trace still works after logger conflict"
        );
        let messages: Vec<_> = trace
            .lines()
            .into_iter()
            .filter(|line| line.target == "logs_logger_conflict::trace")
            .map(|line| line.message)
            .collect();
        assert_eq!(
            messages,
            vec!["trace still works after logger conflict"],
            "tracing lines still reach a later capture_trace in a binary with an existing logger"
        );
    });

    log::warn!(target: "logs_logger_conflict::direct", "after capture_trace");
    assert_eq!(
        recorded_direct_logs(),
        vec!["before conflict", "after conflict", "after capture_trace"],
        "existing logger still receives direct log records after capture_trace"
    );
}
