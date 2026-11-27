//! Contract point 8 of `extension-toggle`: `Extension::new(cx, "extensions")`
//! returns `None` and logs an `ERROR`, as `yard` does.
//!
//! Installs a process-wide diagnostic logger, so this test is alone in its file.

use gpui::TestAppContext;
use std::sync::{Mutex, Once};

static INIT: Once = Once::new();
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            RECORDS
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

#[gpui::test]
fn extension_named_extensions_is_refused_with_an_error(cx: &mut TestAppContext) {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    RECORDS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clear();

    cx.update(|cx| {
        assert!(
            yard_shell::Extension::new(cx, "extensions").is_none(),
            "`extensions` is a reserved settings key and must be refused by Extension::new"
        );
    });

    let records = RECORDS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    let errors: Vec<&str> = records
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message.as_str())
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one diagnostic ERROR when refusing `extensions`: {records:#?}"
    );
    assert!(
        errors[0].contains("extensions"),
        "diagnostic ERROR must mention `extensions`: {}",
        errors[0]
    );
}
