# Packaging feature 4: update-check

## What it adds

The application asks an address named by the product config for the newest version, and tells the user with a toast when there is a newer one, as Zed's `auto_update` polls its release server. Nothing is downloaded yet: that is feature 5.

## How it works

- **The config.** `product.toml` can name `update_url`, the address of a small JSON file describing the newest release. Without it the application never checks (`debug` `no update address`): yard's own config names none, since yard has no release server yet. `build.rs` checks it when present: an `https://` URL, or `http://` only to `127.0.0.1` or `localhost`, for checks on this machine. When `update_url` is present, `version` must be a semantic version (`0.1.0`, not `1.0`), since it is compared with the release's: the build fails otherwise, with the file and the key. It gives `PRODUCT_UPDATE_URL`, empty when absent.
- **The release file.** JSON: `{"version": "0.2.0", "url": "<download address>", "sha256": "<hex>"}`. This feature reads `version`; `url` and `sha256` are read by feature 5, and must be present, so that a release file this feature accepts is one feature 5 can install. Unknown fields are allowed.
- **When.** Once shortly after the first window opens (5 s, so that the start is not slowed), then every 24 hours while the application runs. One check at a time.
- **Comparing.** The versions are compared as semantic versions (`semver`) by precedence, against the product's version: build metadata (`+build.1`) does not make a version newer, and a pre-release (`0.2.0-beta`) is newer than `0.1.0` but older than `0.2.0`. Newer: a toast. The same or older: nothing for the user, a `debug` line. A version that cannot be read is a failure.
- **The toast.** An information toast of the window engine on the active window: `<name> <version> is available`. It is shown once per version per run: the 24-hour check does not repeat it for the same version.
- **Failures** (no network, an error status, a timeout of 30 s, a broken file): a `WARN` with the reason, no toast, and the next check at its time. Never an `ERROR`: the network is not the application's bug.
- **Safe to ask.** The answer is read up to 64 KiB; a longer one is a failure. Redirects are not followed: a `3xx` is a failure, so that an `https` address can never lead to plain `http`. The logs name the address's host, never its path or query, which can hold a token.
- **Off the main thread.** The request runs on the background executor through Zed's `http_client` with `reqwest_client` (Apache-2.0, at the same revision as `gpui`), inside its group.

## Where the code lives

A new core crate, `yard_update`, with its first part `yard_update/check` in dev. It depends on `yard_product`, `yard_shell` (for the toast), `http_client`, `reqwest_client`, and `semver`, and is started by the startup:

```rust
// yard_update::dev
/// Starts checking `url` for a version newer than `product`'s, showing its
/// toast through `extension`.
pub fn start(
    cx: &mut App,
    extension: &yard_shell::Extension,
    product: yard_product::Product,
    url: &str,
    client: std::sync::Arc<dyn http_client::HttpClient>,
);
```

The startup gives the real client (`reqwest_client`); a test gives Zed's `http_client::FakeHttpClient` (feature `test-support`). A toast is shown through an extension handle, as every toast is, so the startup creates a handle named `update` for it, after the listed extensions.

## Logging

| Situation | Log |
|---|---|
| A check | A group `update-check{current}` holding `debug` `newer {version}`, `up to date {version}`, or the `WARN` with its reason |
| The toast | `debug` `toast shown {version}`, or `debug` `already shown {version}` |
| No address | `debug` `no update address`, once at start |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_update/check` (new crate, dev) | The checks, the version check, the toast |
| `yard/startup` | Dev copy that calls `yard_update::dev::start` when `PRODUCT_UPDATE_URL` is not empty. While `update-check` is under review, `cargo run dev` loads `review` on it |
| App build script (no stage) | `update_url`, `PRODUCT_UPDATE_URL` |
| Workspace | `http_client`, `reqwest_client` from Zed's repository at `gpui`'s revision; `semver` |

## Tests

- **Build script:** `update_url` absent, `https://`, `http://127.0.0.1:…`, and refused `http://example.com`, `ftp://…`, an unreadable URL.
- **`yard_update`, GPUI tests with a fake HTTP client:** a newer version shows one toast; the same and an older version show none; a second check of the same version does not show it again; a failure status, a broken file, a missing `url` or `sha256`, and an unreadable version each give a `WARN` and no toast; the first check waits 5 s and the next 24 hours (with the test clock); no check runs while one is running.
- **Logging**, alone in its file: `update-check{current}` with `newer` and with the `WARN`; `toast shown`.

## Check by the lead

No test extension. With a config naming `yard-check` and `update_url = "http://127.0.0.1:8765/latest.json"`, and `python3 -m http.server 8765` serving a `latest.json` in a temporary folder:

| Command and action | What to look for |
|---|---|
| `latest.json` says `0.2.0`; `YARD_CHECK_HOME=/tmp/yard-update PRODUCT_CONFIG=/tmp/yard-check.toml cargo run dev` | After about 5 s, the toast `yard-check 0.2.0 is available`; `update-check{current=0.1.0}` with `newer {version=0.2.0}`. |
| `latest.json` says `0.1.0` | No toast; `up to date {version=0.1.0}`. |
| The server stopped | No toast; the `WARN` with the reason. |

## Under review

- `yard_update/check`, and `yard/startup` in its dev version

## Not in this feature

- Downloading and installing: feature 5, `update-install`.
- A setting to turn checks off, and a menu item to check now: by need.
