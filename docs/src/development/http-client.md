# http-client: one shared, logged HTTP client

Today YARD's startup builds an HTTP client for `yard_update` with `ReqwestClient::user_agent`. That client trusts only the roots bundled with it, not the operating system's store, so it fails behind a corporate TLS-inspecting proxy. It ignores a proxy setting. It logs nothing on its own. And extensions have no client at all, so each would bring its own.

This slice gives the application one client:

- it trusts the system's root certificates;
- it honours the proxy;
- it logs every request without the user's data;
- `yard_update`, GPUI, and every extension share it.

The design follows Zed's `http_client`, `reqwest_client`, and `http_client_tls`, all Apache-2.0, read in the research for this slice (sources in the proposal). They are permissive, so they stay outside the GPL boundary.

## What an extension writes

```rust
let http = ext.http_client(cx);
let task = cx.background_spawn(async move {
    let response = http.get("https://api.example.com/items", AsyncBody::empty(), true).await?;
    let body = yard_http::read_limited(response, 1 << 20).await?; // at most 1 MiB
    anyhow::Ok(body)
});
```

- **`Extension::http_client(cx)`** returns the shared client, tagged with the extension's name for the logs. It implements Zed's `http_client::HttpClient`, which `yard_shell` re-exports with its request and body types, so an extension adds no HTTP dependency.
- **Requests run off the UI thread**, on the client's own runtime, as in Zed. Dropping the future cancels the request.
- **Defaults:**
  - **Timeouts:** connect 10 s; a request may set its own total timeout.
  - **Redirects:** up to 10. A redirect from `https` to `http` is refused.
  - **Schemes:** `http` and `https` are allowed. Any other is refused.
- **`yard_http::read_limited(response, max)`** reads a body up to a limit, and refuses a larger one.

## Trust and proxy

- **Certificates** come from the system's store through `rustls-platform-verifier`, as Zed's `http_client_tls` does: Security framework on macOS, CryptoAPI on Windows, the system bundle on Linux. A corporate root the user's system trusts is trusted.
- **Proxy:**
  - The new settings section `network` holds `proxy`, a URL (`http://`, `https://`, `socks5://`, `socks5h://`), as Zed's `proxy` setting does. A missing value, `null`, or an empty string means no proxy is set.
  - A proxy set in `network.proxy` carries every request: `NO_PROXY` does not apply to it.
  - Without it, the standard variables apply as `reqwest` reads them: `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`.
  - Without those, the operating system's HTTP and HTTPS proxy applies on macOS and Windows, which Zed's `reqwest` reads. An OS SOCKS proxy is not read.
  - A change to the section rebuilds the client for the next requests.
  - **An invalid `proxy` value fails closed:** an unparsable URL, another scheme, or a value that is not a string. Requests fail with a proxy error until it is fixed, and a `WARN` names the problem. Nothing goes out unproxied by mistake.
  - A SOCKS proxy's host name, set or from the environment, is resolved off the UI thread, at the request. Each resolved address is tried in order, and the first that accepts a connection is used. A failure fails that request, never sends it direct, and is tried again at the next one.
  - A request's time limit covers building the connection too. Requests that arrive while one is being built for the same setting wait for it.
- **The user agent** is `<Name>/<version> (<os>; <arch>)`, from the product. The engine hardcodes no name.
- `network` is a section name an extension cannot take, like `yard` and `extensions`.

## Who uses it

- **`yard_log/diagnostic_log`** (production, changed): drops the HTTP stack's records below `WARN`.
- **The startup** builds the client from the product and the `network` section, and gives it to the shell and to GPUI (`cx.set_http_client`).
- **`yard_update`** gets the same client tagged `update`, instead of building its own. Its rules do not change: no redirect, HTTPS only (loopback allowed in tests), and its size and time limits.

## Logging

