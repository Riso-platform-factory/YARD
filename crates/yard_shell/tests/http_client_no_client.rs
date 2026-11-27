//! Calling `Extension::http_client` before `set_http_client` returns a client that refuses
//! every request and logs one diagnostic `ERROR`.
//!
//! Installs a process-wide diagnostic logger; alone in its binary.

use gpui::TestAppContext;
use std::sync::Mutex;
use yard_shell::{
    self as shell,
    yard_http::http_client::{AsyncBody, HttpClient as _},
};

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell") || metadata.target().starts_with("yard_http")
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
async fn http_client_before_set_refuses_requests_with_one_error(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();

    let app = cx.new_app();
    let notes = app.update(|cx| shell::Extension::new(cx, "notes").unwrap());

    let notes_a = notes.clone();
    let client_a = app.read(move |cx| notes_a.http_client(cx));
    let first = client_a
        .get("http://127.0.0.1:1/never", AsyncBody::empty(), true)
        .await;
    assert!(
        first.is_err(),
        "request before set_http_client must be refused, got {:?}",
        first.ok().map(|r| r.status())
    );
    let second = client_a
        .get(
            "http://127.0.0.1:1/never-same-handle",
            AsyncBody::empty(),
            true,
        )
        .await;
    assert!(
        second.is_err(),
        "second request on same handle before set_http_client must be refused, got {:?}",
        second.ok().map(|r| r.status())
    );

    let client_b = app.read(move |cx| notes.http_client(cx));
    let third = client_b
        .get("http://127.0.0.1:1/never-again", AsyncBody::empty(), true)
        .await;
    assert!(
        third.is_err(),
        "request on second handle before set_http_client must also be refused, got {:?}",
        third.ok().map(|r| r.status())
    );

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
        "requests with no HTTP client set must log exactly one ERROR: {records:#?}"
    );

    app.quit();
    app.run_until_parked();
}
