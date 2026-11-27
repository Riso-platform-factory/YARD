use anyhow::{Result, anyhow, bail};
use futures::{
    channel::oneshot,
    future::{BoxFuture, Either, FutureExt as _, Shared, select},
    io::{AsyncRead, AsyncReadExt as _},
};
use http_client::{
    AsyncBody, HttpClient as _, HttpRequestExt as _, Inner, Method, RedirectPolicy, Request,
    RequestTimeout, Response, StatusCode, Url,
    http::{HeaderValue, header, uri::Authority},
};
use reqwest_client::ReqwestClient;
use std::{
    future::Future,
    io,
    net::{IpAddr, TcpStream},
    pin::Pin,
    sync::{Arc, Mutex, OnceLock, RwLock},
    task::{Context, Poll, ready},
    time::{Duration, Instant},
};
use tracing::Instrument as _;

const MAX_REDIRECTS: u32 = 10;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
type ResponseFuture = Pin<Box<dyn Future<Output = Result<Response<AsyncBody>>> + Send + 'static>>;

struct SharedClient {
    user_agent: HeaderValue,
    transport: RwLock<Result<Arc<Routes>, &'static str>>,
}

enum Routes {
    Single(Arc<Transport>),
    Environment {
        http: Arc<Transport>,
        https: Arc<Transport>,
        direct: Arc<Transport>,
        exclusions: String,
    },
}

impl Routes {
    fn select(&self, url: &Url) -> &Arc<Transport> {
        match self {
            Self::Single(transport) => transport,
            Self::Environment {
                http,
                https,
                direct,
                exclusions,
            } => {
                if bypass_proxy(url, exclusions) {
                    direct
                } else if url.scheme() == "https" {
                    https
                } else {
                    http
                }
            }
        }
    }
}

struct Transport {
    proxy: Option<Url>,
    automatic: bool,
    state: Mutex<BuildState>,
}

type BuildResult = Result<Arc<ReqwestClient>, &'static str>;
type Build = Shared<BoxFuture<'static, BuildResult>>;

enum BuildState {
    Idle,
    Building(Build),
    Ready(Arc<ReqwestClient>),
}

impl Transport {
    fn new(proxy: Option<Url>, automatic: bool) -> Arc<Self> {
        Arc::new(Self {
            proxy,
            automatic,
            state: Mutex::new(BuildState::Idle),
        })
    }

    fn build(self: &Arc<Self>, build: impl FnOnce() -> BuildResult + Send + 'static) -> Build {
        let mut state = self.state.lock().unwrap();
        match &*state {
            BuildState::Ready(client) => {
                return futures::future::ready(Ok(client.clone())).boxed().shared();
            }
            BuildState::Building(build) => return build.clone(),
            BuildState::Idle => {}
        }
        let (sender, receiver) = oneshot::channel();
        let pending = receiver
            .map(|result| result.unwrap_or(Err("HTTP client could not be initialized")))
            .boxed()
            .shared();
        *state = BuildState::Building(pending.clone());

        // The producer outlives request waiters, so cancellation cannot leave a second
        // blocking build queued behind the first.
        let transport = self.clone();
        let runtime = reqwest_client::runtime();
        let build = runtime.spawn_blocking(build);
        runtime.spawn(async move {
            let result = build
                .await
                .unwrap_or(Err("HTTP client could not be initialized"));
            *transport.state.lock().unwrap() = match &result {
                Ok(client) => BuildState::Ready(client.clone()),
                Err(_) => BuildState::Idle,
            };
            let _ = sender.send(result);
        });
        pending
    }

    async fn client(
        self: &Arc<Self>,
        timeout: Option<Duration>,
        started: Instant,
        build: impl FnOnce(Instant) -> BuildResult + Send + 'static,
    ) -> Result<Arc<ReqwestClient>> {
        let remaining = timeout
            .map(|timeout| remaining_timeout(timeout, started))
            .transpose()?;
        let connect_started = Instant::now();
        let wait = if self.proxy.as_ref().is_some_and(is_socks) {
            Some(remaining.map_or(CONNECT_TIMEOUT, |remaining| remaining.min(CONNECT_TIMEOUT)))
        } else {
            remaining
        };
        // The producer may outlive this caller and still serve longer-lived waiters.
        let pending = self.build(move || build(connect_started + CONNECT_TIMEOUT));
        let result = if let Some(wait) = wait {
            let timer = {
                let _runtime = reqwest_client::runtime().enter();
                Box::pin(tokio::time::sleep(
                    wait.saturating_sub(connect_started.elapsed()),
                ))
            };
            match select(pending, timer).await {
                Either::Left((result, _)) => result,
                Either::Right(_) => return Err(warn_failure("HTTP request timed out")),
            }
        } else {
            pending.await
        };
        result.map_err(warn_failure)
    }

