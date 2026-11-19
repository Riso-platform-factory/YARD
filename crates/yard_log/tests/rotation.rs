//! One test owns the process-wide logger and lifecycle writer.

use serde_json::Value;
use std::time::Duration;

#[test]
fn opening_a_large_log_reports_the_actual_rotation_in_log_open() {
    let logs = tempfile::tempdir().unwrap();
    let diagnostic = logs.path().join("rotation.log");
    let rotated = logs.path().join("rotation.log.old");
    let previous = vec![b'x'; 1024 * 1024];
    std::fs::write(&diagnostic, &previous).unwrap();
    std::fs::write(&rotated, b"older run").unwrap();
    // SAFETY: this binary holds one test and has started no threads yet.
    unsafe { std::env::set_var("ROTATION_LIFECYCLE", "debug") };
    yard_log::start(yard_product::Product {
        name: "rotation",
        version: "0.0.0",
        identifier: "com.example.rotation",
    });
    yard_log::open_files(logs.path());
    assert_eq!(std::fs::read(&rotated).unwrap(), previous);
    assert!(std::fs::metadata(&diagnostic).unwrap().len() < previous.len() as u64);

    let path = logs.path().join("lifecycle.jsonl");
    let mut content = String::new();
    for _ in 0..200 {
        content = std::fs::read_to_string(&path).unwrap_or_default();
        if content.contains("rotated previous diagnostic log") && content.contains("group-end") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let records: Vec<Value> = content
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let group = records
        .iter()
        .find(|r| r["kind"] == "group-start" && r["name"] == "log-open")
        .unwrap();
    let line = records
        .iter()
        .find(|r| r["message"] == "rotated previous diagnostic log")
        .unwrap_or_else(|| panic!("no rotation line: {content}"));
    assert_eq!(line["level"], "DEBUG");
    assert_eq!(line["group"], group["id"]);
    assert_eq!(line["fields"]["path"], rotated.display().to_string());
    assert_eq!(line["fields"]["bytes"], previous.len() as u64);
}
