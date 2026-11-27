use std::panic::catch_unwind;
use yard_test::{capture_logs, capture_trace};

fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("")
}

#[test]
fn capture_recovers_after_a_panicking_closure() {
    let trace_panic = catch_unwind(|| {
        capture_trace(|_trace| {
            tracing::info!(target: "logs_poison::trace", "before trace panic");
            panic!("simulated trace capture panic");
        });
    })
    .expect_err("panic inside capture_trace propagates");
    assert!(
        panic_text(&trace_panic).contains("simulated trace capture panic"),
        "expected closure panic, got: {:?}",
        panic_text(&trace_panic)
    );

    capture_trace(|trace| {
        tracing::info!(target: "logs_poison::trace", "after trace panic");
        let messages: Vec<_> = trace.lines().into_iter().map(|line| line.message).collect();
        assert_eq!(
            messages,
            vec!["after trace panic"],
            "capture_trace still works after a poisoned lock and drops aborted lines"
        );
    });

    let logs_panic = catch_unwind(|| {
        capture_logs(|_trace| {
            log::warn!(target: "logs_poison::diag", "before logs panic");
            panic!("simulated logs capture panic");
        });
    })
    .expect_err("panic inside capture_logs propagates");
    assert!(
        panic_text(&logs_panic).contains("simulated logs capture panic"),
        "expected closure panic, got: {:?}",
        panic_text(&logs_panic)
    );

    capture_logs(|trace| {
        log::warn!(target: "logs_poison::diag", "after logs panic");
        let messages: Vec<_> = trace
            .records()
            .into_iter()
            .map(|record| record.message)
            .collect();
        assert_eq!(
            messages,
            vec!["after logs panic"],
            "capture_logs still works after a poisoned lock and drops aborted records"
        );
    });
}