    async fn send(
        self: &Arc<Self>,
        user_agent: &HeaderValue,
        mut request: Request<AsyncBody>,
        timeout: Option<Duration>,
        started: Instant,
    ) -> Result<Response<AsyncBody>> {
        let proxy = self.proxy.clone();
        let automatic = self.automatic;
        let user_agent = user_agent.clone();
        let client = self
            .client(timeout, started, move |connect_deadline| {
                build_client(proxy, automatic, user_agent, connect_deadline).map(Arc::new)
            })
            .await?;
        if let Some(timeout) = timeout {
            request
                .extensions_mut()
                .insert(RequestTimeout(remaining_timeout(timeout, started)?));
        }
        client.send(request).await.map_err(|error| {
            let (timed_out, connect) = error
                .downcast_ref::<zed_reqwest::Error>()
                .map_or((false, false), |error| {
                    (error.is_timeout(), error.is_connect())
                });
            warn_failure(self.send_failure(&client, timed_out, connect))
        })
    }

    fn send_failure(
        &self,
        client: &Arc<ReqwestClient>,
        timed_out: bool,
        connect: bool,
    ) -> &'static str {
        let socks = self.proxy.as_ref().is_some_and(is_socks);
        if socks && (connect || timed_out) {
            let mut state = self.state.lock().unwrap();
            if matches!(&*state, BuildState::Ready(cached) if Arc::ptr_eq(cached, client)) {
                *state = BuildState::Idle;
            }
        }
        if timed_out {
            "HTTP request timed out"
        } else if socks && connect {
            "HTTP proxy connection failed"
        } else {
            "HTTP request failed"
        }
    }
}

/// A handle to the application's shared HTTP client.
#[derive(Clone)]
pub struct HttpClient {
    shared: Arc<OnceLock<Arc<SharedClient>>>,
    caller: Arc<str>,
}

impl HttpClient {
    pub fn new(user_agent: &str, network: &NetworkSettings) -> Result<Self> {
        let user_agent =
            HeaderValue::from_str(user_agent).map_err(|_| anyhow!("invalid HTTP user agent"))?;
        let transport = RwLock::new(configure(network));
        Ok(Self {
            shared: Arc::new(OnceLock::from(Arc::new(SharedClient {
                user_agent,
                transport,
            }))),
            caller: Arc::from("app"),
        })
    }

    /// A handle that refuses requests until the application installs a client.
    #[doc(hidden)]
    pub fn unavailable() -> Self {
        Self {
            shared: Arc::new(OnceLock::new()),
            caller: Arc::from("app"),
        }
    }

    /// Installs the transport for a waiting handle and its existing clones.
    #[doc(hidden)]
    pub fn activate(&self, client: &Self) {
        if let Some(shared) = client.shared.get() {
            let _ = self.shared.set(shared.clone());
        }
    }

    pub fn reconfigure(&self, network: &NetworkSettings) {
        if let Some(shared) = self.shared.get() {
            let transport = configure(network);
            *shared.transport.write().unwrap() = transport;
        }
    }

    pub fn for_caller(&self, caller: &str) -> HttpClient {
        Self {
            shared: self.shared.clone(),
            caller: Arc::from(caller),
        }
    }
}

