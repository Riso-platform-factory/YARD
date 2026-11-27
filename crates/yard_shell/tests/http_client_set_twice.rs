//! Calling `set_http_client` a second time is refused with one `ERROR`, and the
//! first client stays active.
//!
//! Installs a process-wide diagnostic logger; alone in its binary.

use gpui::TestAppContext;
use std::{
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

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell") || metadata.target().starts_with("yard_http")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

struct UserAgentServer {
    addr: SocketAddr,
    seen_ua: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl UserAgentServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen_ua = Arc::new(Mutex::new(None::<String>));
        let seen_ua_thread = Arc::clone(&seen_ua);
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
                let seen_ua_req = Arc::clone(&seen_ua_thread);
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
                    let head = String::from_utf8_lossy(&buf[..header_end - 4]);
                    for line in head.split("\r\n").skip(1) {
                        if let Some((k, v)) = line.split_once(':')
                            && k.trim().eq_ignore_ascii_case("user-agent")
                        {
                            *seen_ua_req.lock().unwrap() = Some(v.trim().to_owned());
                        }
                    }
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    );
                    let _ = stream.flush();
                });
            }
        });
        Self {
            addr,
            seen_ua,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for UserAgentServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[gpui::test]
async fn set_http_client_twice_is_refused_with_one_error_and_keeps_first_client(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();

    let server = UserAgentServer::start();
    let app = cx.new_app();
    let notes = app.update(|cx| {
        let first = HttpClient::new("FirstClient/1.0", &NetworkSettings::default()).unwrap();
        let second = HttpClient::new("SecondClient/2.0", &NetworkSettings::default()).unwrap();
        shell::set_http_client(cx, first);
        shell::set_http_client(cx, second);
        shell::Extension::new(cx, "notes").unwrap()
    });

    let records = DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let errors: Vec<_> = records
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "calling set_http_client twice must log exactly one ERROR: {records:#?}"
    );

    let http = app.read(move |cx| notes.http_client(cx));
    let url = format!("http://{}/check", server.addr);
    let resp = http.get(&url, AsyncBody::empty(), true).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = read_limited(resp, 1024).await.unwrap();
    assert_eq!(body, b"ok");
    assert_eq!(
        server.seen_ua.lock().unwrap().as_deref(),
        Some("FirstClient/1.0"),
        "the first client must stay active after a second set_http_client call"
    );

    app.quit();
    app.run_until_parked();
}
