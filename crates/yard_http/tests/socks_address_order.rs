//! A SOCKS proxy host that resolves to `::1` first and `127.0.0.1` second
//! (`socks5://localhost:<port>` on macOS), with a local SOCKS5 server listening
//! on `127.0.0.1` only, tries resolved addresses in order and carries the request.

use std::{
    future::Future,
    io::{self, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs as _},
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

/// Minimal SOCKS5 server (no auth, CONNECT only) listening on `127.0.0.1` only.
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
fn socks_proxy_tries_resolved_addresses_in_order_from_ipv6_to_ipv4() {
    let origin = OriginServer::start();
    let socks = Socks5Server::start();
    let port = socks.addr.port();

    // Confirm the host resolves to `::1` first and `127.0.0.1` second, and nothing
    // is listening on `[::1]:<port>`.
    let resolved: Vec<SocketAddr> = ("localhost", port).to_socket_addrs().unwrap().collect();
    assert!(
        resolved.len() >= 2
            && resolved[0].ip() == Ipv6Addr::LOCALHOST
            && resolved[1].ip() == Ipv4Addr::LOCALHOST,
        "test requires `localhost` to resolve to `::1` first and `127.0.0.1` second, got: {resolved:?}"
    );
    assert!(
        TcpStream::connect((Ipv6Addr::LOCALHOST, port)).is_err(),
        "nothing should listen on [::1]:{port}"
    );

    // Sanity check: the minimal SOCKS5 server carries a request when addressed directly on 127.0.0.1.
    let direct_socks = HttpClient::new(
        "SocksOrderSanity/1.0",
        &NetworkSettings {
            proxy: Some(format!("socks5://127.0.0.1:{port}")),
        },
    )
    .unwrap();
    let sanity = block_on(direct_socks.get(&origin.url("/sanity"), AsyncBody::empty(), true))
        .expect("minimal SOCKS5 server must work on 127.0.0.1");
    assert_eq!(sanity.status(), StatusCode::OK);
    assert_eq!(block_on(read_limited(sanity, 1024)).unwrap(), b"ok");
    let connects_after_sanity = socks.connects.load(Ordering::SeqCst);
    assert!(connects_after_sanity >= 1);
    assert_eq!(origin.hits.load(Ordering::SeqCst), 1);

    // Now configure `socks5://localhost:<port>`: `::1` refuses, so the client must fall
    // through to `127.0.0.1` and carry the request.
    let localhost_socks = HttpClient::new(
        "SocksOrderTest/1.0",
        &NetworkSettings {
            proxy: Some(format!("socks5://localhost:{port}")),
        },
    )
    .unwrap();
    let response = block_on(localhost_socks.get(&origin.url("/ordered"), AsyncBody::empty(), true))
        .expect("socks5://localhost:<port> must try 127.0.0.1 after ::1 refuses");
    assert_eq!(response.status(), StatusCode::OK);
    let body = block_on(read_limited(response, 1024)).unwrap();
    assert_eq!(body, b"ok");
    assert!(
        socks.connects.load(Ordering::SeqCst) > connects_after_sanity,
        "request through socks5://localhost:{port} must reach the 127.0.0.1 SOCKS5 server"
    );
    assert_eq!(origin.hits.load(Ordering::SeqCst), 2);
}
