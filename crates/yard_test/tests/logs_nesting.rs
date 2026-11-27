use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::mpsc,
    thread,
    time::Duration,
};
use yard_test::{capture_logs, capture_trace};

const DEADLOCK_TIMEOUT: Duration = Duration::from_secs(2);

fn panic_text(payload: &Box<dyn Any + Send>) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("")
}

fn run_with_timeout(label: &str, f: impl FnOnce() + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(f));
        let _ = tx.send(outcome);
    });
    match rx.recv_timeout(DEADLOCK_TIMEOUT) {
        Ok(Ok(())) => {}
        Ok(Err(payload)) => resume_unwind(payload),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("{label}: timed out after {DEADLOCK_TIMEOUT:?} (deadlock instead of panicking)");
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{label}: worker thread exited unexpectedly");
        }
    }
}

#[test]
fn nested_capture_trace_panics_immediately_and_following_capture_works() {
    run_with_timeout("capture_trace inside capture_trace", || {
        let panicked = catch_unwind(|| {
            capture_trace(|_| {
                capture_trace(|_| {});
            });
        })
        .expect_err("capture_trace inside capture_trace on the same thread must panic");
        let message = panic_text(&panicked);
        assert!(
            !message.is_empty(),
            "nested capture_trace must panic with a message"
        );

        capture_trace(|trace| {
            tracing::info!(target: "logs_nesting::trace", "after nested capture_trace");
            let messages: Vec<_> = trace.lines().into_iter().map(|line| line.message).collect();
            assert_eq!(
                messages,
                vec!["after nested capture_trace"],
                "a capture after a nested-capture panic still works"
            );
        });
    });
}

#[test]
fn capture_logs_inside_capture_trace_panics_immediately_and_following_capture_works() {
    run_with_timeout("capture_logs inside capture_trace", || {
        let panicked = catch_unwind(|| {
            capture_trace(|_| {
                capture_logs(|_| {});
            });
        })
        .expect_err("capture_logs inside capture_trace on the same thread must panic");
        let message = panic_text(&panicked);
        assert!(
            !message.is_empty(),
            "capture_logs inside capture_trace must panic with a message"
        );

        capture_logs(|trace| {
            log::warn!(target: "logs_nesting::diag", "after nested capture_logs");
            assert_eq!(
                trace.warnings(),
                vec!["after nested capture_logs"],
                "a capture_logs after a nested-capture panic still works"
            );
        });
    });
}

struct ScopedSubscriber;

impl tracing::Subscriber for ScopedSubscriber {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, _: &tracing::Event<'_>) {}

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn capture_trace_inside_with_default_panics_and_following_capture_works() {
    let panicked = catch_unwind(|| {
        tracing::subscriber::with_default(ScopedSubscriber, || {
            capture_trace(|_| {
                tracing::info!(
                    target: "logs_nesting::scoped",
                    "should not run inside a scoped subscriber"
                );
            });
        });
    })
    .expect_err("capture_trace inside tracing::subscriber::with_default must panic");
    let message = panic_text(&panicked);
    assert!(
        !message.is_empty(),
        "capture_trace inside tracing::subscriber::with_default must panic with a message"
    );

    capture_trace(|trace| {
        tracing::info!(
            target: "logs_nesting::scoped",
            "after scoped subscriber panic"
        );
        let messages: Vec<_> = trace
            .lines()
            .into_iter()
            .filter(|line| line.target == "logs_nesting::scoped")
            .map(|line| line.message)
            .collect();
        assert_eq!(
            messages,
            vec!["after scoped subscriber panic"],
            "a capture outside tracing::subscriber::with_default works after the panic"
        );
    });
}
