#![cfg(feature = "dev")]

//! A request's time limit covers building the connection (including slow SOCKS proxy
//! host resolution): a request with a 1 s timeout ends within about 1 s with a timeout
//! error, both on its own and when two requests start together.
//!
//! Alone in its binary so any abandoned slow DNS lookup terminates cleanly on exit.

use std::{
    future::Future,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use yard_http::{
    HttpClient, NetworkSettings,
    http_client::{AsyncBody, HttpClient as _, HttpRequestExt as _, Request},
};

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

fn assert_timeout_error(context: &str, err: &anyhow::Error) {
    let lower = err.to_string().to_ascii_lowercase();
    assert!(
        lower.contains("timed out") || lower.contains("timeout"),
        "{context}: expected a timeout error, got: {err}"
    );
    assert!(
        !lower.contains("secret-timeout") && !lower.contains("token=t123"),
        "{context}: timeout error must not leak request URL: {err}"
    );
}

#[test]
fn request_timeout_covers_slow_socks_connection_build() {
    let origin = OriginServer::start();
    let pid = std::process::id();

    // `.local` triggers local mDNS resolution without external network traffic, which
    // delays resolution well beyond 1 s on macOS.
    let slow_proxy = format!("socks5://yard-slow-build-{pid}-a.local:1080");
    let client = HttpClient::new(
        "SocksBuildTimeoutTest/1.0",
        &NetworkSettings {
            proxy: Some(slow_proxy),
        },
    )
    .unwrap();

    // 1. Single request with a 1 s timeout ends within about 1 s with a timeout error.
    let request = Request::builder()
        .uri(origin.url("/secret-timeout?token=t123"))
        .timeout(Duration::from_secs(1))
        .body(AsyncBody::empty())
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let client_for_thread = client.clone();
    let started = Instant::now();
    thread::spawn(move || {
        let result = block_on(client_for_thread.send(request));
        let _ = tx.send((result, started.elapsed()));
    });

    let (result, elapsed) = rx.recv_timeout(Duration::from_secs(3)).expect(
        "request with 1 s timeout must end within about 1 s while building a slow SOCKS connection (still blocked after 3 s)",
    );
    let err = result
        .map(|r| r.status())
        .expect_err("slow SOCKS build must fail with a timeout error");
    assert_timeout_error("single slow SOCKS build", &err);
    assert!(
        elapsed >= Duration::from_millis(800) && elapsed <= Duration::from_millis(2500),
        "request with 1 s timeout should finish around 1 s, elapsed={elapsed:?}"
    );
    assert_eq!(
        origin.hits.load(Ordering::SeqCst),
        0,
        "timed-out SOCKS build must not hit origin"
    );

    // 2. Two requests started together on the same client wait on the same slow build
    //    and both end within about 1 s with timeout errors.
    let slow_proxy_pair = format!("socks5://yard-slow-build-{pid}-b.local:1080");
    let pair_client = HttpClient::new(
        "SocksBuildTimeoutPair/1.0",
        &NetworkSettings {
            proxy: Some(slow_proxy_pair),
        },
    )
    .unwrap();
    let req1 = Request::builder()
        .uri(origin.url("/secret-timeout?token=t123&n=1"))
        .timeout(Duration::from_secs(1))
        .body(AsyncBody::empty())
        .unwrap();
    let req2 = Request::builder()
        .uri(origin.url("/secret-timeout?token=t123&n=2"))
        .timeout(Duration::from_secs(1))
        .body(AsyncBody::empty())
        .unwrap();

    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    thread::spawn(move || {
        let (r1, r2) = block_on(futures::future::join(
            pair_client.send(req1),
            pair_client.send(req2),
        ));
        let _ = tx.send((r1, r2, started.elapsed()));
    });

    let (r1, r2, elapsed) = rx.recv_timeout(Duration::from_secs(3)).expect(
        "two concurrent requests with 1 s timeout must end within about 1 s while building a slow SOCKS connection (still blocked after 3 s)",
    );
    let e1 = r1
        .map(|r| r.status())
        .expect_err("first concurrent request must fail with timeout");
    let e2 = r2
        .map(|r| r.status())
        .expect_err("second concurrent request must fail with timeout");
    assert_timeout_error("first concurrent request", &e1);
    assert_timeout_error("second concurrent request", &e2);
    assert!(
        elapsed >= Duration::from_millis(800) && elapsed <= Duration::from_millis(2500),
        "concurrent requests with 1 s timeout should finish around 1 s, elapsed={elapsed:?}"
    );
    assert_eq!(origin.hits.load(Ordering::SeqCst), 0);
}
