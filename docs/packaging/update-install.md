# Packaging feature 5: update-install

## What it adds

When feature 4 finds a newer version, the application downloads it, checks it, and puts it in place of the running bundle; the new version runs at the next start, and a toast offers to restart now. As Zed's `auto_update` downloads a release, checks it, and replaces the running application on macOS (`install_release_macos`, as reference: Zed uses a `.dmg` and `rsync`; yard uses a `.zip` and one atomic exchange).

## How it works

- **The release.** The release file's `url` names a `.zip` holding `<name>.app`, as `ditto -c -k --keepParent` makes it, and `sha256` its digest in hex. `script/bundle-macos --zip` writes `<out>/<name>-<version>.zip` beside the bundle and prints its `sha256`, computed before the `.zip` is moved into place, so that a release is made with the same command.
- **Only from a bundle.** The running executable must be inside `<something>.app/Contents/MacOS/`, in a folder the user can write. Otherwise (`cargo run`, a read-only `/Applications`) nothing is downloaded, with a `debug` `not installable {reason}` once: the toast of feature 4 still tells the user.
- **Download.** Started by feature 4 when it finds a newer version, once per version per run, and only if the bundle on disk is older than the release (a bundle already replaced by a newer one is not replaced again). The release's `url` follows `update_url`'s rule: `https://`, or `http://` only to `127.0.0.1` or `localhost`. On the background executor, streamed to a file in a private temporary folder (`tempfile`, owner-only) beside the bundle, at most 1 GiB, within 10 minutes: the time limit covers the download only, never the steps after it. The digest is computed while downloading; a different digest stops it.
- **Checks before unpacking.** The `.zip`'s entries are listed first (`zipinfo -1`): an absolute path, a `..` component, an entry outside `<name>.app/`, or more than 2 GiB once unpacked stops it.
- **Checks before installing**, each a failure that leaves the running bundle as it was:
  - the digest is the release's `sha256`;
  - the `.zip` unpacks (`ditto -x -k`) to exactly one real folder named `<name>.app`, and nothing else;
  - no symbolic link inside it points outside it;
  - its `Info.plist` has the running product's identifier and the release's `version`, both read as strings (`plutil -extract … raw -expect string`);
  - `/usr/bin/codesign --verify --deep --strict` passes on it.
- **Installing.** One atomic exchange on the same volume: `renamex_np` with `RENAME_SWAP` swaps the new bundle and the running one, so that `<name>.app` is a complete bundle at every moment, even if the process is killed; then the old one, now in the temporary folder, is removed with it. macOS lets a running application's bundle be swapped: the running process keeps its files until it ends.
- **The toast.** After installing: `<name> <version> is installed`, with a button `Restart`, which restarts the application (`gpui`'s `restart`). Without a click, the new version runs at the next start.
- **Failures** are `WARN`s with the reason, never an `ERROR`, and the temporary folder is removed. The next check of feature 4 tries again.
- **Privacy.** As in feature 4, the logs name the download address's host only.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_update/install` (new, dev) | Download, checks, the exchange, the toast |
| `yard_update/check` dev version | Starts the download when it finds a newer version, and passes the release's `url` and `sha256` |
| `yard/startup` | Dev copy that starts the dev check |
| `script/bundle-macos` (no stage) | `--zip` |
| Workspace | `sha2` |

```rust
// yard_update::dev
pub fn start(cx: &mut App, extension: &yard_shell::Extension, product: yard_product::Product, url: &str, client: Arc<dyn http_client::HttpClient>);
/// The bundle to replace, from the running executable, or why there is none.
pub fn running_bundle() -> Result<PathBuf, String>;
/// Downloads, checks, and installs a release over `bundle`, as the running
/// application does after it finds a newer version; tests call it directly.
pub async fn install(client: Arc<dyn http_client::HttpClient>, bundle: PathBuf, name: &str, identifier: &str, version: &str, release_url: &str, sha256: &str) -> Result<(), String>;
```

## Logging

| Situation | Log |
|---|---|
| Not installable | `debug` `not installable {reason}`, once |
| Download | A group `update-download{version, host}`, `debug` `downloaded {bytes}` |
| Install | A group `update-install{version}`, `debug` for each check and the exchange, `debug` `installed` |
| Restart | `debug` `restart` |
| A failure | `WARN` with the reason, in its group |

## Tests

- **`yard_update`, tests with a fake HTTP client and temporary folders holding small fake bundles** (an `Info.plist` and a script executable, signed ad hoc by the test with `codesign`): a good release is installed in place and the old bundle is gone; a wrong digest, a `.zip` with two apps or none, another identifier, another version, an unsigned bundle, a body over the limit each leave the running bundle untouched, remove the temporary folder, and `WARN`; a failing exchange leaves the running bundle as it was; a `.zip` with `..`, an absolute path, an extra entry beside the app, or a symbolic link pointing outside is refused before anything is installed; an `http://` release address to another host is refused; a bundle on disk newer than the release is not replaced; not a bundle and a read-only folder give `not installable`; one download per version per run.
- **Logging**, alone in its file: `update-download{version, host}` with `downloaded`, `update-install{version}` with `installed`, the `WARN`.

## Check by the lead

With `python3 -m http.server` on `127.0.0.1:8765` in a temporary folder:

1. A config naming `yard-check`, version `0.1.0`, `update_url = "http://127.0.0.1:8765/latest.json"`: `script/bundle-macos --dev --env YARD_CHECK_HOME=/tmp/yard-install --config … --out /tmp/yard-apps` builds `yard-check.app` 0.1.0 with the code under review.
2. The same config with version `0.2.0`: `script/bundle-macos --dev --env YARD_CHECK_HOME=/tmp/yard-install --zip --config … --out /tmp/upd-srv` makes `yard-check-0.2.0.zip`; `latest.json` names it and its `sha256`.
3. `open /tmp/yard-apps/yard-check.app`: after about 5 s the toast `yard-check 0.2.0 is installed` with `Restart`; `Info.plist` in `/tmp/yard-apps/yard-check.app` now says `0.2.0`.
4. Click `Restart`: the log's `run-start` says `yard-check 0.2.0`.
5. A `latest.json` with a wrong `sha256`: no install, the `WARN`, the bundle unchanged.

After the promotion, steps 1 to 4 are run once more with production bundles (without `--dev`).

## Under review

- `yard_update/install`, and `yard_update/check`, `yard/startup` in their dev versions

## Not in this feature

- Signing with a developer identity and notarizing: later. An ad hoc signature is enough for this machine.
- Updates for Linux and Windows.
