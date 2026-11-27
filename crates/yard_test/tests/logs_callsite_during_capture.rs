use std::{sync::mpsc, thread};
use yard_test::capture_trace;

fn emit_fresh(source: &str) {
    tracing::info!(
        target: "logs_callsite_during_capture::shared",
        source,
        "fresh callsite"
    );
}

#[test]
fn callsite_first_reached_by_uncaptured_thread_during_capture_is_seen() {
    capture_trace(|trace| {
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            emit_fresh("uncaptured");
            tx.send(()).expect("signal after uncaptured callsite");
        });

        rx.recv()
            .expect("wait for uncaptured thread to reach the callsite first");
        emit_fresh("captured");
        handle.join().expect("uncaptured thread joins");

        let shared_lines: Vec<_> = trace
            .lines()
            .into_iter()
            .filter(|line| line.target == "logs_callsite_during_capture::shared")
            .collect();
        assert_eq!(
            shared_lines.len(),
            1,
            "capture must see the capturing thread's line (and not the uncaptured thread's): {shared_lines:#?}"
        );
        assert_eq!(shared_lines[0].message, "fresh callsite");
        assert_eq!(
            shared_lines[0].fields.get("source").map(String::as_str),
            Some("captured"),
            "captured line must come from the capturing thread"
        );
    });
}
