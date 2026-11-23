# Updates

`yard_update` checks an application's configured release feed for newer versions and, on macOS, downloads, verifies, and installs the update in place.

> **Platform support:** Automatic update checks and installation currently run on **macOS only**. On Linux and Windows, startup logs a `debug` message and skips update checking.

## Configuration and Release Feed

Enable update checks by setting `update_url` in [`product.toml`](./product-config.md):

```toml
version = "0.1.0"
update_url = "https://updates.example.com/latest.json"
```

- `update_url` must use `https://` (or `http://` only for `127.0.0.1` or `localhost` during local testing).
- When `update_url` is set, `version` must be valid semantic versioning (`semver`).
- Without `update_url`, update checking is disabled.

The release feed must return JSON with three required fields (unknown fields are ignored):

```json
{
  "version": "0.2.0",
  "url": "https://updates.example.com/myapp-0.2.0.zip",
  "sha256": "<64-character hex digest>"
}
```

Make a matching `.zip` and print its SHA-256 digest with [`cargo xtask bundle macos --zip`](./bundling.md#macos).

## Checking Schedule and Startup Integration

Application startup begins update checking with:

```rust
yard_update::start(cx, &extension, product, url, client);
```

where `extension: &yard_shell::Extension` shows update toasts, `product: yard_product::Product` supplies the product name, version, and identifier, `url: &str` is `PRODUCT_UPDATE_URL`, and `client: Arc<dyn http_client::HttpClient>` performs HTTP requests.

- The first check runs `5s` after startup so launch is not delayed; subsequent checks run every `24h` while the application stays open. Only one check runs at a time.
- Requests run on GPUI's background executor using Zed's `http_client` (`reqwest_client`), with a `30s` timeout, a `64 KiB` maximum response size, and redirects disabled (any `3xx` response fails).
- Versions are compared by `semver` precedence against the running `PRODUCT_VERSION` (build metadata is ignored; pre-releases precede their final release) and against versions already successfully installed during this run.
- Network errors, non-success statuses, timeouts, and invalid JSON log a `WARN` (never `ERROR`) and retry on the next 24-hour cycle. Logs record only the URL's host, never its path or query.
- Update toasts are shown in the active window, falling back to the first open window (if no window is open, a `debug` message is logged and no toast is shown).

## Downloading and Installing (macOS)

When a newer version is found and has not already been installed in this run, `yard_update` checks whether the running executable lives inside `<Name>.app/Contents/MacOS/` and whether the bundle's parent directory is writable (by creating and removing a temporary probe directory beside the bundle):

- **Not installable** (for example, running via `cargo run` or from a read-only folder): `not installable` is logged once per run at `debug`, nothing is downloaded, and an informational toast displays `<Name> <version> is available` once per version per run.
- **Installable:** `yard_update` downloads and installs the update in the background. Successfully installed versions are remembered for the rest of the run so they are not reinstalled on later checks; a failed download, verification, or swap is not recorded as installed and retries on the next check cycle:
  1. Validates that the download `url` uses `https://` (or loopback `http://`) and streams the archive to a private temporary directory beside the running bundle (max `1 GiB`, `10 min` download timeout, redirects disabled), computing SHA-256 while streaming. The `10 min` limit applies to downloading; verification and bundle replacement happen afterward.
  2. Verifies the SHA-256 digest matches the feed's `sha256`.
  3. Inspects archive entries with `zipinfo -1`: rejects absolute paths, `..` components, entries outside `<Name>.app/`, or unpacked size exceeding `2 GiB`.
  4. Unpacks with `ditto -x -k` and checks that it yields a single `<Name>.app` directory with no symlinks escaping the bundle.
  5. Verifies `Contents/Info.plist` via `plutil` (`CFBundleIdentifier` matches the running product and `CFBundleShortVersionString` matches the release `version`).
  6. Runs `/usr/bin/codesign --verify --deep --strict` on the unpacked bundle.
  7. Atomically swaps the new bundle into place with `renamex_np(..., RENAME_SWAP)` and removes the old bundle along with the temporary folder.
  8. Shows a toast `<Name> <version> is installed` with a **Restart** button that calls GPUI's `cx.restart()`. Without clicking **Restart**, the new version runs on the next launch.

Any download or verification failure logs a `WARN`, deletes the temporary directory, and leaves the running bundle untouched.

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `update-check` | `current` | One scheduled check against `update_url` |
| `update-download` | `version`, `host` | Downloading the release `.zip` archive |
| `update-install` | `version` | Verifying, unpacking, and swapping the macOS bundle |
