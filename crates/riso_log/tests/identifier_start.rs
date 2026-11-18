//! The first lines of a run name the product's identifier: the lifecycle
//! trace's `run-start` holds `<name> <version> <identifier>`, and the
//! diagnostic log's first line of the run is
//! `<name> <version> (<identifier>) starting`.
//!
//! This file installs the process-wide logger and subscriber, so it holds one
//! test.

use riso_log::{open_files, start};
use riso_product::Product;
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
fn run_start_and_the_first_diagnostic_line_hold_the_identifier() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: this file holds one test, and it has started no thread yet.
    unsafe {
        // Set but empty is the same as not set: the filter is `info`.
        std::env::set_var("SAMPLE_LIFECYCLE", "");
        std::env::remove_var("SAMPLE_LOG");
    }
    start(Product::new("sample", "9.9.9", "com.example.sample"));
    let logs = home.path().join("logs");
    open_files(&logs);

    let diagnostic_log = wait_for(&logs.join("sample.log"), "starting");
    assert!(
        diagnostic_log
            .lines()
            .next()
            .is_some_and(|line| line.contains("sample 9.9.9 (com.example.sample) starting")),
        "{diagnostic_log}"
    );

    let lifecycle = wait_for(&logs.join("lifecycle.jsonl"), "run-start");
    let records: Vec<Value> = lifecycle
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records[0]["kind"], "run-start");
    assert_eq!(records[0]["name"], "sample");
    assert_eq!(records[0]["version"], "9.9.9");
    assert_eq!(records[0]["identifier"], "com.example.sample");
}
