#![cfg(feature = "dev")]

//! A SOCKS proxy host name is not resolved at configuration:
//! - `HttpClient::new` and `reconfigure` with `socks5://no-such-host.invalid:1080` return
//!   without a DNS failure;
//! - a request then fails with a proxy error;
//! - a later `reconfigure` to a working setting succeeds (the failure is not kept).
//!
//! Installs a process-wide logger and tracing subscriber; alone in its binary.

use std::{
    future::Future,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, JoinHandle},
    time::Duration,
};
use yard_http::{
    HttpClient, NetworkSettings,
    http_client::{AsyncBody, HttpClient as _, StatusCode},
    read_limited,
};

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static SPANS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
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

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        if attributes.metadata().target().starts_with("yard_http") {
            let mut fields = Fields::default();
            attributes.record(&mut fields);
            SPANS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((attributes.metadata().name().to_owned(), fields.0));
        }
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
    SPANS.lock().unwrap_or_else(|e| e.into_inner()).clear();
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

fn spans() -> Vec<(String, String)> {
    SPANS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn trace_events() -> Vec<(tracing::Level, String)> {
    TRACE_EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
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

struct OriginServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl OriginServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_thread = Arc::clone(&hits);
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
                let hits_req = Arc::clone(&hits_thread);
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
                    hits_req.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    );
                    let _ = stream.flush();
                });
            }
        });
        Self {
            addr,
            hits,
            stop,
            handle: Some(handle),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
}

impl Drop for OriginServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn socks_host_name_is_resolved_at_request_not_at_configuration() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    let origin = OriginServer::start();
    let bad_socks = NetworkSettings {
        proxy: Some("socks5://no-such-host.invalid:1080".to_owned()),
    };

    // 1. `HttpClient::new` returns at once without a DNS failure or warning.
    clear();
    let client = HttpClient::new("SocksDnsTest/1.0", &bad_socks).unwrap();
    let cfg_spans: Vec<_> = spans()
        .into_iter()
        .filter(|(name, _)| name == "http-configure")
        .collect();
    assert_eq!(cfg_spans.len(), 1);
    assert!(
        cfg_spans[0]
            .1
            .contains("proxy=socks5://no-such-host.invalid:1080"),
        "expected configured socks5 proxy in http-configure, got: {:?}",
        cfg_spans[0]
    );
    assert!(
        diagnostics().is_empty(),
        "HttpClient::new with SOCKS hostname must not resolve DNS or log WARN at configure: {:?}",
        diagnostics()
    );
    assert!(
        trace_events()
            .iter()
            .all(|(level, _)| *level != tracing::Level::WARN && *level != tracing::Level::ERROR),
        "HttpClient::new with SOCKS hostname must not emit WARN/ERROR at configure: {:?}",
        trace_events()
    );

    // 2. `reconfigure` with the same unresolvable SOCKS hostname also returns without a DNS failure.
    clear();
    client.reconfigure(&bad_socks);
    assert!(
        diagnostics().is_empty(),
        "reconfigure with SOCKS hostname must not resolve DNS or log WARN at configure: {:?}",
        diagnostics()
    );
    assert!(
        trace_events()
            .iter()
            .all(|(level, _)| *level != tracing::Level::WARN && *level != tracing::Level::ERROR),
        "reconfigure with SOCKS hostname must not emit WARN/ERROR at configure: {:?}",
        trace_events()
    );

    // 3. A request then fails with a proxy error and does not reach origin.
    clear();
    let result = block_on(client.get(
        &origin.url("/secret-path?token=abc"),
        AsyncBody::empty(),
        true,
    ));
    let err = result
        .map(|r| r.status())
        .expect_err("request through unresolvable SOCKS proxy must fail");
    let err_lower = err.to_string().to_ascii_lowercase();
    assert!(
        err_lower.contains("proxy"),
        "request through unresolvable SOCKS proxy must fail with a proxy error, got: {err}"
    );
    assert!(
        !err_lower.contains("secret-path") && !err_lower.contains("token=abc"),
        "proxy error must not leak request URL: {err}"
    );
    assert_eq!(
        origin.hits.load(Ordering::SeqCst),
        0,
        "failed SOCKS proxy resolution must not fall back to direct request"
    );

    // 4. A later `reconfigure` to a working setting succeeds: the failure is not kept.
    clear();
    client.reconfigure(&NetworkSettings { proxy: None });
    let ok = block_on(client.get(&origin.url("/recovered"), AsyncBody::empty(), true))
        .expect("reconfiguring to a working setting must succeed after SOCKS failure");
    assert_eq!(ok.status(), StatusCode::OK);
    let body = block_on(read_limited(ok, 1024)).unwrap();
    assert_eq!(body, b"ok");
    assert_eq!(origin.hits.load(Ordering::SeqCst), 1);
}
