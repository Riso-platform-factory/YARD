//! whose every call returns `SecretError::Unavailable`, with one diagnostic `ERROR`,
//! without touching the real system credential store.
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

fn take_diagnostics() -> Vec<(log::Level, String)> {
    std::mem::take(&mut *DIAGNOSTICS.lock().unwrap_or_else(|e| e.into_inner()))
}

#[gpui::test]
async fn invalid_system_store_names_return_unavailable_with_one_error(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();

    let key = "user-token-xyz";
    let value = b"s3cr3t-value";

    let overlong_identifier = "a".repeat(1025);
    let invalid_cases = [
        ("", "Product"),
        ("a/b", "Product"),
        ("vn.example\nnotes", "Product"),
        (overlong_identifier.as_str(), "Product"),
        ("vn.example.notes", ""),
        ("vn.example.notes", "Bad\nProduct"),
    ];

    for (identifier, product_name) in invalid_cases {
        let _ = take_diagnostics();

        let app = cx.new_app();
        let store = SecretStore::system(identifier, product_name);
        let notes = app.update(|cx| {
            shell::set_secret_store(cx, store);
            Extension::new(cx, "notes").unwrap()
        });

        assert_eq!(
            app.read(|cx| notes.set_secret(key, value, cx)).await,
            Err(SecretError::Unavailable),
            "set_secret on invalid system store ({identifier:?}, {product_name:?}) must return Unavailable"
        );
        assert_eq!(
            app.read(|cx| notes.get_secret(key, cx)).await.err(),
            Some(SecretError::Unavailable),
            "get_secret on invalid system store ({identifier:?}, {product_name:?}) must return Unavailable"
        );
        assert_eq!(
            app.read(|cx| notes.delete_secret(key, cx)).await,
            Err(SecretError::Unavailable),
            "delete_secret on invalid system store ({identifier:?}, {product_name:?}) must return Unavailable"
        );

        let records = take_diagnostics();
        let errors: Vec<_> = records
            .iter()
            .filter(|(level, _)| *level == log::Level::Error)
            .collect();
        assert_eq!(
            errors.len(),
            1,
            "invalid system store ({identifier:?}, {product_name:?}) must log exactly one ERROR: {records:#?}"
        );
        for (_, message) in &records {
            assert!(
                !message.contains(key) && !message.contains("s3cr3t-value"),
                "diagnostic log must not contain key or value: {message}"
            );
        }

        app.quit();
        app.run_until_parked();
    }
}
