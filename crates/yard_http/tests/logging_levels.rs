#![cfg(feature = "dev")]

//! Diagnostic and lifecycle trace levels for `yard_http`:
//! - 404 response -> `debug`, NOT `WARN` or `ERROR`;
//! - refused connection -> `WARN`, NOT `ERROR`;
//! - invalid proxy -> `WARN`, NOT `ERROR`;
//! - refused scheme -> `info`, NOT `WARN` or `ERROR`;
//! - size limit exceeded (`read_limited`) -> `WARN`, NOT `ERROR`.
//!
//! Installs a process-wide `log` logger and a process-wide `tracing` subscriber; alone in its binary.

use std::{
    future::Future,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, JoinHandle},
    time::Duration,
};
use yard_http::{
    HttpClient, NetworkSettings,
    http_client::{AsyncBody, HttpClient as _, Request, Response, StatusCode},
    read_limited,
};

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static TRACE_EVENTS: Mutex<Vec<(tracing::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_http")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((record.level(), record.args().to_string()));
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
        if !event.metadata().target().starts_with("yard_http") {
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

fn clear() {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    TRACE_EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

fn diagnostics() -> Vec<(log::Level, String)> {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn trace_events() -> Vec<(tracing::Level, String)> {
    TRACE_EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn diag_warnings() -> Vec<String> {
    diagnostics()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, msg)| msg)
        .collect()
}

fn diag_errors() -> Vec<String> {
    diagnostics()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, msg)| msg)
        .collect()
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

struct NotFoundServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl NotFoundServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            for incoming in listener.incoming() {
                if stop_thread.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(mut stream) = incoming else {
                    break;
                };
                if stop_thread.load(Ordering::Relaxed) {
                    break;
                }
                thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 512];
                    loop {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => {
                                buf.extend_from_slice(&chunk[..n]);
                                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                    break;
                                }
                            }
                        }
                    }
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
                    );
                    let _ = stream.flush();
                });
            }
        });
        Self {
            addr,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for NotFoundServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn logging_levels_match_contract() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    let server = NotFoundServer::start();
    let client = HttpClient::new("LevelsTest/1.0", &NetworkSettings::default()).unwrap();

    // 1. A 404 is `debug`, NOT `WARN` or `ERROR`.
    {
        clear();
        let url = format!("http://{}/missing", server.addr);
        let response = block_on(client.get(&url, AsyncBody::empty(), true)).unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        assert!(
            diag_warnings().is_empty(),
            "404 must not log diagnostic WARN: {:?}",
            diagnostics()
        );
        assert!(
            diag_errors().is_empty(),
            "404 must not log diagnostic ERROR: {:?}",
            diagnostics()
        );
        let events = trace_events();
        assert!(
            events
                .iter()
                .any(|(level, fields)| *level == tracing::Level::DEBUG
                    && fields.contains("status=404")),
            "404 must log status=404 at debug: {events:#?}"
        );
        assert!(
            events
                .iter()
                .all(|(level, _)| *level != tracing::Level::WARN && *level != tracing::Level::ERROR),
            "404 must not emit WARN or ERROR in trace: {events:#?}"
        );
    }

    // 2. A refused connection is `WARN`, NOT `ERROR`.
    {
        let unused = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_addr = unused.local_addr().unwrap();
        drop(unused);

        clear();
        let url = format!("http://{closed_addr}/unreachable");
        let result = block_on(client.get(&url, AsyncBody::empty(), true));
        assert!(result.is_err(), "refused connection must return Err");

        assert!(
            !diag_warnings().is_empty(),
            "refused connection must log diagnostic WARN: {:?} (trace: {:?})",
            diagnostics(),
            trace_events()
        );
        assert!(
            diag_errors().is_empty(),
            "refused connection must not log diagnostic ERROR: {:?}",
            diagnostics()
        );
        assert!(
            trace_events()
                .iter()
                .all(|(level, _)| *level != tracing::Level::ERROR),
            "refused connection must not log ERROR in trace: {:?}",
            trace_events()
        );
    }

    // 3. An invalid proxy (`"not a url"`) fails closed with a `WARN`, NOT `ERROR`.
    {
        clear();
        let bad = HttpClient::new(
            "InvalidProxyLevel/1.0",
            &NetworkSettings {
                proxy: Some("not a url".to_owned()),
            },
        )
        .unwrap();
        let url = format!("http://{}/blocked", server.addr);
        let result = block_on(bad.get(&url, AsyncBody::empty(), true));
        assert!(result.is_err(), "invalid proxy must refuse requests");

        assert!(
            !diag_warnings().is_empty(),
            "invalid proxy must log diagnostic WARN: {:?} (trace: {:?})",
            diagnostics(),
            trace_events()
        );
        assert!(
            diag_errors().is_empty(),
            "invalid proxy must not log diagnostic ERROR: {:?}",
            diagnostics()
        );
        assert!(
            trace_events()
                .iter()
                .all(|(level, _)| *level != tracing::Level::ERROR),
            "invalid proxy must not log ERROR in trace: {:?}",
            trace_events()
        );
    }

    // 4. A refused scheme is `info`, NOT `WARN` or `ERROR`.
    {
        clear();
        let req = Request::builder()
            .uri("ftp://127.0.0.1/file")
            .body(AsyncBody::empty())
            .unwrap();
        let result = block_on(client.send(req));
        assert!(result.is_err(), "ftp:// must be refused");

        assert!(
            diag_warnings().is_empty(),
            "refused scheme must not log diagnostic WARN: {:?}",
            diagnostics()
        );
        assert!(
            diag_errors().is_empty(),
            "refused scheme must not log diagnostic ERROR: {:?}",
            diagnostics()
        );
        let events = trace_events();
        assert!(
            events
                .iter()
                .any(|(level, _)| *level == tracing::Level::INFO),
            "refused scheme must log at info: {events:#?}"
        );
        assert!(
            events
                .iter()
                .all(|(level, _)| *level != tracing::Level::WARN && *level != tracing::Level::ERROR),
            "refused scheme must not log WARN or ERROR in trace: {events:#?}"
        );
    }

    // 5. A body over the `read_limited` limit logs `WARN`, NOT `ERROR`.
    {
        clear();
        let big = Response::builder()
            .status(StatusCode::OK)
            .body(AsyncBody::from(vec![b'z'; 32]))
            .unwrap();
        let result = block_on(read_limited(big, 16));
        assert!(result.is_err(), "body over limit must be refused");

        assert!(
            !diag_warnings().is_empty(),
            "size failure must log diagnostic WARN: {:?} (trace: {:?})",
            diagnostics(),
            trace_events()
        );
        assert!(
            diag_errors().is_empty(),
            "size failure must not log diagnostic ERROR: {:?}",
            diagnostics()
        );
    }
}
