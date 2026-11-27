//! Calling `read_data` or `write_data` when no data file was set returns `DataError::Unavailable`
//! and logs one diagnostic `ERROR`.
//!
//! Installs a process-wide diagnostic logger; alone in its binary.

use gpui::TestAppContext;
use std::sync::Mutex;
use yard_shell::{self as shell, DataError};

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell") || metadata.target().starts_with("yard_data")
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
async fn request_without_data_file_returns_unavailable_and_logs_one_error(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();

    let app = cx.new_app();
    let notes = app.update(|cx| shell::Extension::new(cx, "notes").unwrap());

    let first = app.read(|cx| notes.read_data(cx, |_| Ok(()))).await;
    assert!(
        matches!(first, Err(DataError::Unavailable)),
        "expected Unavailable when no data file is set, got {first:?}"
    );

    let second = app.read(|cx| notes.write_data(cx, |_| Ok(()))).await;
    assert!(
        matches!(second, Err(DataError::Unavailable)),
        "expected Unavailable on second request, got {second:?}"
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
        "requests with no data file set must log exactly one ERROR: {records:#?}"
    );
}
