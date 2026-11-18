# Packaging feature 4: update-check

## What it adds

The application asks an address named by the product config for the newest version, and tells the user with a toast when there is a newer one, as Zed's `auto_update` polls its release server. Nothing is downloaded yet: that is feature 5.

## How it works

- **The config.** `product.toml` can name `update_url`, the address of a small JSON file describing the newest release. Without it the application never checks (`debug` `no update address`): riso's own config names none, since riso has no release server yet. `build.rs` checks it when present: an `https://` URL, or `http://` only to `127.0.0.1` or `localhost`, for checks on this machine. It gives `PRODUCT_UPDATE_URL`, empty when absent.
- **The release file.** JSON: `{"version": "0.2.0", "url": "<download address>", "sha256": "<hex>"}`. This feature reads `version`; `url` and `sha256` are read by feature 5, and must be present, so that a release file this feature accepts is one feature 5 can install. Unknown fields are allowed.
- **When.** Once shortly after the first window opens (5 s, so that the start is not slowed), then every 24 hours while the application runs. One check at a time.
- **Comparing.** The versions are compared as semantic versions (`semver`), against the product's version. Newer: a toast. The same or older: nothing for the user, a `debug` line. A version that cannot be read is a failure.
- **The toast.** An information toast of the window engine on the active window: `<name> <version> is available`. It is shown once per version per run: the 24-hour check does not repeat it for the same version.
- **Failures** (no network, an error status, a timeout of 30 s, a broken file): a `WARN` with the reason, no toast, and the next check at its time. Never an `ERROR`: the network is not the application's bug.
- **Off the main thread.** The request runs on the background executor through Zed's `http_client` with `reqwest_client` (Apache-2.0, at the same revision as `gpui`), inside its group.

## Where the code lives

A new core crate, `riso_update`, with its first part `riso_update/check` in dev. It depends on `riso_product`, `riso_shell` (for the toast), `http_client`, `reqwest_client`, and `semver`, and is started by the startup:

```rust
// riso_update::dev
/// Starts checking `url` for a version newer than `product`'s, showing its
/// toast through `extension`.
pub fn start(cx: &mut App, extension: &riso_shell::Extension, product: riso_product::Product, url: &str);
```

A toast is shown through an extension handle, as every toast is, so the startup creates a handle named `update` for it, after the listed extensions.

## Logging

| Situation | Log |
|---|---|
| A check | A group `update-check{current}` holding `debug` `newer {version}`, `up to date {version}`, or the `WARN` with its reason |
| The toast | `debug` `toast shown {version}`, or `debug` `already shown {version}` |
| No address | `debug` `no update address`, once at start |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_update/check` (new crate, dev) | The checks, the comparison, the toast |
| `riso/startup` | Dev copy that calls `riso_update::dev::start` when `PRODUCT_UPDATE_URL` is not empty. While `update-check` is under review, `cargo run dev` loads `review` on it |
| App build script (no stage) | `update_url`, `PRODUCT_UPDATE_URL` |
| Workspace | `http_client`, `reqwest_client` from Zed's repository at `gpui`'s revision; `semver` |

## Tests

- **Build script:** `update_url` absent, `https://`, `http://127.0.0.1:…`, and refused `http://example.com`, `ftp://…`, an unreadable URL.
- **`riso_update`, GPUI tests with a fake HTTP client:** a newer version shows one toast; the same and an older version show none; a second check of the same version does not show it again; a failure status, a broken file, a missing `url` or `sha256`, and an unreadable version each give a `WARN` and no toast; the first check waits 5 s and the next 24 hours (with the test clock); no check runs while one is running.
- **Logging**, alone in its file: `update-check{current}` with `newer` and with the `WARN`; `toast shown`.

## Check by the lead

No test extension. With a config naming `riso-check` and `update_url = "http://127.0.0.1:8765/latest.json"`, and `python3 -m http.server 8765` serving a `latest.json` in a temporary folder:

| Command and action | What to look for |
|---|---|
| `latest.json` says `0.2.0`; `RISO_CHECK_HOME=/tmp/riso-update PRODUCT_CONFIG=/tmp/riso-check.toml cargo run dev` | After about 5 s, the toast `riso-check 0.2.0 is available`; `update-check{current=0.1.0}` with `newer {version=0.2.0}`. |
| `latest.json` says `0.1.0` | No toast; `up to date {version=0.1.0}`. |
| The server stopped | No toast; the `WARN` with the reason. |

## Under review

- `riso_update/check`, and `riso/startup` in its dev version

## Not in this feature

- Downloading and installing: feature 5, `update-install`.
- A setting to turn checks off, and a menu item to check now: by need.
