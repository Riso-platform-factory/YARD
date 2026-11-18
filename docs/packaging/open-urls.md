# Packaging feature 3: open-urls

## What it adds

The product config names a URL scheme. Opening such a URL, from a browser or with `open riso://layout_tour/hello`, starts the application, or reaches the one that is running, and the extension the URL names handles it through its handle, as Zed handles `zed://` links through `on_open_urls`.

## How it works

- **The config.** `app/riso/product.toml` gains `url_scheme = "riso"`. `build.rs` checks it as a URL scheme (a lowercase ASCII letter, then lowercase letters, digits, `+`, `-`, or `.`) and gives `PRODUCT_URL_SCHEME`; a missing or invalid value fails the build with the file and the key, as feature 1's keys do.
- **The bundle.** `script/bundle-macos` writes `CFBundleURLTypes` into `Info.plist`, with the scheme and the identifier as its name, so that macOS knows the application opens it.
- **Which extension.** The URL's host names the extension: `riso://layout_tour/hello?x=1` goes to `layout_tour`, which receives the whole URL. An extension handles only the URLs that name it; it cannot ask for another's.
- **Through the handle.** `Extension::on_open_url(cx, handler)` registers the extension's handler, once; a second registration is refused with an `ERROR`, as feature 1 refuses a second state registration. The handler runs on the main thread with the URL as a string.
- **Refused URLs**, each with an `INFO` `refused {reason}` and nothing else: another scheme; no host; a host that is not a loaded extension; an extension without a handler; a URL that does not parse.
- **Before the extensions are loaded.** macOS can give the URL that started the application before the startup has loaded the extensions. The shell keeps such URLs and hands them over, in order, once the extensions are loaded.
- **The running application.** macOS gives a URL to the application that is running, so a second copy does not start; nothing more is needed in the application. `cargo run` is not a bundle, so macOS does not send it URLs: the checks use a bundle.
- **Privacy.** A URL can carry a token. The logs name the scheme and the host only, never the path or the query.

## What code can do

```rust
// riso_shell::dev
impl Extension {
    pub fn on_open_url(&self, cx: &mut App, handler: impl Fn(&str, &mut App) + 'static);
}
/// Called by the startup with the URLs macOS gives.
pub fn open_urls(cx: &mut App, urls: Vec<String>);
/// Called by the startup when the extensions are loaded.
pub fn extensions_loaded(cx: &mut App);
pub fn set_url_scheme(cx: &mut App, scheme: &str);
```

## Logging

| Situation | Log |
|---|---|
| URLs arrive | A group `urls-open{count}`, with a group `url-open{scheme, host}` for each |
| Kept until the extensions are loaded | `debug` `kept until extensions load`; when handed over, `debug` `handed over {count}` |
| Handled | `debug` `handled` in `url-open`, around the handler's own work |
| Refused | `INFO` `refused {reason}` |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` dev version | `on_open_url`, `open_urls`, `extensions_loaded`, `set_url_scheme`, the routing, keeping early URLs |
| `riso/startup` | Dev copy that registers `on_open_urls` before the application runs, gives the shell the scheme, and tells it when the extensions are loaded. While `open-urls` is under review, `cargo run dev` loads `review` and `layout_tour` on it |
| App build script, `product.toml`, `script/bundle-macos` (no stage) | `url_scheme`, `PRODUCT_URL_SCHEME`, `CFBundleURLTypes`; the script's `--dev` and `--env` |
| `examples/layout_tour` (no stage) | In the case `user-layer`, the controls show `url: <last URL>` once one arrives |

## Tests

- **Build script:** missing and invalid schemes refused with the key; a valid one accepted.
- **Shell, GPUI tests, with the dev shell:** a URL reaches the extension its host names, whole; another extension's handler is not called; each refusal with its reason; a second handler refused with an `ERROR`; URLs given before `extensions_loaded` are kept and handed over in order after it.
- **Logging**, alone in its file: `urls-open{count}`, `url-open{scheme, host}`, `handled`, `refused`; no line holds the URL's path or query.

## Test extension: `examples/layout_tour`

The checks need a bundle that runs dev code, even when macOS starts it for a URL, without arguments. `script/bundle-macos --dev [--env KEY=VALUE]…` builds one, for checking only: the binary is built with the dev features; the bundle's executable is a small script that runs it with `dev`; each `--env` goes into `Info.plist`'s `LSEnvironment`, so that macOS sets it however the bundle starts. It refuses to build a dev bundle named as `app/riso/product.toml`'s product, so that a dev bundle never takes the real name, directories, or scheme.

| Command and action | What to look for |
|---|---|
| `script/bundle-macos --dev --env RISO_CHECK_HOME=/tmp/riso-urls --env LAYOUT_TOUR=user-layer --config <a config naming riso-check, scheme riso-check>`, then `open dist/riso-check.app` | The tour's window, with its home in `/tmp/riso-urls`. |
| `open "riso-check://layout_tour/hello?token=secret"` | The controls show `url: riso-check://layout_tour/hello?token=secret`; no second copy starts; the log has `url-open{scheme=riso-check, host=layout_tour}` and `handled`, and no `secret`. |
| Quit, then `open "riso-check://layout_tour/first"` with the application closed | It starts and shows `url: riso-check://layout_tour/first`, with `kept until extensions load` and `handed over {count=1}`. |
| `open "riso-check://nobody/x"` | `refused {reason=no extension nobody}`. |

## Under review

- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Opening files and folders, and a command-line launcher: later, as the roadmap says.
