//! An `Extension::http_client` handle taken before `set_http_client` refuses requests until
//! the client is installed, then succeeds and keeps the extension's caller tag in
//! `http-request{caller, method, host}`.
//!
//! Installs a process-wide tracing subscriber because requests run on background threads;
//! alone in its binary.

use gpui::TestAppContext;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use yard_shell::{
    self as shell,
    yard_http::{
        HttpClient, NetworkSettings,
        http_client::{AsyncBody, HttpClient as _, StatusCode},
        read_limited,
    },
};

#[derive(Clone, Debug)]
struct SpanRecord {
    name: &'static str,
    fields: String,
}

#[derive(Default)]
struct Captured {
    spans_by_id: BTreeMap<u64, SpanRecord>,
    yard_span_ids: Vec<u64>,
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured {
    spans_by_id: BTreeMap::new(),
    yard_span_ids: Vec::new(),
});

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
            name: attributes.metadata().name(),
            fields: fields.0,
        };
        if attributes.metadata().target().starts_with("yard_http") {
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
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn take_spans() -> Vec<SpanRecord> {
    let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
    let ids = std::mem::take(&mut captured.yard_span_ids);
    ids.into_iter()
        .filter_map(|id| captured.spans_by_id.get(&id).cloned())
        .collect()
}

struct LocalServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LocalServer {
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
                        b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nshell-ok",
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

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[gpui::test]
async fn early_handle_works_once_http_client_is_set(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    tracing::subscriber::set_global_default(Trace).unwrap();

    let server = LocalServer::start();
    let app = cx.new_app();
    let notes = app.update(|cx| shell::Extension::new(cx, "notes").unwrap());

    // Take the handle before `set_http_client` is called: its first request must fail.
    let early = app.read(move |cx| notes.http_client(cx));
    let url = format!("http://{}/early", server.addr);
    let before = early.get(&url, AsyncBody::empty(), true).await;
    assert!(
        before.is_err(),
        "request on early handle before set_http_client must fail, got {:?}",
        before.ok().map(|r| r.status())
    );
    assert_eq!(
        server.hits.load(Ordering::SeqCst),
        0,
        "failed request before set_http_client must not hit the server"
    );

    // Install the shared client; the same handle's next request must now succeed.
    app.update(|cx| {
        let shared = HttpClient::new("ShellEarlyTest/1.0", &NetworkSettings::default()).unwrap();
        shell::set_http_client(cx, shared);
    });
    let _ = take_spans();

    let after = early
        .get(&url, AsyncBody::empty(), true)
        .await
        .expect("early handle taken before set_http_client must work once the client is set");
    assert_eq!(after.status(), StatusCode::OK);
    let body = read_limited(after, 1024).await.unwrap();
    assert_eq!(body, b"shell-ok");
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);

    let spans = take_spans();
    assert!(
        spans
            .iter()
            .any(|s| s.name == "http-request" && s.fields.contains("caller=notes")),
        "early handle must keep extension caller tag after set_http_client: {spans:#?}"
    );

    app.quit();
    app.run_until_parked();
}
