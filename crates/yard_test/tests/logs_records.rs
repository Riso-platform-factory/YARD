use std::panic::{AssertUnwindSafe, catch_unwind};
use yard_test::capture_logs;

fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("")
}

#[test]
fn capture_logs_records_diagnostics_only_while_active_and_keeps_tracing_separate() {
    log::warn!(target: "logs_records::outside", "logged before first capture");

    capture_logs(|trace| {
        log::warn!(target: "logs_records::diag", "disk almost full");
        tracing::info!(target: "logs_records::trace", "trace event stays out of log records");

        let records = trace.records();
        assert_eq!(
            records.len(),
            1,
            "only the log::warn! inside capture_logs is recorded as a DiagnosticRecord: {records:#?}"
        );
        assert_eq!(records[0].level, log::Level::Warn);
        assert_eq!(records[0].target, "logs_records::diag");
        assert_eq!(records[0].message, "disk almost full");

        assert!(
            trace.lines().iter().any(|line| {
                line.level == tracing::Level::INFO
                    && line.target == "logs_records::trace"
                    && line.message == "trace event stays out of log records"
            }),
            "tracing::info! is still captured as a TraceLine"
        );
        assert_eq!(trace.warnings(), vec!["disk almost full"]);
        assert!(trace.errors().is_empty());
        trace.assert_no_errors();
    });

    log::warn!(target: "logs_records::outside", "logged between captures");

    capture_logs(|trace| {
        assert!(
            trace.records().is_empty(),
            "a log::warn! outside any capture is not seen by the next capture"
        );

        log::error!(target: "logs_records::diag", "diagnostic error");
        let records = trace.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].level, log::Level::Error);
        assert_eq!(records[0].target, "logs_records::diag");
        assert_eq!(records[0].message, "diagnostic error");
        assert_eq!(trace.errors(), vec!["diagnostic error"]);

        let panicked = catch_unwind(AssertUnwindSafe(|| trace.assert_no_errors()))
            .expect_err("assert_no_errors must panic on a diagnostic error");
        assert!(
            !panic_text(&panicked).contains("not yet implemented"),
            "expected assert_no_errors panic, not todo!()"
        );
    });
}
