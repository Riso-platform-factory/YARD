//! A non-string `network.proxy` (`8080`, `true`, `[]`, `{}`) fails closed at start
//! and after a settings reload: requests fail with a proxy error and a `WARN`, and
//! nothing goes direct. `""` and `null` mean no proxy is set: requests go direct.
//!
//! Installs a process-wide diagnostic logger; alone in its binary.

use gpui::TestAppContext;
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use yard_shell::yard_http::{
    HttpClient, NetworkSettings,
    http_client::{AsyncBody, HttpClient as _, StatusCode},
    read_limited,
};

static DIAGNOSTICS: Mutex<Vec<(String, log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_http") || metadata.target().starts_with("yard_settings")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS.lock().unwrap_or_else(|e| e.into_inner()).push((
                record.target().to_owned(),
                record.level(),
                record.args().to_string(),
            ));
        }
    }

    fn flush(&self) {}
}

fn clear_diagnostics() {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

fn diagnostics() -> Vec<(String, log::Level, String)> {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn yard_http_warnings() -> Vec<String> {
    diagnostics()
        .into_iter()
        .filter(|(target, level, _)| target.starts_with("yard_http") && *level == log::Level::Warn)
        .map(|(_, _, msg)| msg)
        .collect()
}

fn yard_http_errors() -> Vec<String> {
    diagnostics()
        .into_iter()
        .filter(|(target, level, _)| target.starts_with("yard_http") && *level == log::Level::Error)
        .map(|(_, _, msg)| msg)
        .collect()
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
                        b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\ndirect-ok",
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

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
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

fn write_settings(path: &PathBuf, proxy_json: &str) {
    fs::write(path, format!(r#"{{"network": {{"proxy": {proxy_json}}}}}"#)).unwrap();
}

#[gpui::test]
async fn non_string_proxy_fails_closed_at_start_and_reload_while_empty_and_null_go_direct(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();

    let origin = OriginServer::start();
    let dir = tempfile::tempdir().unwrap();

    // 1. At start, non-string `network.proxy` values must fail closed with a proxy error and WARN.
    for (index, raw) in ["8080", "true", "[]", "{}"].into_iter().enumerate() {
        let path = dir.path().join(format!("start-invalid-{index}.json"));
        write_settings(&path, raw);
        let hits_before = origin.hits();
        clear_diagnostics();

        let app = cx.new_app();
        let path_for_load = path.clone();
        let client = app.update(move |cx| {
            yard_settings::init(cx);
            yard_settings::load(cx, path_for_load);
            yard_settings::register::<NetworkSettings>(cx);
            HttpClient::new(
                "ProxySettingsStart/1.0",
                yard_settings::get::<NetworkSettings>(cx),
            )
            .unwrap()
        });

        let result = client
            .get(&origin.url("/blocked-at-start"), AsyncBody::empty(), true)
            .await;
        let err = result.map(|r| r.status()).expect_err(&format!(
            "non-string network.proxy ({raw}) at start must fail closed"
        ));
        assert!(
            err.to_string().to_ascii_lowercase().contains("proxy"),
            "non-string network.proxy ({raw}) at start must fail with a proxy error, got: {err}"
        );
        assert_eq!(
            origin.hits(),
            hits_before,
            "non-string network.proxy ({raw}) at start must not fall back to direct request"
        );
        assert!(
            !yard_http_warnings().is_empty(),
            "non-string network.proxy ({raw}) at start must log a yard_http WARN, diagnostics: {:?}",
            diagnostics()
        );
        assert!(
            yard_http_errors().is_empty(),
            "non-string network.proxy ({raw}) at start must not log ERROR: {:?}",
            diagnostics()
        );

        app.quit();
        app.run_until_parked();
    }

    // 2. At start, `""` and `null` mean no proxy is set: requests go direct.
    for (index, raw) in [r#""""#, "null"].into_iter().enumerate() {
        let path = dir.path().join(format!("start-direct-{index}.json"));
        write_settings(&path, raw);
        let hits_before = origin.hits();
        clear_diagnostics();

        let app = cx.new_app();
        let path_for_load = path.clone();
        let client = app.update(move |cx| {
            yard_settings::init(cx);
            yard_settings::load(cx, path_for_load);
            yard_settings::register::<NetworkSettings>(cx);
            HttpClient::new(
                "ProxySettingsDirect/1.0",
                yard_settings::get::<NetworkSettings>(cx),
            )
            .unwrap()
        });

        let resp = client
            .get(&origin.url("/direct-at-start"), AsyncBody::empty(), true)
            .await
            .unwrap_or_else(move |err| {
                panic!("network.proxy = {raw} at start must go direct, failed with: {err}")
            });
        assert_eq!(resp.status(), StatusCode::OK);
        let body = read_limited(resp, 1024).await.unwrap();
        assert_eq!(body, b"direct-ok");
        assert_eq!(
            origin.hits(),
            hits_before + 1,
            "network.proxy = {raw} at start must reach origin directly"
        );
        assert!(
            yard_http_warnings().is_empty(),
            "network.proxy = {raw} at start must not log WARN: {:?}",
            diagnostics()
        );
        assert!(
            yard_http_errors().is_empty(),
            "network.proxy = {raw} at start must not log ERROR: {:?}",
            diagnostics()
        );

        app.quit();
        app.run_until_parked();
    }

    // 3. After reload, non-string `network.proxy` values must fail closed with a proxy error
    //    and WARN, and reloading back to `null` or `""` must restore direct requests.
    let reload_path = dir.path().join("reload.json");
    write_settings(&reload_path, "null");

    let app = cx.new_app();
    let reload_path_for_load = reload_path.clone();
    let (client, _sub) = app.update(move |cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, reload_path_for_load);
        yard_settings::register::<NetworkSettings>(cx);
        let client = HttpClient::new(
            "ProxySettingsReload/1.0",
            yard_settings::get::<NetworkSettings>(cx),
        )
        .unwrap();
        let client_for_reload = client.clone();
        let sub = yard_settings::observe::<NetworkSettings>(cx, move |cx| {
            client_for_reload.reconfigure(yard_settings::get::<NetworkSettings>(cx));
        });
        yard_settings::watch(cx);
        (client, sub)
    });

    // Confirm baseline direct request succeeds before testing reloads.
    let hits_before = origin.hits();
    let baseline = client
        .get(&origin.url("/baseline"), AsyncBody::empty(), true)
        .await
        .unwrap();
    assert_eq!(baseline.status(), StatusCode::OK);
    assert_eq!(origin.hits(), hits_before + 1);

    for (raw, recovery) in [
        ("8080", "null"),
        ("true", r#""""#),
        ("[]", "null"),
        ("{}", r#""""#),
    ] {
        let hits_before = origin.hits();
        clear_diagnostics();

        write_settings(&reload_path, raw);
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();

        let result = client
            .get(
                &origin.url("/blocked-after-reload"),
                AsyncBody::empty(),
                true,
            )
            .await;
        let err = result.map(|r| r.status()).expect_err(&format!(
            "reloading network.proxy to non-string ({raw}) must fail closed"
        ));
        assert!(
            err.to_string().to_ascii_lowercase().contains("proxy"),
            "reloading network.proxy to ({raw}) must fail with a proxy error, got: {err}"
        );
        assert_eq!(
            origin.hits(),
            hits_before,
            "reloading network.proxy to ({raw}) must not fall back to direct request"
        );
        assert!(
            !yard_http_warnings().is_empty(),
            "reloading network.proxy to ({raw}) must log a yard_http WARN, diagnostics: {:?}",
            diagnostics()
        );
        assert!(
            yard_http_errors().is_empty(),
            "reloading network.proxy to ({raw}) must not log ERROR: {:?}",
            diagnostics()
        );

        // Reloading to `null` or `""` restores direct requests.
        clear_diagnostics();
        write_settings(&reload_path, recovery);
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();

        let recovered = client
            .get(&origin.url("/recovered"), AsyncBody::empty(), true)
            .await
            .unwrap_or_else(move |err| {
                panic!("reloading network.proxy to {recovery} must go direct, failed with: {err}")
            });
        assert_eq!(recovered.status(), StatusCode::OK);
        let body = read_limited(recovered, 1024).await.unwrap();
        assert_eq!(body, b"direct-ok");
        assert_eq!(
            origin.hits(),
            hits_before + 1,
            "reloading network.proxy to {recovery} must reach origin directly"
        );
        assert!(
            yard_http_warnings().is_empty(),
            "reloading network.proxy to {recovery} must not log WARN: {:?}",
            diagnostics()
        );
    }

    drop(_sub);
    app.quit();
    app.run_until_parked();
}
