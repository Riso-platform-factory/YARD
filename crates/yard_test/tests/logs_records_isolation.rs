use std::{sync::mpsc, thread};
use yard_test::capture_logs;

#[test]
fn capture_logs_ignores_other_threads_and_restores_max_level() {
    let initial_level = log::max_level();

    capture_logs(|trace| {
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            log::error!(
                target: "logs_records_isolation::other",
                "error from uncaptured thread"
            );
            tx.send(()).expect("signal after uncaptured log::error!");
        });

        rx.recv()
            .expect("wait for uncaptured thread to log::error!");
        handle.join().expect("uncaptured thread joins");

        log::warn!(
            target: "logs_records_isolation::own",
            "warning from capturing thread"
        );

        let records = trace.records();
        assert_eq!(
            records.len(),
            1,
            "uncaptured thread's log::error! must not appear in records(): {records:#?}"
        );
        assert_eq!(records[0].level, log::Level::Warn);
        assert_eq!(records[0].target, "logs_records_isolation::own");
        assert_eq!(records[0].message, "warning from capturing thread");
        assert!(
            trace.errors().is_empty(),
            "uncaptured thread's log::error! must not appear in errors(): {:?}",
            trace.errors()
        );
        trace.assert_no_errors();
    });

    assert_eq!(
        log::max_level(),
        initial_level,
        "capture_logs restores log::max_level() to what it was before"
    );

    log::set_max_level(log::LevelFilter::Warn);
    capture_logs(|trace| {
        log::debug!(
            target: "logs_records_isolation::own",
            "debug captured while max level is raised"
        );
        assert!(
            trace
                .records()
                .iter()
                .any(|record| record.level == log::Level::Debug
                    && record.message == "debug captured while max level is raised"),
            "capture_logs raises log::max_level() while active"
        );
    });

    assert_eq!(
        log::max_level(),
        log::LevelFilter::Warn,
        "capture_logs restores a previously configured log::max_level()"
    );
}
