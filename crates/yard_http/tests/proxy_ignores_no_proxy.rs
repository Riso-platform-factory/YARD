#![cfg(feature = "dev")]

//! An explicit `network.proxy` carries every request and ignores `NO_PROXY`.
//!
//! Mutates `NO_PROXY` in this process, so this test runs alone in its own binary.

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

fn read_request_line(stream: &mut TcpStream) -> Option<String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
            }
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end - 4]);
    head.split("\r\n").next().map(str::to_owned)
}

fn write_ok(stream: &mut TcpStream, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
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
    fn start(handler: impl Fn(String, &mut TcpStream) + Send + Sync + 'static) -> Self {
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
                    if let Some(line) = read_request_line(&mut stream) {
                        handler(line, &mut stream);
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
fn explicit_network_proxy_ignores_no_proxy_env_var() {
    // SAFETY: this file holds one test, and it has started no thread yet.
    unsafe {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    }

    let origin_hits = Arc::new(AtomicUsize::new(0));
    let origin_hits_server = Arc::clone(&origin_hits);
    let origin = TestServer::start(move |_, stream| {
        origin_hits_server.fetch_add(1, Ordering::SeqCst);
        write_ok(stream, b"direct-origin");
    });

    let proxy_hits = Arc::new(AtomicUsize::new(0));
    let proxy_line = Arc::new(Mutex::new(None::<String>));
    let proxy_hits_server = Arc::clone(&proxy_hits);
    let proxy_line_server = Arc::clone(&proxy_line);
    let proxy = TestServer::start(move |line, stream| {
        proxy_hits_server.fetch_add(1, Ordering::SeqCst);
        *proxy_line_server.lock().unwrap() = Some(line);
        write_ok(stream, b"via-proxy");
    });

    let proxy_url = format!("http://{}", proxy.addr);
    let client = HttpClient::new(
        "NoProxyTest/1.0",
        &NetworkSettings {
            proxy: Some(proxy_url.clone()),
        },
    )
    .unwrap();

    let response = block_on(client.get(&origin.url("/check"), AsyncBody::empty(), true)).unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = block_on(read_limited(response, 1024)).unwrap();
    assert_eq!(
        body, b"via-proxy",
        "explicit network.proxy must ignore NO_PROXY and route through the proxy"
    );
    assert_eq!(proxy_hits.load(Ordering::SeqCst), 1);
    assert_eq!(origin_hits.load(Ordering::SeqCst), 0);
    let seen_line = proxy_line.lock().unwrap().clone().unwrap_or_default();
    assert!(
        seen_line.contains(&origin.addr.to_string()),
        "proxy must receive the origin address in the request line, got {seen_line:?}"
    );

    // Reconfiguring to the explicit proxy must also ignore NO_PROXY.
    client.reconfigure(&NetworkSettings { proxy: None });
    client.reconfigure(&NetworkSettings {
        proxy: Some(proxy_url),
    });
    let response2 = block_on(client.get(&origin.url("/check2"), AsyncBody::empty(), true)).unwrap();
    assert_eq!(response2.status(), StatusCode::OK);
    let body2 = block_on(read_limited(response2, 1024)).unwrap();
    assert_eq!(body2, b"via-proxy");
    assert_eq!(proxy_hits.load(Ordering::SeqCst), 2);
    assert_eq!(origin_hits.load(Ordering::SeqCst), 0);
}