- **`http-configure{proxy}`:** on start and on every change. `proxy` gives only the scheme, host, and port, never the username or password. If the platform verifier fails to start, Zed's `http_client_tls` logs that itself and falls back to bundled roots.
- **`http-request{caller, method, host}`:** around each request, with `status` and `elapsed_ms` at `debug`.
  - A failure is logged once, through `log` (the trace receives it through the bridge). An error returned to the caller, including one while reading the body, carries a fixed text and never the URL.
  - `yard_log` drops records and `tracing` events and spans below `WARN` from the HTTP stack's own crates (`reqwest`, `zed_reqwest`, `hyper`, `hyper_util`, `hyper_rustls`, `h2`, `rustls`; matched by crate name), in both logs, since their debug lines carry URLs. Their `WARN` and `ERROR` lines are kept with a fixed text, `HTTP stack warning` or `HTTP stack error`, since even those can carry a path.
  - It never logs the path, query, headers, or body.
  - An HTTP error status is not a failure of the client: `debug`.
  - A transport, TLS, timeout, proxy, or size failure: `WARN`.
  - A refused scheme or redirect: `info`.

## Parts

- **`yard_http/client`** (new engine crate `crates/yard_http`, `src/dev/client.rs`):
  - the client, wrapping Zed's `reqwest_client` with the `http_client_tls` verifier;
  - the `network` settings type;
  - reconfiguring;
  - the caller tag;
  - the limits;
  - `read_limited`;
  - the logging.

  It depends on Zed's `http_client` and `reqwest_client`, both Apache-2.0, and on `yard_settings` for the section type, which brings GPUI in.
- **`yard_shell/http`** (`crates/yard_shell/src/dev/http.rs`): `set_http_client`, which is called once (a second call is refused with an `ERROR`), and `Extension::http_client`. A handle looks up the shared client at each request, so a handle taken before `set_http_client` works once it is set. Until a client is set, it returns a client that refuses every request, with one `ERROR`, as `yard_shell/data` does.
- **The startup and `yard_update`:**
  - The startup is production, so while the parts are under review, the review extension in `review.rs` builds the client, sets it, and gives it to GPUI.
  - `yard_update` keeps the startup's client until promotion.
  - At promotion, the startup builds it, `yard_update` takes it, and both are shown to the user and stamped again.
- **Acceptance tests**, written first by a worker that does not write the code, against a local HTTP server in the test (no network):
  - a request reaches the server, with the user agent;
  - the caller tag appears in the log;
  - redirects up to 10 are followed, and an `https`-to-`http` redirect is refused;
  - a body over the limit is refused;
  - a request through a local proxy goes through the proxy;
  - an invalid `proxy` refuses requests;
  - changing the section switches the proxy for the next request;
  - an unknown scheme is refused;
  - dropping the future cancels the request;
  - in their own files, the groups, fields, and levels above, and that no path, query, header, or body appears in either log.
- **Test extension `examples/http_tour`**, named in `review.rs`. Without `HTTP_TOUR` it does nothing. With `HTTP_TOUR=<url>` it places a card with a **Fetch** button; the card shows the status, the byte count, and the time.

  The README walks through:

  - a public `https` URL;
  - a URL whose server sends a redirect to `http` (refused);
  - a local proxy, with `mitmproxy` or `python -m http.server` as the target, and `network.proxy` set: the proxy sees the request;
  - an invalid `proxy` (refused, `WARN`);
  - changing `proxy` while running;
  - `HTTPS_PROXY` set in the environment.

## Known limits

- **On Windows, a SOCKS proxy from the environment ignores the Internet Options exclusion list (`ProxyOverride`).** `NO_PROXY` applies.
- **Native `WARN` and `ERROR` events of the HTTP stack (`hyper`, `h2`) reach the diagnostic log only while the lifecycle trace is on**, as before this slice. Their `log` records reach it always.
- **The SOCKS slow-build test uses the system resolver** (an unresolvable `.local` name), so it proves the behaviour on macOS, and elsewhere may pass without exercising it. The address-order test follows whatever order the resolver gives `localhost`, and says SKIP when it gives one address ([YARD#23](https://github.com/Riso-platform-factory/YARD/issues/23)).

## Not in this slice

- Reading the Linux desktop's proxy settings, an OS SOCKS proxy, and PAC files: only the setting, the environment, and the macOS and Windows HTTP(S) proxy are read. `TODO(http-client-system-proxy)`, [YARD#18](https://github.com/Riso-platform-factory/YARD/issues/18).
- Proxy authentication prompts. Credentials can be put in the proxy URL.
- A per-extension allow-list of hosts, as Tauri's scope has.
- Moving `yard_update`'s own rules into the client.
