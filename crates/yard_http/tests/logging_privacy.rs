//! Neither the lifecycle trace nor the diagnostic log may record request paths, queries,
//! headers, bodies, or proxy credentials.
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
    http_client::{AsyncBody, HttpClient as _, Method, Request, Response, StatusCode},
    read_limited,
};

const SECRET_PATH: &str = "secret-path";
const SECRET_QUERY: &str = "token=abc";
const SECRET_REQ_HEADER_NAME: &str = "x-yard-secret-req";
const SECRET_REQ_HEADER_VAL: &str = "req-header-secret-33c";
const SECRET_REQ_BODY: &str = "req-body-secret-44d";
const SECRET_RESP_HEADER_NAME: &str = "x-yard-secret-resp";
const SECRET_RESP_HEADER_VAL: &str = "resp-header-secret-55e";
const SECRET_RESP_BODY: &str = "resp-body-secret-66f";
const SECRET_OVERFLOW_BODY: &str = "overflow-secret-body-77g";
const SECRET_PROXY_USER: &str = "proxy-user-91a";
const SECRET_PROXY_PASS: &str = "proxy-pass-82b";

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_http")
            || metadata.target().starts_with("reqwest_client")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            LOGS.lock().unwrap_or_else(|e| e.into_inner()).push(format!(
                "log {} {}: {}",
                record.level(),
                record.target(),
                record.args()
            ));
        }
    }

    fn flush(&self) {}
}

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(self.0, " {}={value:?}", field.name()).unwrap();
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
            LOGS.lock().unwrap_or_else(|e| e.into_inner()).push(format!(
                "span {} {}{}",
                attributes.metadata().level(),
                attributes.metadata().name(),
                fields.0
            ));
            tracing::span::Id::from_u64(1)
        } else {
            tracing::span::Id::from_u64(2)
        }
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        if id.into_u64() != 1 {
            return;
        }
        let mut fields = Fields::default();
        values.record(&mut fields);
        if !fields.0.is_empty() {
            LOGS.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(format!("record{}", fields.0));
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("yard_http") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        LOGS.lock().unwrap_or_else(|e| e.into_inner()).push(format!(
            "event {}{}",
            event.metadata().level(),
            fields.0
        ));
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

struct PrivacyServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl PrivacyServer {
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
                    let mut chunk = [0u8; 1024];
                    let header_end = loop {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => {
                                buf.extend_from_slice(&chunk[..n]);
                                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                    break pos + 4;
                                }
                            }
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..header_end - 4]).into_owned();
                    let content_len: usize = head
                        .lines()
                        .find_map(|line| {
                            let (k, v) = line.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    while buf.len() - header_end < content_len {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }

                    let status = if head.starts_with("DELETE ") {
                        "404 Not Found"
                    } else {
                        "200 OK"
                    };
                    let body = SECRET_RESP_BODY.as_bytes();
                    let resp = format!(
                        "HTTP/1.1 {status}\r\n{SECRET_RESP_HEADER_NAME}: {SECRET_RESP_HEADER_VAL}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                    let _ = stream.write_all(body);
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

impl Drop for PrivacyServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn neither_log_contains_path_query_headers_body_or_proxy_credentials() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    let server = PrivacyServer::start();

    // 1. Configure with proxy credentials, then reconfigure to direct.
    let proxy_url = format!("http://{SECRET_PROXY_USER}:{SECRET_PROXY_PASS}@127.0.0.1:18081");
    let client = HttpClient::new(
        "PrivacyTest/1.0",
        &NetworkSettings {
            proxy: Some(proxy_url),
        },
    )
    .unwrap();
    client.reconfigure(&NetworkSettings { proxy: None });

    // 2. Successful POST with secret path, query, header, and body; secret response header & body.
    let ok_req = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "http://{}/{SECRET_PATH}?{SECRET_QUERY}",
            server.addr
        ))
        .header(SECRET_REQ_HEADER_NAME, SECRET_REQ_HEADER_VAL)
        .body(AsyncBody::from(SECRET_REQ_BODY))
        .unwrap();
    let ok_resp = block_on(client.send(ok_req)).unwrap();
    assert_eq!(ok_resp.status(), StatusCode::OK);
    let body = block_on(read_limited(ok_resp, 1024)).unwrap();
    assert_eq!(body, SECRET_RESP_BODY.as_bytes());

    // 3. 404 response on the same secret path and query.
    let not_found_req = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "http://{}/{SECRET_PATH}?{SECRET_QUERY}",
            server.addr
        ))
        .header(SECRET_REQ_HEADER_NAME, SECRET_REQ_HEADER_VAL)
        .body(AsyncBody::empty())
        .unwrap();
    let not_found_resp = block_on(client.send(not_found_req)).unwrap();
    assert_eq!(not_found_resp.status(), StatusCode::NOT_FOUND);

    // 4. Transport failure (refused connection) with secret path and query.
    let unused = TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_addr = unused.local_addr().unwrap();
    drop(unused);
    let fail_req = Request::builder()
        .uri(format!("http://{closed_addr}/{SECRET_PATH}?{SECRET_QUERY}"))
        .header(SECRET_REQ_HEADER_NAME, SECRET_REQ_HEADER_VAL)
        .body(AsyncBody::from(SECRET_REQ_BODY))
        .unwrap();
    assert!(block_on(client.send(fail_req)).is_err());

    // 5. Refused scheme with secret path and query.
    let bad_scheme = Request::builder()
        .uri(format!("ftp://127.0.0.1/{SECRET_PATH}?{SECRET_QUERY}"))
        .body(AsyncBody::empty())
        .unwrap();
    assert!(block_on(client.send(bad_scheme)).is_err());

    // 6. `read_limited` overflow with secret body.
    let overflow_resp = Response::builder()
        .status(StatusCode::OK)
        .body(AsyncBody::from(SECRET_OVERFLOW_BODY))
        .unwrap();
    assert!(block_on(read_limited(overflow_resp, 8)).is_err());

    let entries = LOGS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(
        entries.iter().any(|entry| entry.contains("http-configure")),
        "expected `http-configure` in captured logs: {entries:#?}"
    );
    assert!(
        entries.iter().any(|entry| entry.contains("http-request")),
        "expected `http-request` in captured logs: {entries:#?}"
    );

    let lowered: Vec<String> = entries.iter().map(|e| e.to_ascii_lowercase()).collect();
    for needle in [
        SECRET_PATH,
        SECRET_QUERY,
        SECRET_REQ_HEADER_NAME,
        SECRET_REQ_HEADER_VAL,
        SECRET_REQ_BODY,
        SECRET_RESP_HEADER_NAME,
        SECRET_RESP_HEADER_VAL,
        SECRET_RESP_BODY,
        SECRET_OVERFLOW_BODY,
        SECRET_PROXY_USER,
        SECRET_PROXY_PASS,
    ] {
        let needle_lower = needle.to_ascii_lowercase();
        assert!(
            lowered
                .iter()
                .all(move |entry| !entry.contains(&needle_lower)),
            "log leaked sensitive HTTP data ({needle}): {entries:#?}"
        );
    }
}
