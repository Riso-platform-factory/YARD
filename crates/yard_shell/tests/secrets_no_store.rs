#![cfg(feature = "dev")]
//! Calling `set_secret`, `get_secret`, or `delete_secret` before `set_secret_store`
//! returns `SecretError::Unavailable` and logs one diagnostic `ERROR` per application.
//!
//! Installs a process-wide diagnostic logger; alone in its binary.

use gpui::TestAppContext;
use std::sync::Mutex;
use yard_shell::{Extension, SecretError};

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn take_diagnostics() -> Vec<(log::Level, String)> {
    std::mem::take(&mut *DIAGNOSTICS.lock().unwrap_or_else(|e| e.into_inner()))
}

fn count_errors(records: &[(log::Level, String)]) -> usize {
    records
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .count()
}

fn assert_no_leak(records: &[(log::Level, String)], key: &str) {
    for (_, message) in records {
        assert!(
            !message.contains(key) && !message.contains("s3cr3t-value"),
            "diagnostic log must not contain key or value: {message}"
        );
    }
}

#[gpui::test]
async fn calls_without_secret_store_return_unavailable_with_one_error_per_app(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();

    let key = "user-token-xyz";
    let value = b"s3cr3t-value";

    let first_app = cx.new_app();
    let second_app = cx.new_app();
    let first_notes = first_app.update(|cx| Extension::new(cx, "notes").unwrap());
    let second_notes = second_app.update(|cx| Extension::new(cx, "notes").unwrap());
    let _ = take_diagnostics();

    // First application: first call logs one ERROR; follow-up calls log none.
    assert_eq!(
        first_app
            .read(|cx| first_notes.set_secret(key, value, cx))
            .await,
        Err(SecretError::Unavailable),
        "first app set_secret without store must return Unavailable"
    );
    let first_initial = take_diagnostics();
    assert_eq!(
        count_errors(&first_initial),
        1,
        "first call without secret store in first app must log one ERROR: {first_initial:#?}"
    );
    assert_no_leak(&first_initial, key);

    assert_eq!(
        first_app
            .read(|cx| first_notes.get_secret(key, cx))
            .await
            .err(),
        Some(SecretError::Unavailable),
        "first app get_secret without store must return Unavailable"
    );
    assert_eq!(
        first_app
            .read(|cx| first_notes.delete_secret(key, cx))
            .await,
        Err(SecretError::Unavailable),
        "first app delete_secret without store must return Unavailable"
    );
    let first_followup = take_diagnostics();
    assert_eq!(
        count_errors(&first_followup),
        0,
        "subsequent calls without secret store in the same app must log no ERROR: {first_followup:#?}"
    );
    assert_no_leak(&first_followup, key);

    // Second application: first call logs one ERROR of its own; follow-up calls log none.
    assert_eq!(
        second_app
            .read(|cx| second_notes.get_secret(key, cx))
            .await
            .err(),
        Some(SecretError::Unavailable),
        "second app get_secret without store must return Unavailable"
    );
    let second_initial = take_diagnostics();
    assert_eq!(
        count_errors(&second_initial),
        1,
        "first call without secret store in second app must log one ERROR: {second_initial:#?}"
    );
    assert_no_leak(&second_initial, key);

    assert_eq!(
        second_app
            .read(|cx| second_notes.set_secret(key, value, cx))
            .await,
        Err(SecretError::Unavailable),
        "second app set_secret without store must return Unavailable"
    );
    assert_eq!(
        second_app
            .read(|cx| second_notes.delete_secret(key, cx))
            .await,
        Err(SecretError::Unavailable),
        "second app delete_secret without store must return Unavailable"
    );
    let second_followup = take_diagnostics();
    assert_eq!(
        count_errors(&second_followup),
        0,
        "subsequent calls without secret store in second app must log no ERROR: {second_followup:#?}"
    );
    assert_no_leak(&second_followup, key);

    first_app.quit();
    first_app.run_until_parked();
    second_app.quit();
    second_app.run_until_parked();
}
