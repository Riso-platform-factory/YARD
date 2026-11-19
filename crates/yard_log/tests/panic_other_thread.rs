//! One test: installs the process-wide logger and subscriber across threads.

#[test]
fn panic_on_other_thread_leaves_run_open() {
    let home = tempfile::tempdir().unwrap();
    yard_log::start(yard_product::Product {
        name: "sample",
        version: "1",
        identifier: "com.example.sample",
    });
    yard_log::open_files(home.path());
    assert!(
        std::thread::spawn(|| {
            let _group = tracing::info_span!("worker-work").entered();
            panic!("worker panic");
        })
        .join()
        .is_err()
    );
    let text = std::fs::read_to_string(home.path().join("lifecycle.jsonl")).unwrap();
    assert!(text.contains("worker panic"), "{text}");
    assert!(!text.contains("run-end"), "{text}");
    assert!(!text.contains("interrupted"), "{text}");
    let diagnostic = std::fs::read_to_string(home.path().join("sample.log")).unwrap();
    assert!(diagnostic.contains("worker panic"), "{diagnostic}");
}
