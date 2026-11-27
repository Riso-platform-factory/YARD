//! After `set_http_client`, `Extension::http_client` performs requests through the shared
//! client and tags each request with the extension's name in `http-request{caller, method, host}`.
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
        atomic::{AtomicBool, Ordering},
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
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LocalServer {
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
                        b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nshell-ok",
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
async fn extension_http_client_works_and_tags_caller_with_extension_name(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    tracing::subscriber::set_global_default(Trace).unwrap();

    let server = LocalServer::start();
    let app = cx.new_app();
    let (notes, reader) = app.update(move |cx| {
        let shared = HttpClient::new("ShellTest/1.0", &NetworkSettings::default()).unwrap();
        shell::set_http_client(cx, shared);
        let notes = shell::Extension::new(cx, "notes").unwrap();
        let reader = shell::Extension::new(cx, "reader").unwrap();
        (notes, reader)
    });

    let notes_http = app.read(move |cx| notes.http_client(cx));
    let reader_http = app.read(move |cx| reader.http_client(cx));
    let url = format!("http://{}/items", server.addr);

    let resp = notes_http
        .get(&url, AsyncBody::empty(), true)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = read_limited(resp, 1024).await.unwrap();
    assert_eq!(body, b"shell-ok");

    let resp2 = reader_http
        .get(&url, AsyncBody::empty(), true)
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = read_limited(resp2, 1024).await.unwrap();
    assert_eq!(body2, b"shell-ok");

    let spans = take_spans();
    assert!(
        spans
            .iter()
            .any(|s| s.name == "http-request" && s.fields.contains("caller=notes")),
        "expected `http-request` tagged with `caller=notes`: {spans:#?}"
    );
    assert!(
        spans
            .iter()
            .any(|s| s.name == "http-request" && s.fields.contains("caller=reader")),
        "expected `http-request` tagged with `caller=reader`: {spans:#?}"
    );

    app.quit();
    app.run_until_parked();
}
