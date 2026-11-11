#![cfg(feature = "dev")]

//! Both logs from start to end: what is logged before the application's directories are
//! known reaches both files in order, `tracing` stays out of `sample.log`, the
//! files open once, and a panic is logged.
//!
//! This file installs the process-wide logger and subscriber, so it holds one
//! test.

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
fn early_lines_reach_both_files_in_order_and_the_files_open_once() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: this file holds one test, and it has started no thread yet.
    unsafe {
        // Set but empty is the same as not set: the filter is `info`.
        std::env::set_var("SAMPLE_LIFECYCLE", "");
        std::env::remove_var("SAMPLE_LOG");
    }
    riso_log::start(riso_log::Product {
        name: "sample",
        version: "9.9.9",
    });
    log::info!("early one");
    log::warn!("early two");
    tracing::info!("trace only");
    tracing::debug!("below the filter");

    let logs = home.path().join("logs");
    riso_log::open_files(&logs);
    log::info!("after open");
    let other = home.path().join("other");
    riso_log::open_files(&other);
    assert!(!other.exists());
    let _ = std::thread::spawn(|| panic!("boom")).join();

    let diagnostic_log = wait_for(&logs.join("sample.log"), "boom");
    let order: Vec<_> = diagnostic_log
        .lines()
        .filter_map(|line| {
            [
                "sample 9.9.9 starting",
                "early one",
                "early two",
                "after open",
                "already open",
                "boom",
            ]
            .into_iter()
            .find(|text| line.contains(text))
        })
        .collect();
    assert_eq!(
        order,
        [
            "sample 9.9.9 starting",
            "early one",
            "early two",
            "after open",
            "already open",
            "boom"
        ],
        "{diagnostic_log}"
    );
    assert!(
        diagnostic_log
            .lines()
            .any(|line| line.contains("WARN") && line.contains("early two"))
    );
    assert!(!diagnostic_log.contains("trace only"), "{diagnostic_log}");

    let lifecycle = wait_for(&logs.join("lifecycle.jsonl"), "boom");
    let records: Vec<Value> = lifecycle
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records[0]["kind"], "run-start");
    assert_eq!(records[0]["version"], "9.9.9");
    assert_eq!(records[0]["name"], "sample");
    let message = |text: &str| {
        records
            .iter()
            .find(|record| {
                record["kind"] == "line"
                    && record["message"].as_str().is_some_and(|m| m.contains(text))
            })
            .unwrap_or_else(|| panic!("no line {text:?} in {lifecycle}"))
    };
    assert_eq!(message("early one")["level"], "INFO");
    assert_eq!(message("early two")["level"], "WARN");
    assert_eq!(message("trace only")["level"], "INFO");
    assert_eq!(message("already open")["level"], "WARN");
    assert_eq!(message("boom")["level"], "ERROR");
    assert!(!lifecycle.contains("below the filter"), "{lifecycle}");
    assert!(
        records
            .iter()
            .any(|record| record["kind"] == "group-start" && record["name"] == "log-open"),
        "{lifecycle}"
    );
}
