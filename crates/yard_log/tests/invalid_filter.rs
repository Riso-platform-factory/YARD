//! An invalid `SAMPLE_LIFECYCLE` filter is a warning in the diagnostic log, and
//! the trace uses `info`.
//!
//! This file installs the process-wide logger and subscriber, so it holds one
//! test.

use std::time::Duration;

#[test]
fn an_invalid_filter_is_a_warning_and_info_is_used() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: this file holds one test, and it has started no thread yet.
    unsafe { std::env::set_var("SAMPLE_LIFECYCLE", "info,[") };
    yard_log::start(yard_product::Product {
        name: "sample",
        version: "9.9.9",
        identifier: "com.example.sample",
    });
    tracing::info!("at info");
    tracing::debug!("below info");
    let logs = home.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let previous = vec![b'x'; 1024 * 1024];
    std::fs::write(logs.join("sample.log"), &previous).unwrap();
    yard_log::open_files(&logs);
    assert_eq!(
        std::fs::read(logs.join("sample.log.old")).unwrap(),
        previous
    );

    let read = |name: &str, text: &str| {
        let path = logs.join(name);
        for _ in 0..200 {
            if let Ok(content) = std::fs::read_to_string(&path)
                && content.contains(text)
            {
                return content;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{name} never held {text:?}");
    };
    let diagnostic_log = read("sample.log", "invalid SAMPLE_LIFECYCLE filter");
    assert!(
        diagnostic_log
            .lines()
            .any(|line| line.contains("WARN") && line.contains("invalid SAMPLE_LIFECYCLE filter")),
        "{diagnostic_log}"
    );
    let lifecycle = read("lifecycle.jsonl", "at info");
    assert!(lifecycle.contains("at info"), "{lifecycle}");
    assert!(!lifecycle.contains("below info"), "{lifecycle}");
}
