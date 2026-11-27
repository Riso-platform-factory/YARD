#![cfg(feature = "dev")]
//! Calling `set_secret_store` a second time is refused with one `ERROR`, and the
//! first store stays active.
//!
//! Installs a process-wide diagnostic logger; alone in its binary.

use gpui::TestAppContext;
use std::sync::Mutex;
use yard_shell::{self as shell, Extension, SecretError, SecretStore};

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

#[gpui::test]
async fn set_secret_store_twice_is_refused_with_one_error_and_keeps_first_store(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();

    let first = SecretStore::memory();
    let second = SecretStore::memory();
    let app = cx.new_app();
    let notes = app.update(|cx| {
        shell::set_secret_store(cx, first.clone());
        shell::set_secret_store(cx, second.clone());
        Extension::new(cx, "notes").unwrap()
    });

    let records = DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let errors: Vec<_> = records
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "calling set_secret_store twice must log exactly one ERROR: {records:#?}"
    );

    // The second store was refused, so arming it must not affect calls; arming the first must.
    second.fail_next(SecretError::Failed);
    app.read(|cx| notes.set_secret("api-token", b"kept-in-first", cx))
        .await
        .expect("second store must not be installed");

    first.fail_next(SecretError::Denied);
    assert_eq!(
        app.read(|cx| notes.get_secret("api-token", cx)).await.err(),
        Some(SecretError::Denied),
        "first store must stay active after a second set_secret_store call"
    );

    let loaded = app
        .read(|cx| notes.get_secret("api-token", cx))
        .await
        .unwrap()
        .expect("entry written through first store must be readable");
    assert_eq!(loaded.expose(), b"kept-in-first");

    app.quit();
    app.run_until_parked();
}
