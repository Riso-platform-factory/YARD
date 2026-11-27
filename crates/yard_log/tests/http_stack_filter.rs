//! Records below `WARN` from the HTTP stack (`reqwest`, `hyper`, `hyper_util`, `h2`,
//! `rustls`) are dropped from both the diagnostic log and the lifecycle trace, even
//! when debug logging is enabled. A `WARN` from `reqwest` still reaches both as
//! `HTTP stack warning`, without its original text.
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
fn http_stack_debug_lines_are_dropped_while_reqwest_warn_reaches_both_logs() {
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

    log::debug!("app debug visible");
    log::debug!(
        target: "reqwest::async_impl::client",
        "redirect policy disallowed redirection to 'http://h/secret?token=abc'"
    );
    log::debug!(target: "hyper", "hyper debug line leaked-hyper");
    log::debug!(target: "hyper_util", "hyper_util debug line leaked-hyper-util");
    log::debug!(target: "h2", "h2 debug line leaked-h2");
    log::debug!(target: "rustls", "rustls debug line leaked-rustls");
    log::warn!(target: "reqwest", "reqwest warning reached");
    log::info!("http-stack-filter-done");

    let diagnostic_log = wait_for(&logs.join("sample.log"), "http-stack-filter-done");
    let lifecycle = wait_for(&logs.join("lifecycle.jsonl"), "http-stack-filter-done");

    // Sanity check: debug logging really is enabled in both logs.
    assert!(
        diagnostic_log.contains("app debug visible"),
        "expected debug logging to be enabled in diagnostic log: {diagnostic_log}"
    );
    assert!(
        lifecycle.contains("app debug visible"),
        "expected debug logging to be enabled in lifecycle trace: {lifecycle}"
    );

    // Every HTTP-stack debug line must be dropped from both logs.
    for dropped in [
        "redirect policy disallowed redirection to 'http://h/secret?token=abc'",
        "http://h/secret?token=abc",
        "leaked-hyper",
        "leaked-hyper-util",
        "leaked-h2",
        "leaked-rustls",
    ] {
        assert!(
            !diagnostic_log.contains(dropped),
            "HTTP stack debug line ({dropped}) must not reach diagnostic log:\n{diagnostic_log}"
        );
        assert!(
            !lifecycle.contains(dropped),
            "HTTP stack debug line ({dropped}) must not reach lifecycle trace:\n{lifecycle}"
        );
    }

    // A WARN from `reqwest` must reach both logs as `HTTP stack warning`,
    // without its original text.
    assert!(
        !diagnostic_log.contains("reqwest warning reached"),
        "reqwest WARN original text must not reach diagnostic log:\n{diagnostic_log}"
    );
    assert!(
        !lifecycle.contains("reqwest warning reached"),
        "reqwest WARN original text must not reach lifecycle trace:\n{lifecycle}"
    );
    assert!(
        diagnostic_log
            .lines()
            .any(|line| line.contains("WARN") && line.contains("HTTP stack warning")),
        "reqwest WARN must reach diagnostic log as `HTTP stack warning`:\n{diagnostic_log}"
    );
    let records: Vec<Value> = lifecycle
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        records.iter().any(|record| {
            record["kind"] == "line"
                && record["level"] == "WARN"
                && record["target"] == "reqwest"
                && record["message"] == "HTTP stack warning"
        }),
        "reqwest WARN must reach lifecycle trace as `HTTP stack warning`:\n{lifecycle}"
    );
}
