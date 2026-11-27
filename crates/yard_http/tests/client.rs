//! Acceptance tests for `yard_http/client`: user agent, GET requests, redirect limits,
//! scheme validation, `read_limited`, proxy routing and reconfiguration, invalid proxy
//! fail-closed behavior, and request cancellation.

use std::{
    future::Future,
    io::{ErrorKind, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use yard_http::{
    HttpClient, NetworkSettings,
    http_client::{AsyncBody, HttpClient as _, Request, Response, StatusCode},
    read_limited, user_agent,
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

struct ParsedRequest {
    request_line: String,
    target: String,
    headers: Vec<(String, String)>,
}

impl ParsedRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(move |(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_http_request(stream: &mut TcpStream) -> Option<ParsedRequest> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
            }
            Err(_) => return None,
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end - 4]);
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?.to_owned();
    let target = request_line.split_whitespace().nth(1)?.to_owned();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    Some(ParsedRequest {
        request_line,
        target,
        headers,
    })
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra_headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

struct TestServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TestServer {
    fn start(handler: impl Fn(ParsedRequest, &mut TcpStream) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handler = Arc::new(handler);
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
                let handler = Arc::clone(&handler);
                thread::spawn(move || {
                    if let Some(req) = read_http_request(&mut stream) {
                        handler(req, &mut stream);
                    }
                });
            }
        });
        Self {
            addr,
            stop,
            handle: Some(handle),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn user_agent_formats_name_version_os_and_arch() {
    let expected = format!(
        "North Star/1.2.3 ({}; {})",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    assert_eq!(user_agent("North Star", "1.2.3"), expected);
}

#[test]
fn get_reaches_server_with_configured_user_agent() {
    let seen_ua = Arc::new(Mutex::new(None::<String>));
    let seen_ua_server = Arc::clone(&seen_ua);
    let server = TestServer::start(move |req, stream| {
        if let Some(ua) = req.header("user-agent") {
            *seen_ua_server.lock().unwrap() = Some(ua.to_owned());
        }
        write_response(stream, "200 OK", &[], b"hello-local");
    });

    let ua = user_agent("North Star", "1.2.3");
    let client = HttpClient::new(&ua, &NetworkSettings::default()).unwrap();
    let response = block_on(client.get(&server.url("/ping"), AsyncBody::empty(), true)).unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = block_on(read_limited(response, 1024)).unwrap();
    assert_eq!(body, b"hello-local");
    assert_eq!(
        seen_ua.lock().unwrap().as_deref(),
        Some(ua.as_str()),
        "server must receive the configured User-Agent header"
    );
}

#[test]
fn redirects_follow_up_to_ten_and_refuse_eleventh() {
    let reached_zero = Arc::new(AtomicUsize::new(0));
    let reached_zero_server = Arc::clone(&reached_zero);
    let server = TestServer::start(move |req, stream| {
        if let Some(raw) = req.target.strip_prefix("/hop/")
            && let Ok(remaining) = raw.parse::<usize>()
        {
            if remaining == 0 {
                reached_zero_server.fetch_add(1, Ordering::SeqCst);
                write_response(stream, "200 OK", &[], b"arrived");
            } else {
                let next = format!("/hop/{}", remaining - 1);
                write_response(stream, "302 Found", &[("Location", next.as_str())], b"");
            }
            return;
        }
        write_response(stream, "404 Not Found", &[], b"missing");
    });

    let client = HttpClient::new("RedirectTest/1.0", &NetworkSettings::default()).unwrap();

    // 10 redirects must succeed.
    let ok = block_on(client.get(&server.url("/hop/10"), AsyncBody::empty(), true)).unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let body = block_on(read_limited(ok, 1024)).unwrap();
    assert_eq!(body, b"arrived");
    assert_eq!(reached_zero.load(Ordering::SeqCst), 1);

    // An 11th redirect must be refused and must not reach the final hop.
    reached_zero.store(0, Ordering::SeqCst);
    let too_many = block_on(client.get(&server.url("/hop/11"), AsyncBody::empty(), true));
    assert!(
        too_many.is_err(),
        "an 11th redirect must be refused, got status {:?}",
        too_many.ok().map(|r| r.status())
    );
    assert_eq!(
        reached_zero.load(Ordering::SeqCst),
        0,
        "client must not follow the 11th redirect"
    );

    // With follow_redirects = false, a redirect response is returned directly.
    let no_follow = block_on(client.get(&server.url("/hop/1"), AsyncBody::empty(), false)).unwrap();
    assert_eq!(no_follow.status(), StatusCode::FOUND);
}

#[test]
fn redirect_to_disallowed_scheme_is_refused() {
    let server = TestServer::start(move |_, stream| {
        write_response(
            stream,
            "302 Found",
            &[("Location", "file://localhost/tmp/yard-redirect-secret")],
            b"",
        );
    });

    let client = HttpClient::new("SchemeRedirectTest/1.0", &NetworkSettings::default()).unwrap();
    let result = block_on(client.get(&server.url("/bounce"), AsyncBody::empty(), true));
    assert!(
        result.is_err(),
        "redirect to file:// must be refused, got status {:?}",
        result.ok().map(|r| r.status())
    );
}

#[test]
fn disallowed_schemes_are_refused() {
    let client = HttpClient::new("SchemeTest/1.0", &NetworkSettings::default()).unwrap();

    // Build Requests directly so http::Uri validates cleanly and yard_http::HttpClient::send
    // itself enforces the scheme allow-list.
    let file_req = Request::builder()
        .uri("file://localhost/tmp/yard-http-test")
        .body(AsyncBody::empty())
        .unwrap();
    let file_res = block_on(client.send(file_req));
    assert!(file_res.is_err(), "file:// URL must be refused");

    let ftp_req = Request::builder()
        .uri("ftp://127.0.0.1/file")
        .body(AsyncBody::empty())
        .unwrap();
    let ftp_res = block_on(client.send(ftp_req));
    assert!(ftp_res.is_err(), "ftp:// URL must be refused");

    let ftp_get = block_on(client.get("ftp://127.0.0.1/file", AsyncBody::empty(), true));
    assert!(ftp_get.is_err(), "ftp:// via get() must be refused");
}

#[test]
fn read_limited_returns_body_within_limit_and_refuses_larger() {
    // In-memory bodies: under limit, exact limit, over limit.
    let under = Response::builder()
        .status(StatusCode::OK)
        .body(AsyncBody::from(vec![b'a'; 10]))
        .unwrap();
    assert_eq!(block_on(read_limited(under, 16)).unwrap(), vec![b'a'; 10]);

    let exact = Response::builder()
        .status(StatusCode::OK)
        .body(AsyncBody::from(vec![b'b'; 16]))
        .unwrap();
    assert_eq!(block_on(read_limited(exact, 16)).unwrap(), vec![b'b'; 16]);

    let over = Response::builder()
        .status(StatusCode::OK)
        .body(AsyncBody::from(vec![b'c'; 17]))
        .unwrap();
    assert!(
        block_on(read_limited(over, 16)).is_err(),
        "body larger than max must be refused"
    );

    // Streaming response from a real local server.
    let server = TestServer::start(move |req, stream| match req.target.as_str() {
        "/exact" => write_response(stream, "200 OK", &[], &[b'x'; 64]),
        "/too-big" => write_response(stream, "200 OK", &[], &[b'y'; 65]),
        _ => write_response(stream, "404 Not Found", &[], b""),
    });

    let client = HttpClient::new("LimitTest/1.0", &NetworkSettings::default()).unwrap();

    let ok_resp = block_on(client.get(&server.url("/exact"), AsyncBody::empty(), true)).unwrap();
    assert_eq!(block_on(read_limited(ok_resp, 64)).unwrap(), vec![b'x'; 64]);

    let big_resp = block_on(client.get(&server.url("/too-big"), AsyncBody::empty(), true)).unwrap();
    assert!(
        block_on(read_limited(big_resp, 64)).is_err(),
        "streaming body larger than max must be refused"
    );
}

#[test]
fn proxy_routes_requests_and_reconfigure_to_none_goes_direct() {
    let origin_hits = Arc::new(AtomicUsize::new(0));
    let origin_hits_server = Arc::clone(&origin_hits);
    let origin = TestServer::start(move |_, stream| {
        origin_hits_server.fetch_add(1, Ordering::SeqCst);
        write_response(stream, "200 OK", &[], b"direct-origin");
    });

    let proxy_hits = Arc::new(AtomicUsize::new(0));
    let proxy_line = Arc::new(Mutex::new(None::<String>));
    let proxy_hits_server = Arc::clone(&proxy_hits);
    let proxy_line_server = Arc::clone(&proxy_line);
    let proxy = TestServer::start(move |req, stream| {
        proxy_hits_server.fetch_add(1, Ordering::SeqCst);
        *proxy_line_server.lock().unwrap() = Some(req.request_line);
        write_response(stream, "200 OK", &[], b"via-proxy");
    });

    let settings = NetworkSettings {
        proxy: Some(format!("http://{}", proxy.addr)),
    };
    let client = HttpClient::new("ProxyTest/1.0", &settings).unwrap();

    // First request must go through the local proxy.
    let proxied = block_on(client.get(&origin.url("/check"), AsyncBody::empty(), true)).unwrap();
    assert_eq!(proxied.status(), StatusCode::OK);
    let proxied_body = block_on(read_limited(proxied, 1024)).unwrap();
    assert_eq!(proxied_body, b"via-proxy");
    assert_eq!(proxy_hits.load(Ordering::SeqCst), 1);
    assert_eq!(origin_hits.load(Ordering::SeqCst), 0);
    let seen_line = proxy_line.lock().unwrap().clone().unwrap_or_default();
    assert!(
        seen_line.contains(&origin.addr.to_string()),
        "proxy must see origin address in request line, got {seen_line:?}"
    );

    // Reconfiguring proxy to None makes the next request go directly to origin.
    client.reconfigure(&NetworkSettings { proxy: None });
    let direct = block_on(client.get(&origin.url("/check"), AsyncBody::empty(), true)).unwrap();
    assert_eq!(direct.status(), StatusCode::OK);
    let direct_body = block_on(read_limited(direct, 1024)).unwrap();
    assert_eq!(direct_body, b"direct-origin");
    assert_eq!(origin_hits.load(Ordering::SeqCst), 1);
    assert_eq!(proxy_hits.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_proxy_fails_closed_and_reconfigure_recovers() {
    let origin_hits = Arc::new(AtomicUsize::new(0));
    let origin_hits_server = Arc::clone(&origin_hits);
    let origin = TestServer::start(move |_, stream| {
        origin_hits_server.fetch_add(1, Ordering::SeqCst);
        write_response(stream, "200 OK", &[], b"origin");
    });

    let invalid = NetworkSettings {
        proxy: Some("not a url".to_owned()),
    };
    let client = HttpClient::new("InvalidProxyTest/1.0", &invalid).unwrap();

    // Must fail closed and must not connect directly to origin.
    let refused = block_on(client.get(&origin.url("/blocked"), AsyncBody::empty(), true));
    assert!(refused.is_err(), "invalid proxy must refuse requests");
    assert_eq!(
        origin_hits.load(Ordering::SeqCst),
        0,
        "invalid proxy must not fall back to a direct request"
    );

    // Fixing the setting via reconfigure allows subsequent requests to succeed.
    client.reconfigure(&NetworkSettings { proxy: None });
    let recovered = block_on(client.get(&origin.url("/ok"), AsyncBody::empty(), true)).unwrap();
    assert_eq!(recovered.status(), StatusCode::OK);
    assert_eq!(origin_hits.load(Ordering::SeqCst), 1);

    // Breaking it again fails closed again.
    client.reconfigure(&invalid);
    let refused_again = block_on(client.get(&origin.url("/blocked2"), AsyncBody::empty(), true));
    assert!(
        refused_again.is_err(),
        "reconfiguring to invalid proxy must refuse requests"
    );
    assert_eq!(origin_hits.load(Ordering::SeqCst), 1);
}

#[test]
fn dropping_request_future_cancels_request_and_closes_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let (connected_tx, connected_rx) = mpsc::channel::<()>();
    let (closed_tx, closed_rx) = mpsc::channel::<bool>();

    let server = thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            let _ = closed_tx.send(false);
            return;
        };
        if read_http_request(&mut stream).is_none() {
            let _ = closed_tx.send(false);
            return;
        }
        let _ = connected_tx.send(());

        // Hold the response back and wait until the client closes the connection.
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 64];
        let closed = loop {
            match stream.read(&mut buf) {
                Ok(0) => break true,
                Ok(_) => continue,
                Err(err)
                    if matches!(
                        err.kind(),
                        ErrorKind::ConnectionReset
                            | ErrorKind::ConnectionAborted
                            | ErrorKind::BrokenPipe
                            | ErrorKind::UnexpectedEof
                    ) =>
                {
                    break true;
                }
                Err(_) => break false,
            }
        };
        let _ = closed_tx.send(closed);
    });

    let client = HttpClient::new("CancelTest/1.0", &NetworkSettings::default()).unwrap();
    let url = format!("http://{addr}/slow");

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
    {
        let mut fut = client.get(&url, AsyncBody::empty(), true);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(res) => {
                    panic!(
                        "request completed unexpectedly before cancellation: {:?}",
                        res.map(|r| r.status())
                    );
                }
                Poll::Pending => {}
            }
            if connected_rx.try_recv().is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for client to reach local server"
            );
            thread::park_timeout(Duration::from_millis(20));
        }
        // Dropping `fut` here must cancel the in-flight request.
    }

    let saw_close = closed_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("timed out waiting for server to observe connection close");
    assert!(
        saw_close,
        "dropping the request future must close the client connection"
    );
    server.join().unwrap();
}