impl http_client::HttpClient for HttpClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        self.shared.get().map(|shared| &shared.user_agent)
    }

    fn proxy(&self) -> Option<&Url> {
        // The route can change after reconfiguration and depends on the destination.
        None
    }

    fn send(&self, request: Request<AsyncBody>) -> ResponseFuture {
        let caller = self.caller.clone();
        let shared = self.shared.clone();
        Box::pin(async move {
            let span = tracing::info_span!(
                "http-request",
                caller = %caller,
                method = %request.method(),
                host = request.uri().host().unwrap_or(""),
            );
            async move {
                let started = Instant::now();
                let Some(shared) = shared.get() else {
                    tracing::info!("HTTP client unavailable");
                    bail!("HTTP client unavailable");
                };
                let transport = shared
                    .transport
                    .read()
                    .unwrap()
                    .clone()
                    .map_err(|reason| anyhow!(reason))?;
                let response =
                    send_request(&transport, &shared.user_agent, request, started).await?;
                tracing::debug!(
                    status = response.status().as_u16(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "request finished"
                );
                Ok(response.map(sanitize_body))
            }
            .instrument(span)
            .await
        })
    }
}

fn configure(network: &NetworkSettings) -> Result<Arc<Routes>, &'static str> {
    let proxy = network
        .proxy
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(parse_proxy)
        .transpose();
    let label = match &proxy {
        Ok(Some(url)) => {
            let port = url.port_or_known_default().unwrap_or(1080);
            format!("{}://{}:{port}", url.scheme(), url.host_str().unwrap_or(""))
        }
        Ok(None) => "automatic".into(),
        Err(_) => "invalid".into(),
    };
    let _span = tracing::info_span!("http-configure", proxy = %label).entered();
    match proxy {
        Ok(_) => tracing::debug!("configured"),
        Err(reason) => log::warn!("{reason}"),
    }
    proxy.map(|proxy| {
        Arc::new(match proxy {
            Some(proxy) => Routes::Single(Transport::new(Some(proxy), false)),
            None => environment_routes(),
        })
    })
}

fn is_socks(url: &Url) -> bool {
    matches!(url.scheme(), "socks4" | "socks4a" | "socks5" | "socks5h")
}

fn env_proxy(names: &[&str]) -> Option<Url> {
    names.iter().find_map(|name| {
        let value = std::env::var(name).ok()?;
        let value = value.trim();
        let url = Url::parse(value)
            .ok()
            .filter(|url| url.has_host())
            .or_else(|| {
                if !value.is_empty() && !value.contains("://") {
                    Url::parse(&format!("http://{value}")).ok()
                } else {
                    None
                }
            })?;
        // Validate other schemes with reqwest. A SOCKS URL is validated here without
        // allowing reqwest to resolve it and silently discard it on lookup failure.
        if is_socks(&url) || zed_reqwest::Proxy::all(url.clone()).is_ok() {
            Some(url)
        } else {
            None
        }
    })
}

fn environment_routes() -> Routes {
    let all = env_proxy(&["ALL_PROXY", "all_proxy"]);
    let http = if std::env::var_os("REQUEST_METHOD").is_none() {
        env_proxy(&["HTTP_PROXY", "http_proxy"])
    } else {
        None
    };
    let https = env_proxy(&["HTTPS_PROXY", "https_proxy"]);
    if ![&all, &http, &https].into_iter().flatten().any(is_socks) {
        return Routes::Single(Transport::new(None, true));
    }

    let http = http.or_else(|| all.clone());
    let https = https.or(all);
    let direct = Transport::new(None, false);
    let route = |proxy| match proxy {
        Some(proxy) => Transport::new(Some(proxy), false),
        None => direct.clone(),
    };
    let http_route = route(http.clone());
    let https_route = if http == https {
        http_route.clone()
    } else {
        route(https)
    };
    Routes::Environment {
        http: http_route,
        https: https_route,
        direct,
        exclusions: std::env::var("NO_PROXY")
            .or_else(|_| std::env::var("no_proxy"))
            .unwrap_or_default(),
    }
}

