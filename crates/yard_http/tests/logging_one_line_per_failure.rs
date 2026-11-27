#![cfg(feature = "dev")]

//! A failure is logged once through `log` (and reaches the lifecycle trace through the
//! bridge): a refused connection produces exactly one `WARN` in the captured trace, not two.
//!
//! Installs a process-wide logger bridge and tracing subscriber; alone in its binary.

use std::{
    future::Future,
    net::TcpListener,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
    thread,
};
use yard_http::{
    HttpClient, NetworkSettings,
    http_client::{AsyncBody, HttpClient as _},
};

const PROBE: &str = "yard_http_one_line_probe";

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static TRACE_EVENTS: Mutex<Vec<(tracing::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_http")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        DIAGNOSTICS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((record.level(), record.args().to_string()));
        match record.level() {
            log::Level::Error => tracing::error!(target: PROBE, "{}", record.args()),
            log::Level::Warn => tracing::warn!(target: PROBE, "{}", record.args()),
            log::Level::Info => tracing::info!(target: PROBE, "{}", record.args()),
            log::Level::Debug => tracing::debug!(target: PROBE, "{}", record.args()),
            log::Level::Trace => tracing::trace!(target: PROBE, "{}", record.args()),
        }
    }

    fn flush(&self) {}
}

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let rendered = format!("{value:?}").replace('"', "");
        write!(self.0, " {}={rendered}", field.name()).unwrap();
    }
}

struct Trace;

impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::always()
    }

    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !target.starts_with("yard_http") && target != PROBE {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        TRACE_EVENTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((*event.metadata().level(), fields.0));
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

#[test]
fn refused_connection_produces_one_warn_in_captured_trace() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    let client = HttpClient::new("OneLineTest/1.0", &NetworkSettings::default()).unwrap();

    let unused = TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_addr = unused.local_addr().unwrap();
    drop(unused);

    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    TRACE_EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();

    let url = format!("http://{closed_addr}/unreachable");
    let result = block_on(client.get(&url, AsyncBody::empty(), true));
    assert!(result.is_err(), "refused connection must fail");

    let trace_warns: Vec<_> = TRACE_EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(level, _)| *level == tracing::Level::WARN)
        .cloned()
        .collect();
    assert_eq!(
        trace_warns.len(),
        1,
        "refused connection must emit exactly one WARN in the captured trace, not two: {trace_warns:#?}"
    );

    let diag_warns: Vec<_> = DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .cloned()
        .collect();
    assert_eq!(
        diag_warns.len(),
        1,
        "refused connection must emit exactly one diagnostic WARN: {diag_warns:#?}"
    );
}
