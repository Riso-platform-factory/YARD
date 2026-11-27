//! `WARN` and `ERROR` records and `tracing` events from the HTTP stack are kept
//! with fixed text (`HTTP stack warning` / `HTTP stack error`) so paths and query
//! strings never leak into either log.
//!
//! This file installs the process-wide logger and subscriber, so it holds one test.

use serde_json::Value;
use std::{path::Path, time::Duration};

/// Waits until the file holds `text`, and returns its content.
fn wait_for(path: &Path, text: &str) -> String {
    for _ in 0..200 {
        if let Ok(content) = std::fs::read_to_string(path)
            && content.contains(text)
        {
            return content;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{} never held {text:?}", path.display());
}

#[test]
fn http_stack_warn_and_error_text_is_replaced_in_both_logs() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: this file holds one test, and it has started no thread yet.
    unsafe {
        std::env::set_var("SAMPLE_LIFECYCLE", "debug");
        std::env::set_var("SAMPLE_LOG", "debug");
    }
    yard_log::start(yard_product::Product {
        name: "sample",
        version: "9.9.9",
        identifier: "com.example.sample",
    });

    let logs = home.path().join("logs");
    yard_log::open_files(&logs);

    log::warn!(target: "hyper::proto", "bad request /secret?token=abc");
    tracing::warn!(
        target: "hyper::proto",
        uri = "/secret-field?token=field",
        "bad trace /secret-trace?token=xyz"
    );
    log::error!(target: "hyper::proto", "fatal request /secret-err?token=err");
    tracing::error!(
        target: "h2::proto",
        uri = "/secret-h2?token=h2",
        "fatal trace /secret-h2?token=h2"
    );
    log::warn!(target: "hyperfoo", "hyperfoo warning kept");
    log::info!("http-stack-redaction-done");

    let diagnostic_log = wait_for(&logs.join("sample.log"), "http-stack-redaction-done");
    let lifecycle = wait_for(&logs.join("lifecycle.jsonl"), "http-stack-redaction-done");

    // Original sensitive text must not appear in either log.
    for forbidden in [
        "/secret",
        "token",
        "bad request",
        "bad trace",
        "fatal request",
        "fatal trace",
    ] {
        assert!(
            !diagnostic_log.contains(forbidden),
            "HTTP stack message ({forbidden}) leaked into diagnostic log:\n{diagnostic_log}"
        );
        assert!(
            !lifecycle.contains(forbidden),
            "HTTP stack message ({forbidden}) leaked into lifecycle trace:\n{lifecycle}"
        );
    }

    // Diagnostic log keeps the fixed replacement text for WARN and ERROR.
    assert!(
        diagnostic_log
            .lines()
            .any(|line| line.contains("WARN") && line.contains("HTTP stack warning")),
        "HTTP stack WARN must reach diagnostic log as `HTTP stack warning`:\n{diagnostic_log}"
    );
    assert!(
        diagnostic_log
            .lines()
            .any(|line| line.contains("ERROR") && line.contains("HTTP stack error")),
        "HTTP stack ERROR must reach diagnostic log as `HTTP stack error`:\n{diagnostic_log}"
    );
    assert!(
        diagnostic_log.contains("hyperfoo warning kept"),
        "non-HTTP crate WARN must keep its original text in diagnostic log:\n{diagnostic_log}"
    );

    // Lifecycle trace keeps both `log` and native `tracing` WARN/ERROR with fixed replacement text.
    let records: Vec<Value> = lifecycle
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    let hyper_warnings = records
        .iter()
        .filter(|record| {
            record["kind"] == "line"
                && record["level"] == "WARN"
                && record["target"] == "hyper::proto"
                && record["message"] == "HTTP stack warning"
        })
        .count();
    assert!(
        hyper_warnings >= 2,
        "both `log::warn!` and `tracing::warn!` under `hyper::proto` must reach lifecycle trace as `HTTP stack warning` (found {hyper_warnings}):\n{lifecycle}"
    );

    assert!(
        records.iter().any(|record| {
            record["kind"] == "line"
                && record["level"] == "ERROR"
                && record["target"] == "hyper::proto"
                && record["message"] == "HTTP stack error"
        }),
        "`log::error!` under `hyper::proto` must reach lifecycle trace as `HTTP stack error`:\n{lifecycle}"
    );
    assert!(
        records.iter().any(|record| {
            record["kind"] == "line"
                && record["level"] == "ERROR"
                && record["target"] == "h2::proto"
                && record["message"] == "HTTP stack error"
        }),
        "`tracing::error!` under `h2::proto` must reach lifecycle trace as `HTTP stack error`:\n{lifecycle}"
    );
    assert!(
        records.iter().any(|record| {
            record["kind"] == "line"
                && record["level"] == "WARN"
                && record["target"] == "hyperfoo"
                && record["message"] == "hyperfoo warning kept"
        }),
        "non-HTTP crate WARN must keep its original text in lifecycle trace:\n{lifecycle}"
    );
}