// Reqwest's NO_PROXY matcher is private. Apply its domain, address and CIDR
// rules before waiting on SOCKS, so an excluded destination never waits on DNS.
fn bypass_proxy(url: &Url, exclusions: &str) -> bool {
    let host = url.host_str().unwrap_or("");
    let address = host.trim_matches(['[', ']']).parse::<IpAddr>().ok();
    exclusions
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .any(|entry| {
            if let Some(address) = address {
                if let Some((network, prefix)) = entry.split_once('/') {
                    return match (address, network.parse::<IpAddr>(), prefix.parse::<u32>()) {
                        (IpAddr::V4(address), Ok(IpAddr::V4(network)), Ok(prefix))
                            if prefix <= 32 =>
                        {
                            (u32::from(address) ^ u32::from(network)).leading_zeros() >= prefix
                        }
                        (IpAddr::V6(address), Ok(IpAddr::V6(network)), Ok(prefix))
                            if prefix <= 128 =>
                        {
                            (u128::from(address) ^ u128::from(network)).leading_zeros() >= prefix
                        }
                        _ => false,
                    };
                }
                return entry.parse::<IpAddr>() == Ok(address);
            }
            let domain = entry.strip_prefix('.').unwrap_or(entry);
            entry == "*"
                || host == domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
}

fn resolve_socks_proxy(mut proxy: Url, deadline: Instant) -> Result<Url, &'static str> {
    if !is_socks(&proxy)
        || proxy
            .host_str()
            .is_some_and(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok())
    {
        return Ok(proxy);
    }
    let addresses = proxy
        .socket_addrs(|| Some(1080))
        .map_err(|_| "HTTP proxy host could not be resolved")?;
    let mut addresses = addresses.into_iter();
    let address = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("HTTP proxy connection timed out");
        }
        let Some(address) = addresses.next() else {
            return Err("HTTP proxy host could not be reached");
        };
        // Reserve part of the remaining build time for later addresses.
        let attempt = remaining.div_f64(addresses.len() as f64 + 1.0);
        if !attempt.is_zero() && TcpStream::connect_timeout(&address, attempt).is_ok() {
            break address;
        }
    };
    proxy
        .set_ip_host(address.ip())
        .map_err(|_| "invalid HTTP proxy host")?;
    proxy
        .set_port(Some(address.port()))
        .map_err(|_| "invalid HTTP proxy port")?;
    Ok(proxy)
}

fn build_client(
    proxy: Option<Url>,
    automatic: bool,
    user_agent: HeaderValue,
    connect_deadline: Instant,
) -> Result<ReqwestClient, &'static str> {
    if automatic {
        // TODO(http-client-system-proxy): Linux desktop proxy settings, OS SOCKS proxies, and
        // PAC files are not read. macOS and Windows HTTP(S) system proxies come from reqwest.
        // See https://github.com/Riso-platform-factory/YARD/issues/18.
        let agent = std::str::from_utf8(user_agent.as_bytes())
            .expect("the user agent was constructed from a string");
        return ReqwestClient::proxy_and_user_agent(None, agent)
            .map_err(|_| "HTTP transport could not be configured");
    }

    // Zed's explicit proxy constructor attaches NO_PROXY. Build this route without exclusions.
    let socks_deadline = proxy
        .as_ref()
        .filter(|proxy| is_socks(proxy))
        .map(|_| connect_deadline);
    let proxy = proxy
        .map(|proxy| resolve_socks_proxy(proxy, connect_deadline))
        .transpose()?
        .map(zed_reqwest::Proxy::all)
        .transpose()
        .map_err(|_| "HTTP proxy could not be configured")?;
    let builder = client_builder(user_agent, socks_deadline)?;
    let builder = match proxy {
        Some(proxy) => builder.proxy(proxy),
        None => builder.no_proxy(),
    };
    builder
        .build()
        .map(ReqwestClient::from)
        .map_err(|_| "HTTP proxy could not be configured")
}

fn client_builder(
    user_agent: HeaderValue,
    socks_deadline: Option<Instant>,
) -> Result<zed_reqwest::ClientBuilder, &'static str> {
    let mut headers = http_client::http::HeaderMap::new();
    headers.insert(header::USER_AGENT, user_agent);
    let tls = http_client_tls::tls_config();
    if socks_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err("HTTP proxy connection timed out");
    }
    // Probe time belongs to the build. A cached transport gets a fresh
    // connection budget when it opens any connection.
    Ok(zed_reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .connect_timeout(CONNECT_TIMEOUT)
        .tcp_keepalive(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(30))
        .http2_keep_alive_interval(Duration::from_secs(15))
        .http2_keep_alive_timeout(Duration::from_secs(10))
        .http2_keep_alive_while_idle(true)
        .default_headers(headers))
}

