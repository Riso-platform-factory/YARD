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
  - The new settings section `network` holds `proxy`, a URL (`http://`, `https://`, `socks5://`), as Zed's `proxy` setting does.
  - Without it, the standard variables apply as `reqwest` reads them: `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`.
  - A change to the section rebuilds the client for the next requests.
  - **An invalid `proxy` value fails closed:** requests fail with a proxy error until it is fixed, and a `WARN` names the problem. Nothing goes out unproxied by mistake.
- **The user agent** is `<Name>/<version> (<os>; <arch>)`, from the product. The engine hardcodes no name.
- `network` is a section name an extension cannot take, like `yard` and `extensions`.

## Who uses it

- **The startup** builds the client from the product and the `network` section, and gives it to the shell and to GPUI (`cx.set_http_client`).
- **`yard_update`** gets the same client tagged `update`, instead of building its own. Its rules do not change: no redirect, HTTPS only (loopback allowed in tests), and its size and time limits.

## Logging

- **`http-configure{proxy, tls}`:** on start and on every change. `proxy` gives only the scheme, host, and port, never the username or password. `tls` is `platform`, or `bundled` when the platform verifier failed to start, in which case it is a `WARN`.
- **`http-request{caller, method, host}`:** around each request, with `status` and `elapsed_ms` at `debug`.
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

  It depends on Zed's `http_client`, `reqwest_client`, and `http_client_tls`, all Apache-2.0; no GPUI.
- **`yard_shell/http`** (`crates/yard_shell/src/dev/http.rs`): `set_http_client`, and `Extension::http_client`. Until a client is set, it returns a client that refuses every request, with one `ERROR`, as `yard_shell/data` does.
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

## Not in this slice

- Reading the macOS and Windows system proxy settings, and PAC files. Only the setting and the environment are read. `TODO(http-client-system-proxy)`.
- Proxy authentication prompts. Credentials can be put in the proxy URL.
- A per-extension allow-list of hosts, as Tauri's scope has.
- Moving `yard_update`'s own rules into the client.
