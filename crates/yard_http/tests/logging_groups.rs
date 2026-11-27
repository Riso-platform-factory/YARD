//! Lifecycle trace groups for `yard_http`:
//! - `http-configure{proxy}` on start and reconfigure, with proxy credentials stripped;
//! - `http-request{caller, method, host}` with `status` and `elapsed_ms` at `debug`.
//!
//! Requests execute on the HTTP client's background runtime threads, so this binary installs
//! a process-wide tracing subscriber and runs this test alone.

use std::{
    cell::RefCell,
    collections::BTreeMap,
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
    http_client::{AsyncBody, HttpClient as _, StatusCode},
};

#[derive(Clone, Debug)]
struct SpanRecord {
    id: u64,
    name: &'static str,
    target: &'static str,
    level: tracing::Level,
    fields: String,
}

#[derive(Clone, Debug)]
struct EventRecord {
    span: Option<String>,
    level: tracing::Level,
    fields: String,
}

#[derive(Default)]
struct Captured {
    spans_by_id: BTreeMap<u64, SpanRecord>,
    yard_span_ids: Vec<u64>,
    events: Vec<EventRecord>,
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured {
    spans_by_id: BTreeMap::new(),
    yard_span_ids: Vec::new(),
    events: Vec::new(),
});

thread_local! {
    static ENTERED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
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
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
        let id = captured.spans_by_id.len() as u64 + 1;
        let record = SpanRecord {
            id,
            name: attributes.metadata().name(),
            target: attributes.metadata().target(),
            level: *attributes.metadata().level(),
            fields: fields.0,
        };
        if record.target.starts_with("yard_http") {
            captured.yard_span_ids.push(id);
        }
        captured.spans_by_id.insert(id, record);
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(span) = captured.spans_by_id.get_mut(&id.into_u64()) {
            span.fields.push_str(&fields.0);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("yard_http") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let active = ENTERED.with(|stack| stack.borrow().last().copied());
        let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
        let span = active.and_then(|id| {
            captured
                .spans_by_id
                .get(&id)
                .map(|s| format!("{}{}", s.name, s.fields))
        });
        captured.events.push(EventRecord {
            span,
            level: *event.metadata().level(),
            fields: fields.0,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        ENTERED.with(|stack| stack.borrow_mut().push(id.into_u64()));
    }

    fn exit(&self, _: &tracing::span::Id) {
        ENTERED.with(|stack| stack.borrow_mut().pop());
    }
}

fn take_captured() -> (Vec<SpanRecord>, Vec<EventRecord>) {
    let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
    let ids = std::mem::take(&mut captured.yard_span_ids);
    let spans = ids
        .into_iter()
        .filter_map(|id| captured.spans_by_id.get(&id).cloned())
        .collect();
    let events = std::mem::take(&mut captured.events);
    (spans, events)
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

struct SimpleServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SimpleServer {
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
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
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

impl Drop for SimpleServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn http_configure_and_http_request_groups() {
    tracing::subscriber::set_global_default(Trace).unwrap();

    let server = SimpleServer::start();

    // 1. Start with a proxy containing credentials: `http-configure` must log scheme, host,
    //    and port only, never `user:pass@`.
    let proxy_with_creds = "http://alice:super-secret-pass@127.0.0.1:18080".to_owned();
    let client = HttpClient::new(
        "GroupTest/1.0",
        &NetworkSettings {
            proxy: Some(proxy_with_creds),
        },
    )
    .unwrap();

    let (spans, _) = take_captured();
    let configures: Vec<_> = spans
        .iter()
        .filter(|s| s.name == "http-configure")
        .collect();
    assert_eq!(
        configures.len(),
        1,
        "expected one `http-configure` group on startup: {spans:#?}"
    );
    let cfg = configures[0];
    assert_eq!(cfg.level, tracing::Level::INFO);
    assert!(
        cfg.fields.contains("proxy=") && cfg.fields.contains("http://127.0.0.1:18080"),
        "`http-configure` missing proxy field: {cfg:?}"
    );
    assert!(
        !cfg.fields.contains("alice")
            && !cfg.fields.contains("super-secret-pass")
            && !cfg.fields.contains('@'),
        "`http-configure` must never log proxy credentials: {cfg:?}"
    );
    let _ = cfg.id;

    // 2. Reconfigure to no proxy: emits another `http-configure` group.
    client.reconfigure(&NetworkSettings { proxy: None });
    let (spans, _) = take_captured();
    let reconfigures: Vec<_> = spans
        .iter()
        .filter(|s| s.name == "http-configure")
        .collect();
    assert_eq!(
        reconfigures.len(),
        1,
        "expected one `http-configure` group on reconfigure: {spans:#?}"
    );
    assert!(
        reconfigures[0].fields.contains("proxy="),
        "`http-configure` on reconfigure missing proxy field: {:?}",
        reconfigures[0]
    );

    // 3. Request tagged with caller: emits `http-request{caller, method, host}` with
    //    `status` and `elapsed_ms` at `debug`.
    let tagged = client.for_caller("notes");
    let url = format!("http://{}/hello", server.addr);
    let response = block_on(tagged.get(&url, AsyncBody::empty(), true)).unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let (spans, events) = take_captured();
    let requests: Vec<_> = spans.iter().filter(|s| s.name == "http-request").collect();
    assert_eq!(
        requests.len(),
        1,
        "expected one `http-request` group: {spans:#?}"
    );
    let req_span = requests[0];
    assert_eq!(req_span.level, tracing::Level::INFO);
    assert!(
        req_span.fields.contains("caller=notes")
            && req_span.fields.contains("method=GET")
            && req_span.fields.contains("host=127.0.0.1"),
        "`http-request` missing caller, method, or host: {req_span:?}"
    );
    assert!(
        events.iter().any(|e| {
            e.span
                .as_deref()
                .is_some_and(|s| s.starts_with("http-request"))
                && e.level == tracing::Level::DEBUG
                && e.fields.contains("status=200")
                && e.fields.contains("elapsed_ms=")
        }),
        "expected DEBUG event with `status=200` and `elapsed_ms` inside `http-request`: {events:#?}"
    );
}