fn parse_proxy(value: &str) -> Result<Url, &'static str> {
    const INVALID_PROXY: &str = "invalid network.proxy URL";
    let value = value.trim();
    let url = Url::parse(value).map_err(|_| INVALID_PROXY)?;
    let address = value.split_once("://").map(|(_, address)| address);
    if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h")
        || address.is_none_or(|address| address.starts_with('/'))
        || value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(INVALID_PROXY);
    }
    // Validate the credential-free authority as Zed's transport will parse it.
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or(INVALID_PROXY)?;
    let port = url.port_or_known_default().unwrap_or(1080);
    if port == 0 || format!("{host}:{port}").parse::<Authority>().is_err() {
        return Err(INVALID_PROXY);
    }
    Ok(url)
}

async fn send_request(
    routes: &Routes,
    user_agent: &HeaderValue,
    request: Request<AsyncBody>,
    started: Instant,
) -> Result<Response<AsyncBody>> {
    let (mut parts, mut body) = request.into_parts();
    let mut url =
        Url::parse(&parts.uri.to_string()).map_err(|_| refuse("invalid HTTP request URL"))?;
    check_scheme(&url)?;
    let timeout = parts
        .extensions
        .get::<RequestTimeout>()
        .map(|timeout| timeout.0);
    let limit = match parts.extensions.remove::<RedirectPolicy>() {
        Some(RedirectPolicy::NoFollow) => {
            parts.extensions.insert(RedirectPolicy::NoFollow);
            return routes
                .select(&url)
                .send(
                    user_agent,
                    Request::from_parts(parts, body),
                    timeout,
                    started,
                )
                .await;
        }
        Some(RedirectPolicy::FollowLimit(limit)) => limit.min(MAX_REDIRECTS),
        Some(RedirectPolicy::FollowAll) | None => MAX_REDIRECTS,
    };
    // Disable upstream redirects to check each destination before sending anything to it.
    for hops in 0..=limit {
        let replay = match &body.0 {
            Inner::Empty => Some(AsyncBody::empty()),
            Inner::Bytes(cursor) => Some(AsyncBody(Inner::Bytes(cursor.clone()))),
            Inner::AsyncReader(_) => None,
        };
        let hop = Request::builder()
            .method(parts.method.clone())
            .uri(parts.uri.clone())
            .follow_redirects(RedirectPolicy::NoFollow);
        let mut hop = hop.body(body).map_err(|_| refuse("invalid HTTP request"))?;
        *hop.headers_mut() = parts.headers.clone();
        let response = routes
            .select(&url)
            .send(user_agent, hop, timeout, started)
            .await?;
        let status = response.status();
        if !matches!(
            status,
            StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::SEE_OTHER
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT
        ) {
            return Ok(response);
        }
        let Some(mut next) = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| std::str::from_utf8(value.as_bytes()).ok())
            .and_then(|location| url.join(location).ok())
        else {
            return Ok(response);
        };
        check_redirect(&url, &next)?;
        body = match status {
            StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT => {
                let Some(replay) = replay else {
                    return Ok(response);
                };
                replay
            }
            _ => {
                if !matches!(parts.method, Method::GET | Method::HEAD) {
                    parts.method = Method::GET;
                }
                for header in [
                    header::TRANSFER_ENCODING,
                    header::CONTENT_ENCODING,
                    header::CONTENT_TYPE,
                    header::CONTENT_LENGTH,
                ] {
                    parts.headers.remove(header);
                }
                AsyncBody::empty()
            }
        };
        if hops == limit {
            return Err(refuse("HTTP redirect limit exceeded"));
        }
        if url.origin() != next.origin() {
            for header in [
                header::AUTHORIZATION,
                header::PROXY_AUTHORIZATION,
                header::COOKIE,
                header::WWW_AUTHENTICATE,
                header::HOST,
                header::REFERER,
            ] {
                parts.headers.remove(header);
            }
            parts.headers.remove("cookie2");
        }
        next.set_fragment(None);
        parts.uri = next
            .as_str()
            .parse()
            .map_err(|_| refuse("invalid HTTP redirect URL"))?;
        url = next;
    }
    unreachable!("every path at the redirect limit returns")
}

