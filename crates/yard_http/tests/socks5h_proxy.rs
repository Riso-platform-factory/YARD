#![cfg(feature = "dev")]

//! `socks5h://` is accepted as a `network.proxy` scheme: the proxy URL parses,
//! the client is configured without warnings, credentials are stripped from
//! `http-configure{proxy}`, and requests connect to the configured proxy endpoint.
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
    http_client::{AsyncBody, HttpClient as _},
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

struct AcceptCounter {
    addr: SocketAddr,
    accepted: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AcceptCounter {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepted_thread = Arc::clone(&accepted);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            for incoming in listener.incoming() {
                if stop_thread.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = incoming else {
                    break;
                };
                if stop_thread.load(Ordering::Relaxed) {
                    break;
                }
                accepted_thread.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        Self {
            addr,
            accepted,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for AcceptCounter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
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
fn socks5h_proxy_scheme_is_accepted_and_configured() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    // 1. `HttpClient::new` accepts `socks5h://` without warnings and logs scheme://host:port.
    clear();
    let client = HttpClient::new(
        "Socks5hTest/1.0",
        &NetworkSettings {
            proxy: Some("socks5h://127.0.0.1:1080".to_owned()),
        },
    )
    .unwrap();

    let cfg_spans: Vec<_> = spans()
        .into_iter()
        .filter(|(name, _)| name == "http-configure")
        .collect();
    assert_eq!(
        cfg_spans.len(),
        1,
        "expected one http-configure: {cfg_spans:#?}"
    );
    assert!(
        cfg_spans[0].1.contains("proxy=socks5h://127.0.0.1:1080"),
        "socks5h:// must parse and appear in http-configure, got: {:?}",
        cfg_spans[0]
    );
    assert!(
        diagnostics().is_empty(),
        "valid socks5h:// must not log diagnostic warnings/errors: {:?}",
        diagnostics()
    );
    assert!(
        trace_events()
            .iter()
            .all(|(level, _)| *level != tracing::Level::WARN && *level != tracing::Level::ERROR),
        "valid socks5h:// must not emit WARN/ERROR during configure: {:?}",
        trace_events()
    );

    // 2. Reconfigure with credentials strips credentials from `http-configure{proxy}`.
    clear();
    client.reconfigure(&NetworkSettings {
        proxy: Some("socks5h://alice:top-secret@127.0.0.1:1081".to_owned()),
    });
    let cfg_spans: Vec<_> = spans()
        .into_iter()
        .filter(|(name, _)| name == "http-configure")
        .collect();
    assert_eq!(cfg_spans.len(), 1);
    assert!(
        cfg_spans[0].1.contains("proxy=socks5h://127.0.0.1:1081"),
        "socks5h:// with credentials must log scheme://host:port only: {:?}",
        cfg_spans[0]
    );
    assert!(
        !cfg_spans[0].1.contains("alice")
            && !cfg_spans[0].1.contains("top-secret")
            && !cfg_spans[0].1.contains('@'),
        "socks5h:// credentials must not appear in http-configure: {:?}",
        cfg_spans[0]
    );
    assert!(
        diagnostics().is_empty(),
        "reconfiguring to valid socks5h:// must not log warnings/errors: {:?}",
        diagnostics()
    );

    // 3. With a local TCP listener as the `socks5h://` target, a request attempts to connect
    //    to the proxy listener (proving the transport is configured) and never hits origin.
    let fake_socks = AcceptCounter::start();
    let origin = OriginServer::start();
    clear();
    let socks_url = format!("socks5h://{}", fake_socks.addr);
    client.reconfigure(&NetworkSettings {
        proxy: Some(socks_url.clone()),
    });
    assert!(
        diagnostics().is_empty(),
        "configuring socks5h:// local listener must not log warnings: {:?}",
        diagnostics()
    );

    let url = format!("http://{}/check", origin.addr);
    let result = block_on(client.get(&url, AsyncBody::empty(), true));
    assert!(
        result.is_err(),
        "request through closed non-SOCKS listener must fail"
    );
    assert!(
        fake_socks.accepted.load(Ordering::SeqCst) >= 1,
        "client configured with {socks_url} must connect to the socks5h proxy endpoint"
    );
    assert_eq!(
        origin.hits.load(Ordering::SeqCst),
        0,
        "client configured with socks5h:// must not bypass proxy and hit origin"
    );
}
