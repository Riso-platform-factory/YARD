#![cfg(feature = "dev")]

//! One test: installs the process-wide logger and subscriber.

use serde_json::Value;

#[test]
fn panic_on_start_thread_finishes_before_unwinding() {
    let home = tempfile::tempdir().unwrap();
    riso_log::start(riso_log::Product {
        name: "sample",
        version: "1",
    });
    riso_log::open_files(home.path());
    let outer = tracing::info_span!("outer-work");
    let _outer = outer.enter();
    let inner = tracing::info_span!("panic-work");
    let _inner = inner.enter();
    let result = std::panic::catch_unwind(|| panic!("start thread panic"));
    assert!(result.is_err());
    // The guards are still alive: finalization must happen in the hook.
    let text = std::fs::read_to_string(home.path().join("lifecycle.jsonl")).unwrap();
    let records: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.last().unwrap()["kind"], "run-end", "{text}");
    for name in ["outer-work", "panic-work", "app-startup"] {
        let start = records
            .iter()
            .find(|r| r["kind"] == "group-start" && r["name"] == name)
            .unwrap();
        let ends: Vec<_> = records
            .iter()
            .filter(|r| r["kind"] == "group-end" && r["id"] == start["id"])
            .collect();
        assert_eq!(ends.len(), 1, "{text}");
        assert_eq!(ends[0]["outcome"], "failure", "{text}");
    }
    // Other threads must not reopen a lifecycle that the hook has ended.
    assert!(
        std::thread::spawn(|| {
            let _group = tracing::info_span!("late-work").entered();
            riso_log::startup_finished();
            panic!("late worker panic"); // The hook also flushes queued records.
        })
        .join()
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(home.path().join("lifecycle.jsonl")).unwrap(),
        text
    );
    let diagnostic = std::fs::read_to_string(home.path().join("sample.log")).unwrap();
    assert!(diagnostic.contains("start thread panic"), "{diagnostic}");
}
