#![cfg(feature = "dev")]

//! A panicking closure must not leak SQL text or values through the process-wide
//! panic hook's log output.
//!
//! Installs a process-wide `log` logger and a process-wide panic hook; alone in its binary.

use std::sync::Mutex;
use yard_data::{DataError, Database};

static RECORDED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        RECORDED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("{}: {}", record.level(), record.args()));
    }

    fn flush(&self) {}
}

#[test]
fn panicking_closure_leaks_no_sql_or_value_to_panic_hook() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    // Mirror `yard_log`'s panic hook (`diagnostic_log.rs`): record `{panic}` via `log::error!`.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        log::error!("{panic}");
        previous(panic);
    }));

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    let db = Database::new(file);
    db.migrate(
        "a",
        &[r#"CREATE TABLE "a.t" (id INTEGER PRIMARY KEY, body TEXT NOT NULL)"#],
    )
    .wait()
    .unwrap();

    let result = db
        .write::<()>("a", |db| {
            db.execute(
                r#"INSERT INTO "a.t" (body) VALUES ('my-diary-text') BAD"#,
                [],
            )
            .unwrap();
            Ok(())
        })
        .wait();
    assert!(
        matches!(result, Err(DataError::Panicked)),
        "expected DataError::Panicked, got {result:?}"
    );
    db.flush();

    let entries = RECORDED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for secret in ["my-diary-text", "INSERT"] {
        assert!(
            entries.iter().all(|entry| !entry.contains(secret)),
            "panic output leaked {secret:?}: {entries:#?}"
        );
    }
    assert!(
        entries
            .iter()
            .any(|entry| entry.contains("database request panicked")),
        "expected recorded output to state that a database request panicked: {entries:#?}"
    );
}
