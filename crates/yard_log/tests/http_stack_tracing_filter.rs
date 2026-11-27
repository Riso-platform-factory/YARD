//! Native `tracing` events and spans below `WARN` from the HTTP stack are dropped
//! from the lifecycle trace (never emitted under their own name or renamed to
//! `HTTP stack warning`), `WARN` events produce the exact expected number of
//! `HTTP stack warning` lines in each sink, and target filtering matches by crate
//! name (`hyper` and `hyper::x` match, `hyperfoo` does not) in both `tracing` and `log`.
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
fn http_stack_native_tracing_and_crate_name_filtering() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: this file holds one test, and it has started no thread yet.
    unsafe {
        std::env::set_var("SAMPLE_LIFECYCLE", "trace");
        std::env::set_var("SAMPLE_LOG", "trace");
    }
    yard_log::start(yard_product::Product {
        name: "sample",
        version: "9.9.9",
        identifier: "com.example.sample",
    });

    let logs = home.path().join("logs");
    yard_log::open_files(&logs);

    // Native tracing events and spans from HTTP stack crates must be dropped below WARN.
    tracing::debug!(
        target: "hyper_util::client",
        "leaked-hyper-util-event /secret?token=1"
    );
    {
        let _span = tracing::debug_span!(
            target: "h2::proto",
            "h2-proto-debug-span",
            path = "/secret-span"
        )
        .entered();
        tracing::trace!(target: "h2::proto", "leaked-h2-trace");
    }
    {
        let _span = tracing::info_span!(
            target: "hyper::proto",
            "hyper-proto-info-span",
            path = "/secret-info-span"
        )
        .entered();
    }
    tracing::debug!(target: "hyper", "leaked-tracing-hyper-exact");
    tracing::debug!(target: "hyper::x", "leaked-tracing-hyper-sub");
    tracing::debug!(target: "hyper_rustls::connector", "leaked-tracing-hyper-rustls");
    tracing::debug!(target: "reqwest::connect", "leaked-tracing-reqwest");
    tracing::debug!(target: "zed_reqwest::connect", "leaked-tracing-zed-reqwest");
    tracing::debug!(target: "rustls::client", "leaked-tracing-rustls");

    // Known HTTP-stack WARN events (one native `tracing::warn!`, one `log::warn!`):
    // each must produce exactly one `HTTP stack warning` line in each sink, with no
    // original text or fields.
    tracing::warn!(
        target: "hyper_util::client",
        uri = "/secret-warn-field",
        "leaked-tracing-warn /secret-warn"
    );
    log::warn!(
        target: "reqwest::connect",
        "leaked-log-warn /secret-warn-log"
    );

    // Non-HTTP crate sharing a prefix (`hyperfoo`) must NOT be filtered in tracing.
    tracing::debug!(target: "hyperfoo", "hyperfoo-tracing-visible");
    tracing::debug!(target: "hyperfoo::sub", "hyperfoo-sub-tracing-visible");

    // `log` records also match by crate name: `hyper` and `hyper::x` match, `hyperfoo` does not.
    log::debug!(target: "hyper", "leaked-log-hyper-exact");
    log::debug!(target: "hyper::x", "leaked-log-hyper-sub");
    log::debug!(target: "hyper_util::client", "leaked-log-hyper-util");
    log::debug!(target: "hyper_rustls::connector", "leaked-log-hyper-rustls");
    log::debug!(target: "hyperfoo", "hyperfoo-log-visible");

    log::info!("http-stack-tracing-filter-done");

    let diagnostic_log = wait_for(&logs.join("sample.log"), "http-stack-tracing-filter-done");
    let lifecycle = wait_for(
        &logs.join("lifecycle.jsonl"),
        "http-stack-tracing-filter-done",
    );

    let records: Vec<Value> = lifecycle
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    // None of the HTTP stack debug/trace events, below-WARN spans, or WARN original texts
    // may reach the lifecycle trace.
    for dropped in [
        "leaked-hyper-util-event",
        "/secret?token=1",
        "h2-proto-debug-span",
        "/secret-span",
        "hyper-proto-info-span",
        "/secret-info-span",
        "leaked-h2-trace",
        "leaked-tracing-hyper-exact",
        "leaked-tracing-hyper-sub",
        "leaked-tracing-hyper-rustls",
        "leaked-tracing-reqwest",
        "leaked-tracing-zed-reqwest",
        "leaked-tracing-rustls",
        "leaked-tracing-warn",
        "/secret-warn",
        "leaked-log-warn",
        "leaked-log-hyper-exact",
        "leaked-log-hyper-sub",
        "leaked-log-hyper-util",
        "leaked-log-hyper-rustls",
    ] {
        assert!(
            !lifecycle.contains(dropped),
            "HTTP stack filtered/redacted item ({dropped}) must not reach lifecycle trace:\n{lifecycle}"
        );
    }

    // No `group-start` for any HTTP-stack span below WARN: not under its own name,
    // and not renamed to `HTTP stack warning` (or `HTTP stack error`).
    let group_starts: Vec<&str> = records
        .iter()
        .filter(|record| record["kind"] == "group-start")
        .filter_map(|record| record["name"].as_str())
        .collect();
    for forbidden_group in [
        "h2-proto-debug-span",
        "hyper-proto-info-span",
        "HTTP stack warning",
        "HTTP stack error",
    ] {
        assert!(
            !group_starts.contains(&forbidden_group),
            "HTTP-stack span below WARN must not emit a `group-start` named `{forbidden_group}` (got {group_starts:?}):\n{lifecycle}"
        );
    }
    assert_eq!(
        group_starts,
        vec!["app-startup", "log-open"],
        "unexpected `group-start` records in lifecycle trace:\n{lifecycle}"
    );

    // Exact count of `HTTP stack warning` lines in both sinks for the 2 emitted WARN events.
    let expected_warnings = 2;
    let lifecycle_warning_lines = records
        .iter()
        .filter(|record| {
            record["kind"] == "line"
                && record["level"] == "WARN"
                && record["message"] == "HTTP stack warning"
        })
        .count();
    assert_eq!(
        lifecycle_warning_lines, expected_warnings,
        "expected exactly {expected_warnings} `HTTP stack warning` line records in lifecycle trace:\n{lifecycle}"
    );
    let lifecycle_warning_occurrences = lifecycle
        .lines()
        .filter(|line| line.contains("HTTP stack warning"))
        .count();
    assert_eq!(
        lifecycle_warning_occurrences, expected_warnings,
        "expected exactly {expected_warnings} `HTTP stack warning` records total in lifecycle trace:\n{lifecycle}"
    );
    let diagnostic_warning_lines = diagnostic_log
        .lines()
        .filter(|line| line.contains("HTTP stack warning"))
        .count();
    assert_eq!(
        diagnostic_warning_lines, expected_warnings,
        "expected exactly {expected_warnings} `HTTP stack warning` lines in diagnostic log:\n{diagnostic_log}"
    );
    let diagnostic_warn_level_lines = diagnostic_log
        .lines()
        .filter(|line| line.contains("WARN") && line.contains("HTTP stack warning"))
        .count();
    assert_eq!(
        diagnostic_warn_level_lines, expected_warnings,
        "expected all {expected_warnings} `HTTP stack warning` lines in diagnostic log to be WARN:\n{diagnostic_log}"
    );

    // None of the HTTP stack `log::debug!` lines or WARN original texts may reach the diagnostic log.
    for dropped in [
        "leaked-log-hyper-exact",
        "leaked-log-hyper-sub",
        "leaked-log-hyper-util",
        "leaked-log-hyper-rustls",
        "leaked-tracing-warn",
        "/secret-warn",
        "leaked-log-warn",
    ] {
        assert!(
            !diagnostic_log.contains(dropped),
            "HTTP stack filtered/redacted item ({dropped}) must not reach diagnostic log:\n{diagnostic_log}"
        );
    }

    // `hyperfoo` (not the `hyper` crate) must reach the trace (and diagnostic log for `log::debug!`).
    for expected in [
        "hyperfoo-tracing-visible",
        "hyperfoo-sub-tracing-visible",
        "hyperfoo-log-visible",
    ] {
        assert!(
            records.iter().any(|record| {
                record["kind"] == "line"
                    && record["level"] == "DEBUG"
                    && record["message"]
                        .as_str()
                        .is_some_and(|m| m.contains(expected))
            }),
            "non-HTTP target `hyperfoo` ({expected}) must reach lifecycle trace:\n{lifecycle}"
        );
    }
    assert!(
        diagnostic_log.contains("hyperfoo-log-visible"),
        "non-HTTP target `hyperfoo` must reach diagnostic log:\n{diagnostic_log}"
    );
}
