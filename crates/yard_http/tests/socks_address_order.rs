//! A SOCKS proxy at `localhost` prefers the first address returned by the resolver
//! and falls back to the second within one request timeout if the first refuses.

use std::{
    future::Future,
    io::{self, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, JoinHandle},
    time::Duration,
};
use yard_http::{
    HttpClient, NetworkSettings,
    http_client::{AsyncBody, HttpClient as _, HttpRequestExt as _, Request, StatusCode, Url},
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

/// Minimal SOCKS5 server (no auth, CONNECT only) listening on one address.
struct Socks5Server {
    addr: SocketAddr,
    connects: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Socks5Server {
    fn start(bind_addr: SocketAddr) -> Self {
        let listener = TcpListener::bind(bind_addr).unwrap();
        let addr = listener.local_addr().unwrap();
        let connects = Arc::new(AtomicUsize::new(0));
        let connects_thread = Arc::clone(&connects);
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
                let connects_conn = Arc::clone(&connects_thread);
                thread::spawn(move || {
                    let _ = handle_socks5(stream, connects_conn);
                });
            }
        });
        Self {
            addr,
            connects,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Socks5Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_socks5(mut client: TcpStream, connects: Arc<AtomicUsize>) -> io::Result<()> {
    let _ = client.set_read_timeout(Some(Duration::from_secs(5)));
    let mut header = [0u8; 2];
    client.read_exact(&mut header)?;
    if header[0] != 0x05 {
        return Ok(());
    }
    let mut methods = vec![0u8; usize::from(header[1])];
    client.read_exact(&mut methods)?;
    client.write_all(&[0x05, 0x00])?;
    client.flush()?;

    let mut req = [0u8; 4];
    client.read_exact(&mut req)?;
    if req[0] != 0x05 || req[1] != 0x01 {
        let _ = client.write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
        return Ok(());
    }

    let upstream = match req[3] {
        0x01 => {
            let mut addr = [0u8; 4];
            let mut port = [0u8; 2];
            client.read_exact(&mut addr)?;
            client.read_exact(&mut port)?;
            TcpStream::connect((Ipv4Addr::from(addr), u16::from_be_bytes(port)))
        }
        0x03 => {
            let mut len = [0u8; 1];
            client.read_exact(&mut len)?;
            let mut host = vec![0u8; usize::from(len[0])];
            let mut port = [0u8; 2];
            client.read_exact(&mut host)?;
            client.read_exact(&mut port)?;
            let host = String::from_utf8_lossy(&host).into_owned();
            TcpStream::connect((host.as_str(), u16::from_be_bytes(port)))
        }
        0x04 => {
            let mut addr = [0u8; 16];
            let mut port = [0u8; 2];
            client.read_exact(&mut addr)?;
            client.read_exact(&mut port)?;
            TcpStream::connect((Ipv6Addr::from(addr), u16::from_be_bytes(port)))
        }
        _ => {
            let _ = client.write_all(&[0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
            return Ok(());
        }
    };

    let upstream = match upstream {
        Ok(stream) => stream,
        Err(_) => {
            let _ = client.write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
            return Ok(());
        }
    };

    connects.fetch_add(1, Ordering::SeqCst);
    let _ = client.set_read_timeout(None);
    let _ = upstream.set_read_timeout(None);
    client.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])?;
    client.flush()?;

    let mut client_read = client.try_clone()?;
    let mut upstream_write = upstream.try_clone()?;
    let relay = thread::spawn(move || {
        let _ = io::copy(&mut client_read, &mut upstream_write);
        let _ = upstream_write.shutdown(Shutdown::Write);
    });
    let mut upstream_read = upstream;
    let _ = io::copy(&mut upstream_read, &mut client);
    let _ = client.shutdown(Shutdown::Write);
    let _ = relay.join();
    Ok(())
}

#[test]
fn socks_proxy_tries_resolved_addresses_in_order() {
    let mut proxy = Url::parse("socks5://localhost:0").unwrap();
    let resolved = proxy.socket_addrs(|| Some(1080)).unwrap();
    if let [only] = resolved.as_slice() {
        // libtest has no dynamic skip. This binary contains only this test, so exit
        // before the harness records a pass and write directly to stderr.
        writeln!(
            io::stderr().lock(),
            "SKIP socks_proxy_tries_resolved_addresses_in_order: localhost resolved to only {}",
            only.ip()
        )
        .unwrap();
        std::process::exit(0);
    }
    let [first, second, ..] = resolved.as_slice() else {
        panic!("localhost resolved to no addresses");
    };
    assert_ne!(first, second, "the first two addresses must be distinct");

    let origin = OriginServer::start();
    let second_socks = Socks5Server::start(*second);
    let port = second_socks.addr.port();
    let mut first_addr = *first;
    first_addr.set_port(port);
    let first_socks = Socks5Server::start(first_addr);
    proxy.set_port(Some(port)).unwrap();

    let request_through = |path| {
        let client = HttpClient::new(
            "SocksOrderTest/1.0",
            &NetworkSettings {
                proxy: Some(proxy.to_string()),
            },
        )
        .unwrap();
        let request = Request::builder()
            .uri(origin.url(path))
            .timeout(Duration::from_secs(5))
            .body(AsyncBody::empty())
            .unwrap();
        let response = block_on(client.send(request))
            .unwrap_or_else(|err| panic!("SOCKS request to {path} failed: {err}"));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(block_on(read_limited(response, 1024)).unwrap(), b"ok");
    };

    // With both listeners available, the first must carry the request.
    request_through("/first");
    assert_eq!(first_socks.connects.load(Ordering::SeqCst), 1);
    assert_eq!(second_socks.connects.load(Ordering::SeqCst), 0);
    assert_eq!(origin.hits.load(Ordering::SeqCst), 1);

    // Close the first listener and use a fresh client so it resolves the proxy again.
    drop(first_socks);
    assert_eq!(
        TcpStream::connect_timeout(&first_addr, Duration::from_secs(1))
            .unwrap_err()
            .kind(),
        io::ErrorKind::ConnectionRefused,
        "the first address {first_addr} must refuse connections"
    );
    request_through("/fallback");
    assert_eq!(second_socks.connects.load(Ordering::SeqCst), 1);
    assert_eq!(origin.hits.load(Ordering::SeqCst), 2);
}