fn remaining_timeout(timeout: Duration, started: Instant) -> Result<Duration> {
    let remaining = timeout.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(warn_failure("HTTP request timed out"));
    }
    Ok(remaining)
}

fn check_scheme(url: &Url) -> Result<()> {
    if matches!(url.scheme(), "http" | "https") {
        Ok(())
    } else {
        Err(refuse("HTTP URL scheme refused"))
    }
}

fn check_redirect(from: &Url, to: &Url) -> Result<()> {
    check_scheme(to)?;
    if from.scheme() == "https" && to.scheme() == "http" {
        return Err(refuse("HTTPS to HTTP redirect refused"));
    }
    Ok(())
}

fn refuse(reason: &'static str) -> anyhow::Error {
    tracing::info!("{reason}");
    anyhow!(reason)
}

fn warn_failure(reason: &'static str) -> anyhow::Error {
    // Transport and body errors can contain request URLs or arbitrary caller data.
    log::warn!("{reason}");
    anyhow!(reason)
}

const BODY_READ_ERROR: &str = "HTTP response body could not be read";

#[derive(Debug)]
struct BodyReadError;

impl std::fmt::Display for BodyReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(BODY_READ_ERROR)
    }
}

impl std::error::Error for BodyReadError {}

struct LoggedBody {
    reader: Option<Pin<Box<dyn AsyncRead + Send + Sync>>>,
    span: tracing::Span,
}

impl AsyncRead for LoggedBody {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let Some(reader) = &mut this.reader else {
            return Poll::Ready(Err(io::Error::other(BodyReadError)));
        };
        match ready!(reader.as_mut().poll_read(cx, buffer)) {
            Ok(count) => Poll::Ready(Ok(count)),
            Err(_) => {
                this.reader = None;
                this.span.in_scope(|| log::warn!("{BODY_READ_ERROR}"));
                Poll::Ready(Err(io::Error::other(BodyReadError)))
            }
        }
    }
}

fn sanitize_body(body: AsyncBody) -> AsyncBody {
    match body.0 {
        Inner::AsyncReader(reader) => AsyncBody::from_reader(LoggedBody {
            reader: Some(reader),
            span: tracing::Span::current(),
        }),
        _ => body,
    }
}

#[tracing::instrument(name = "http-read-limited", skip(response))]
pub async fn read_limited(response: Response<AsyncBody>, max: usize) -> Result<Vec<u8>> {
    let mut body = response.into_body();
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let capacity = buffer.len().min((max - output.len()).saturating_add(1));
        let chunk = &mut buffer[..capacity];
        let read = body.read(chunk).await.map_err(|error| {
            if error
                .get_ref()
                .is_some_and(|source| source.is::<BodyReadError>())
            {
                anyhow!(BODY_READ_ERROR)
            } else {
                warn_failure(BODY_READ_ERROR)
            }
        })?;
        if read == 0 {
            return Ok(output);
        }
        if read > max - output.len() {
            return Err(warn_failure("HTTP response body exceeds the size limit"));
        }
        output.extend_from_slice(&chunk[..read]);
    }
}

#[derive(Clone, Default, serde::Deserialize)]
pub struct NetworkSettings {
    #[serde(default, deserialize_with = "deserialize_proxy")]
    pub proxy: Option<String>,
}

fn deserialize_proxy<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum ProxyValue {
        Text(String),
        Invalid(serde::de::IgnoredAny),
    }

    use serde::Deserialize as _;
    Ok(
        Option::<ProxyValue>::deserialize(deserializer)?.map(|value| match value {
            ProxyValue::Text(text) => text,
            ProxyValue::Invalid(_) => String::from("<invalid proxy>"),
        }),
    )
}

impl yard_settings::Settings for NetworkSettings {
    const KEY: &'static str = "network";
}

