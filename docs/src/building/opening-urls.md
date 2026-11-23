# Opening URLs

Every YARD application declares a custom `url_scheme` in [`product.toml`](./product-config.md). Opening a URL with that scheme starts the application or forwards the URL to the already running instance, then routes it to the extension named by the URL's host.

## Routing URLs to Extensions

The URL's host (matched case-insensitively) identifies the extension:

```text
<url_scheme>://<extension_name>/<path>?<query>
```

For example, `yard://my_ext/open/123` is delivered whole to the extension named `my_ext`.

Register a handler once in the extension's `init`:

```rust
ext.on_open_url(cx, |url, cx| {
    // runs on the main thread with the full URL string
});
```

- An extension can register only one handler; a second call to `on_open_url` logs an `ERROR`.
- An extension receives only URLs whose host matches its own name.
- Application startup configures the product's scheme with `yard_shell::set_url_scheme(cx, scheme)` and passes incoming URL batches to `yard_shell::open_urls(cx, Vec<String>)`. URLs that arrive before extension loading finishes are queued and delivered in order as soon as startup calls `yard_shell::extensions_loaded(cx)`.

### Refused URLs

Each invalid or unroutable URL is refused with an `INFO` log inside its `url-open` trace group:

- Malformed URL (`invalid URL`)
- Scheme does not match `PRODUCT_URL_SCHEME` (`wrong scheme`)
- Missing host (`no host`)
- Host does not name a loaded extension (`no such extension`)
- Target extension registered no handler (`no handler for extension <host>`)

## Platform Delivery and Single Instance

Bundled applications register `url_scheme` with the operating system automatically during [`cargo xtask bundle`](./bundling.md):

- **macOS:** `Info.plist` declares `CFBundleURLTypes`. macOS keeps a single app instance and delivers URLs through GPUI's `on_open_urls`. (Unbundled `cargo run` binaries do not receive macOS URL events.)
- **Linux:** The bundled `.desktop` file registers the scheme, and `yard_shell::instance` enforces a single instance using a Unix datagram socket (`<data>/instance/instance.sock` and `instance.lock`). A second launch encodes any command-line arguments matching `url_scheme` (UTF-8, NUL-delimited, version `1`, up to `64 KiB`), sends the datagram to the running instance, and exits `0` immediately without waiting for the receiver to accept or process it. The second process exits `1` only if encoding or forwarding the request fails. The running instance delivers received URLs via `open_urls` and activates its main window.
- **Windows:** The Inno Setup installer registers the scheme in the registry, and `yard_shell::instance` enforces a single instance using a named mutex and named pipe derived from `yard_product::instance_name(identifier)`. When `<PREFIX>_HOME` is set, an FNV-1a hash of the data directory is appended so isolated runs do not collide with an installed copy.

## Privacy in Logs

URLs may carry authentication tokens or other secrets. YARD's logs never record a URL's path, query string, or non-URL command-line arguments:

- `url-open` trace groups record `scheme` and, only when the host matches a registered extension, `host`.
- Unknown hosts and non-URL arguments are omitted from log fields (only counted).
