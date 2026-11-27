#![cfg(feature = "dev")]

//! An environment SOCKS proxy (`ALL_PROXY=socks5://no-such-host.invalid:1080`) that
//! cannot be resolved fails the request with a proxy error and sends nothing to the
//! origin. After the environment proxy is fixed and the client is reconfigured, the
//! next request succeeds.
//!
//! Mutates proxy environment variables, so this test runs alone in its own binary.

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

/// Minimal SOCKS5 server (no auth, CONNECT only) listening on `127.0.0.1`.
struct Socks5Server {
    addr: SocketAddr,
    connects: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Socks5Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
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
fn unresolvable_env_socks_proxy_fails_closed_and_recovers_after_reconfigure() {
    // SAFETY: this file holds one test, and it has started no thread yet.
    unsafe {
        for var in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        ] {
            std::env::remove_var(var);
        }
        std::env::set_var("ALL_PROXY", "socks5://no-such-host.invalid:1080");
    }

    let origin = OriginServer::start();
    let socks = Socks5Server::start();

    let client = HttpClient::new("EnvSocksTest/1.0", &NetworkSettings::default()).unwrap();

    // 1. With an unresolvable `ALL_PROXY` and no `network.proxy`, the request must fail
    //    with a proxy error and the origin must receive nothing.
    let err = block_on(client.get(
        &origin.url("/secret-env?token=env123"),
        AsyncBody::empty(),
        true,
    ))
    .map(|r| r.status())
    .expect_err("unresolvable ALL_PROXY SOCKS proxy must fail the request");
    let err_lower = err.to_string().to_ascii_lowercase();
    assert!(
        err_lower.contains("proxy"),
        "unresolvable ALL_PROXY SOCKS proxy must fail with a proxy error, got: {err}"
    );
    assert!(
        !err_lower.contains("secret-env") && !err_lower.contains("token=env123"),
        "proxy error must not leak request URL: {err}"
    );
    assert_eq!(
        origin.hits.load(Ordering::SeqCst),
        0,
        "failed environment SOCKS proxy must not fall back to direct request"
    );

    // 2. After fixing `ALL_PROXY` and reconfiguring the client, the next request works.
    // SAFETY: only this test thread mutates env vars; no request is in flight.
    unsafe {
        std::env::set_var("ALL_PROXY", format!("socks5://{}", socks.addr));
    }
    client.reconfigure(&NetworkSettings::default());

    let response = block_on(client.get(&origin.url("/recovered"), AsyncBody::empty(), true))
        .expect("reconfiguring after fixing ALL_PROXY must succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let body = block_on(read_limited(response, 1024)).unwrap();
    assert_eq!(body, b"ok");
    assert!(
        socks.connects.load(Ordering::SeqCst) >= 1,
        "fixed ALL_PROXY SOCKS5 server must carry the request"
    );
    assert_eq!(origin.hits.load(Ordering::SeqCst), 1);
}
