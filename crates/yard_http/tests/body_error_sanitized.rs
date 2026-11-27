//! Body read errors are sanitized: when a server sends headers and closes mid-body,
//! both `read_limited` and a caller streaming the body directly get an error with no
//! URL, path, or query, and exactly one `WARN` is logged (in diagnostics and in the
//! bridged lifecycle trace).
//!
//! Installs a process-wide logger bridge and tracing subscriber; alone in its binary.

use std::{
    error::Error as _,
    future::{Future, poll_fn},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, JoinHandle},
    time::Duration,
};
use yard_http::{
    HttpClient, NetworkSettings,
    http_client::{AsyncBody, HttpClient as _, Inner, Response, StatusCode},
    read_limited,
};

const PROBE: &str = "yard_http_body_error_probe";
const SECRET_PATH: &str = "secret-body-path";
const SECRET_QUERY: &str = "token=body-secret-999";

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

fn diag_warnings() -> Vec<String> {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, msg)| msg.clone())
        .collect()
}

fn diag_errors() -> Vec<String> {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, msg)| msg.clone())
        .collect()
}

fn trace_warnings() -> Vec<String> {
    TRACE_EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(level, _)| *level == tracing::Level::WARN)
        .map(|(_, fields)| fields.clone())
        .collect()
}

fn trace_errors() -> Vec<String> {
    TRACE_EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(level, _)| *level == tracing::Level::ERROR)
        .map(|(_, fields)| fields.clone())
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

/// Sends headers declaring a 1024-byte body, waits for a signal, sends a few bytes,
/// and drops the connection mid-body.
struct TruncatedBodyServer {
    addr: SocketAddr,
    release: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TruncatedBodyServer {
    fn start() -> (Self, mpsc::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel::<()>();
        let release = Arc::new(Mutex::new(Some(rx)));
        let release_thread = Arc::clone(&release);
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
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = Vec::new();
                let mut chunk = [0u8; 512];
                let got_headers = loop {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => break false,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break true;
                            }
                        }
                    }
                };
                if !got_headers {
                    continue;
                }
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n",
                );
                let _ = stream.flush();
                if let Some(rx) = release_thread.lock().unwrap().as_ref() {
                    let _ = rx.recv_timeout(Duration::from_secs(5));
                }
                let _ = stream.write_all(b"partial-body");
                let _ = stream.flush();
                drop(stream);
            }
        });
        (
            Self {
                addr,
                release,
                stop,
                handle: Some(handle),
            },
            tx,
        )
    }
}

impl Drop for TruncatedBodyServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        *self.release.lock().unwrap() = None;
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

async fn stream_body_directly(response: Response<AsyncBody>) -> std::io::Result<Vec<u8>> {
    let mut body = response.into_body().0;
    let mut out = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        let read = match &mut body {
            Inner::Empty => return Ok(out),
            Inner::Bytes(cursor) => cursor.read(&mut buf)?,
            Inner::AsyncReader(reader) => {
                let mut pinned = reader.as_mut();
                let slice = &mut buf[..];
                poll_fn(move |cx| pinned.as_mut().poll_read(cx, slice)).await?
            }
        };
        if read == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..read]);
    }
}

fn assert_no_url_leak(context: &str, text: &str, addr: SocketAddr) {
    let lower = text.to_ascii_lowercase();
    for forbidden in [
        "http://",
        &addr.to_string(),
        SECRET_PATH,
        SECRET_QUERY,
        "body-secret-999",
    ] {
        assert!(
            !lower.contains(&forbidden.to_ascii_lowercase()),
            "{context} leaked {forbidden:?}: {text:?}"
        );
    }
}

#[test]
fn truncated_body_errors_are_sanitized_with_one_warn_for_read_limited_and_streaming() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    let (server, release) = TruncatedBodyServer::start();
    let client = HttpClient::new("BodyErrorTest/1.0", &NetworkSettings::default()).unwrap();
    let url = format!("http://{}/{SECRET_PATH}?{SECRET_QUERY}", server.addr);

    // 1. `read_limited` on a truncated body: sanitized error and exactly one WARN.
    {
        clear();
        let response = block_on(client.get(&url, AsyncBody::empty(), true)).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        release.send(()).unwrap();

        let err = block_on(read_limited(response, 4096))
            .expect_err("mid-body close must fail in read_limited");
        let rendered = format!("{err:#}");
        assert_no_url_leak("read_limited error", &rendered, server.addr);

        let diag_warns = diag_warnings();
        assert_eq!(
            diag_warns.len(),
            1,
            "read_limited body error must log exactly one diagnostic WARN: {diag_warns:#?}"
        );
        assert_no_url_leak("read_limited diagnostic WARN", &diag_warns[0], server.addr);
        assert!(
            diag_errors().is_empty(),
            "read_limited body error must not log ERROR: {:?}",
            diag_errors()
        );

        let trace_warns = trace_warnings();
        assert_eq!(
            trace_warns.len(),
            1,
            "read_limited body error must produce exactly one WARN in trace: {trace_warns:#?}"
        );
        assert_no_url_leak("read_limited trace WARN", &trace_warns[0], server.addr);
        assert!(
            trace_errors().is_empty(),
            "read_limited body error must not emit ERROR in trace: {:?}",
            trace_errors()
        );
    }

    // 2. Streaming the body directly on a truncated body: sanitized error and exactly one WARN.
    {
        clear();
        let response = block_on(client.get(&url, AsyncBody::empty(), true)).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        release.send(()).unwrap();

        let err = block_on(stream_body_directly(response))
            .expect_err("mid-body close must fail when streaming the body directly");
        let mut full_chain = format!("{err} | {err:?}");
        let mut current: Option<&dyn std::error::Error> = err.source();
        while let Some(source) = current {
            full_chain.push_str(&format!(" | {source}"));
            current = source.source();
        }
        assert_no_url_leak("direct body stream error", &full_chain, server.addr);

        let diag_warns = diag_warnings();
        assert_eq!(
            diag_warns.len(),
            1,
            "direct body stream error must log exactly one diagnostic WARN: {diag_warns:#?}"
        );
        assert_no_url_leak(
            "direct body stream diagnostic WARN",
            &diag_warns[0],
            server.addr,
        );
        assert!(
            diag_errors().is_empty(),
            "direct body stream error must not log ERROR: {:?}",
            diag_errors()
        );

        let trace_warns = trace_warnings();
        assert_eq!(
            trace_warns.len(),
            1,
            "direct body stream error must produce exactly one WARN in trace: {trace_warns:#?}"
        );
        assert_no_url_leak(
            "direct body stream trace WARN",
            &trace_warns[0],
            server.addr,
        );
        assert!(
            trace_errors().is_empty(),
            "direct body stream error must not emit ERROR in trace: {:?}",
            trace_errors()
        );
    }
}