pub fn user_agent(name: &str, version: &str) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("{name}/{version} ({os}; {arch})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socks_transport() -> Arc<Transport> {
        Transport::new(Some(Url::parse("socks5://127.0.0.1:1080").unwrap()), false)
    }

    #[test]
    fn short_socks_waiter_does_not_shorten_the_shared_build() {
        reqwest_client::runtime();
        let transport = socks_transport();
        let (building, producer_started) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let started = Instant::now();
        let mut short = std::pin::pin!(transport.client(
            Some(Duration::from_millis(100)),
            started,
            move |deadline| {
                building.send(deadline).unwrap();
                blocked.recv().unwrap();
                Err("shared producer finished")
            },
        ));
        assert!(short.as_mut().now_or_never().is_none());
        let deadline = producer_started
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(deadline >= started + CONNECT_TIMEOUT);

        let mut long = std::pin::pin!(
            transport.client(None, Instant::now(), |_| Err("unexpected second build"),)
        );
        assert!(long.as_mut().now_or_never().is_none());
        let timeout = futures::executor::block_on(short).err().unwrap();
        assert_eq!(timeout.to_string(), "HTTP request timed out");
        release.send(()).unwrap();
        let result = futures::executor::block_on(long).err().unwrap();
        assert_eq!(result.to_string(), "shared producer finished");
    }

    #[test]
    fn socks_build_keeps_the_full_cached_connect_timeout() {
        http_client_tls::tls_config();
        let user_agent = HeaderValue::from_static("Test/1.0");
        let almost_expired = Instant::now() + Duration::from_secs(2);
        let builder = client_builder(user_agent.clone(), Some(almost_expired)).unwrap();
        assert!(format!("{builder:?}").contains("connect_timeout: 10s"));

        let expired = client_builder(user_agent, Some(Instant::now())).unwrap_err();
        assert_eq!(expired, "HTTP proxy connection timed out");
    }

    #[test]
    fn socks_timeout_invalidates_only_the_current_route_and_takes_priority() {
        let transport = socks_transport();
        let new_client = || {
            Arc::new(ReqwestClient::from(
                zed_reqwest::Client::builder().no_proxy().build().unwrap(),
            ))
        };
        let stale = new_client();
        let current = new_client();
        for (timed_out, connect, message) in [
            (true, false, "HTTP request timed out"),
            (true, true, "HTTP request timed out"),
            (false, true, "HTTP proxy connection failed"),
        ] {
            *transport.state.lock().unwrap() = BuildState::Ready(current.clone());
            assert_eq!(transport.send_failure(&stale, timed_out, connect), message);
            assert!(
                matches!(&*transport.state.lock().unwrap(), BuildState::Ready(client) if Arc::ptr_eq(client, &current))
            );
            assert_eq!(
                transport.send_failure(&current, timed_out, connect),
                message
            );
            assert!(matches!(
                &*transport.state.lock().unwrap(),
                BuildState::Idle
            ));
        }
    }

    #[test]
    fn cancelled_waiter_keeps_one_build_and_failed_build_can_be_retried() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let transport = Transport::new(None, false);
        let starts = Arc::new(AtomicUsize::new(0));
        let (signal_started, first_started) = std::sync::mpsc::channel();
        let (release, building) = std::sync::mpsc::channel();
        let first_starts = starts.clone();
        let first = transport.build(move || {
            first_starts.fetch_add(1, Ordering::SeqCst);
            signal_started.send(()).unwrap();
            building.recv().unwrap();
            Err("first attempt failed")
        });
        first_started.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(first);
        let waiter = |count: Arc<AtomicUsize>| {
            transport.build(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Err("unexpected overlapping build")
            })
        };
        let second = waiter(starts.clone());
        let third = waiter(starts.clone());
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        release.send(()).unwrap();
        let (second, third) = futures::executor::block_on(futures::future::join(second, third));
        assert_eq!(second.err(), Some("first attempt failed"));
        assert_eq!(third.err(), Some("first attempt failed"));
        let retry_starts = starts.clone();
        let retry = transport.build(move || {
            retry_starts.fetch_add(1, Ordering::SeqCst);
            Err("second attempt failed")
        });
        assert_eq!(
            futures::executor::block_on(retry).err(),
            Some("second attempt failed")
        );
        assert_eq!(starts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn rejects_https_downgrade() {
        let secure = Url::parse("https://example.test").unwrap();
        let plain = Url::parse("http://example.test").unwrap();
        assert!(check_redirect(&secure, &plain).is_err());
        assert!(check_redirect(&secure, &secure).is_ok());
        assert!(check_redirect(&plain, &secure).is_ok());
    }
}
