# Packaging feature 5: update-install

## What it adds

When feature 4 finds a newer version, the application downloads it, checks it, and puts it in place of the running bundle; the new version runs at the next start, and a toast offers to restart now. As Zed's `auto_update` downloads a release, checks it, and replaces the running application on macOS (`install_release_macos`, as reference: Zed uses a `.dmg` and `rsync`; riso uses a `.zip` and two renames).

## How it works

- **The release.** The release file's `url` names a `.zip` holding `<name>.app`, as `ditto -c -k --keepParent` makes it, and `sha256` its digest in hex. `script/bundle-macos --zip` writes `<out>/<name>-<version>.zip` beside the bundle and prints its `sha256`, so that a release is made with the same command.
- **Only from a bundle.** The running executable must be inside `<something>.app/Contents/MacOS/`, in a folder the user can write. Otherwise (`cargo run`, a read-only `/Applications`) nothing is downloaded, with a `debug` `not installable {reason}` once: the toast of feature 4 still tells the user.
- **Download.** Started by feature 4 when it finds a newer version, once per version per run; on the background executor, streamed to a file in a temporary folder beside the bundle, at most 1 GiB, within 10 minutes. The digest is computed while downloading; a different digest stops it.
- **Checks before installing**, each a failure that leaves the running bundle as it was:
  - the digest is the release's `sha256`;
  - the `.zip` unpacks (`ditto -x -k`) to exactly one `<name>.app`;
  - its `Info.plist` has the running bundle's `CFBundleIdentifier`, and its `CFBundleShortVersionString` is the release's `version`;
  - `codesign --verify --deep --strict` passes on it.
- **Installing.** Two renames on the same volume: the running bundle to `<name>.app.old` beside it, the new one into its place; then the old one is removed. If the second rename fails, the first is undone. macOS lets a running application's bundle be renamed: the running process keeps its files until it ends.
- **The toast.** After installing: `<name> <version> is installed`, with a button `Restart`, which restarts the application (`gpui`'s `restart`). Without a click, the new version runs at the next start.
- **Failures** are `WARN`s with the reason, never an `ERROR`, and the temporary folder is removed. The next check of feature 4 tries again.
- **Privacy.** As in feature 4, the logs name the download address's host only.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_update/install` (new, dev) | Download, checks, the renames, the toast |
| `riso_update/check` dev version | Starts the download when it finds a newer version, and passes the release's `url` and `sha256` |
| `riso/startup` | Dev copy that starts the dev check |
| `script/bundle-macos` (no stage) | `--zip` |
| Workspace | `sha2` |

```rust
// riso_update::dev
pub fn start(cx: &mut App, extension: &riso_shell::Extension, product: riso_product::Product, url: &str, client: Arc<dyn http_client::HttpClient>);
/// The bundle to replace, from the running executable, or why there is none.
pub fn running_bundle() -> Result<PathBuf, String>;
```

## Logging

| Situation | Log |
|---|---|
| Not installable | `debug` `not installable {reason}`, once |
| Download | A group `update-download{version, host}`, `debug` `downloaded {bytes}` |
| Install | A group `update-install{version}`, `debug` for each check and rename, `debug` `installed` |
| Restart | `debug` `restart` |
| A failure | `WARN` with the reason, in its group |

## Tests

- **`riso_update`, tests with a fake HTTP client and temporary folders holding small fake bundles** (an `Info.plist` and a script executable, signed ad hoc by the test with `codesign`): a good release is installed in place and the old bundle is gone; a wrong digest, a `.zip` with two apps or none, another identifier, another version, an unsigned bundle, a body over the limit each leave the running bundle untouched, remove the temporary folder, and `WARN`; a failing second rename puts the old bundle back; not a bundle and a read-only folder give `not installable`; one download per version per run.
- **Logging**, alone in its file: `update-download{version, host}` with `downloaded`, `update-install{version}` with `installed`, the `WARN`.

## Check by the lead

With `python3 -m http.server` on `127.0.0.1:8765` in a temporary folder:

1. A config naming `riso-check`, version `0.1.0`, `update_url = "http://127.0.0.1:8765/latest.json"`: `script/bundle-macos --dev --env RISO_CHECK_HOME=/tmp/riso-install --config … --out /tmp/riso-apps` builds `riso-check.app` 0.1.0 with the code under review.
2. The same config with version `0.2.0`: `script/bundle-macos --dev --env RISO_CHECK_HOME=/tmp/riso-install --zip --config … --out /tmp/upd-srv` makes `riso-check-0.2.0.zip`; `latest.json` names it and its `sha256`.
3. `open /tmp/riso-apps/riso-check.app`: after about 5 s the toast `riso-check 0.2.0 is installed` with `Restart`; `Info.plist` in `/tmp/riso-apps/riso-check.app` now says `0.2.0`.
4. Click `Restart`: the log's `run-start` says `riso-check 0.2.0`.
5. A `latest.json` with a wrong `sha256`: no install, the `WARN`, the bundle unchanged.

After the promotion, steps 1 to 4 are run once more with production bundles (without `--dev`).

## Under review

- `riso_update/install`, and `riso_update/check`, `riso/startup` in their dev versions

## Not in this feature

- Signing with a developer identity and notarizing: later. An ad hoc signature is enough for this machine.
- Updates for Linux and Windows.
