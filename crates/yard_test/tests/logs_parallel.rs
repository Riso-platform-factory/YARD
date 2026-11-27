use std::sync::Once;
use yard_test::capture_trace;

static UNCAPTURED_PRIMED: Once = Once::new();

fn emit_shared(worker: &str) {
    tracing::info!(target: "logs_parallel::shared", worker, "shared callsite");
}

/// Ensures an uncaptured thread reaches `emit_shared` first, priming its
/// `tracing` callsite cache with no active subscriber before any capture runs.
fn prime_shared_callsite_uncaptured() {
    UNCAPTURED_PRIMED.call_once(|| {
        std::thread::spawn(|| {
            emit_shared("uncaptured");
        })
        .join()
        .expect("uncaptured thread joins");
    });
}

#[test]
fn uncaptured_thread_reaches_shared_callsite_first() {
    prime_shared_callsite_uncaptured();
}

macro_rules! parallel_capture_test {
    ($test_name:ident, $worker:literal, $own_message:literal) => {
        #[test]
        fn $test_name() {
            prime_shared_callsite_uncaptured();

            capture_trace(|trace| {
                tracing::info!(target: "logs_parallel::own", $own_message);
                std::thread::yield_now();
                emit_shared($worker);

                let own_lines: Vec<_> = trace
                    .lines()
                    .into_iter()
                    .filter(|line| line.target == "logs_parallel::own")
                    .collect();
                assert_eq!(
                    own_lines.len(),
                    1,
                    "expected own line for {} only, got: {own_lines:#?}",
                    $worker
                );
                assert_eq!(own_lines[0].message, $own_message);

                let shared_lines: Vec<_> = trace
                    .lines()
                    .into_iter()
                    .filter(|line| line.target == "logs_parallel::shared")
                    .collect();
                assert_eq!(
                    shared_lines.len(),
                    1,
                    "expected exactly one shared line for {}, got: {shared_lines:#?}",
                    $worker
                );
                assert_eq!(shared_lines[0].message, "shared callsite");
                assert_eq!(
                    shared_lines[0].fields.get("worker").map(String::as_str),
                    Some($worker),
                    "shared line belongs to this capture"
                );
            });
        }
    };
}

parallel_capture_test!(capture_worker_1, "worker-1", "own line 1");
parallel_capture_test!(capture_worker_2, "worker-2", "own line 2");
parallel_capture_test!(capture_worker_3, "worker-3", "own line 3");
parallel_capture_test!(capture_worker_4, "worker-4", "own line 4");
parallel_capture_test!(capture_worker_5, "worker-5", "own line 5");
parallel_capture_test!(capture_worker_6, "worker-6", "own line 6");
parallel_capture_test!(capture_worker_7, "worker-7", "own line 7");
parallel_capture_test!(capture_worker_8, "worker-8", "own line 8");
