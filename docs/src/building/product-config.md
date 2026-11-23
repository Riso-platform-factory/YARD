# Product Config

YARD engines never hardcode an application's name, version, file names, or environment variables. Every product-specific value comes from a build-time TOML file (`product.toml`) and is passed to the engines at runtime.

## `product.toml`

Place `product.toml` in the application's crate directory (for example `app/<name>/product.toml`, or `app/yard/product.toml` for the sample). `product.toml` is the single source of the product version; the crate's version in `Cargo.toml` is not read. Nothing sets `PRODUCT_CONFIG` by default; set `PRODUCT_CONFIG=<path>` in the environment to build with a different config file (as `cargo xtask bundle --config` does).

```toml
name = "yard"
version = "0.1.0"
identifier = "vn.risotech.yard"
url_scheme = "yard"
icon = "icon.png"
```

All top-level keys are validated strictly; unknown keys fail the build.

| Key | Required | Rules |
|---|---|---|
| `name` | yes | Non-empty string containing at least one ASCII letter or digit |
| `version` | yes | Non-empty version string; must be valid semver when `update_url` is set; on Windows major, minor, and patch must each be at most `65535` |
| `identifier` | yes | Reverse-domain identifier: at least two dot-separated labels of ASCII letters, digits, and `-`, not starting or ending with `-` |
| `url_scheme` | yes | Starts with a lowercase ASCII letter, followed by lowercase ASCII letters, digits, `+`, `-`, or `.` |
| `icon` | yes | Path relative to `product.toml` pointing to a square PNG at least `512 × 512` pixels |
| `update_url` | no | `https://` URL (or `http://` only for `127.0.0.1` or `localhost`) pointing to a release JSON file (see [Updates](./updates.md)) |
| `[release]` | no | Optional packaging and signing configuration (see [Bundling](./bundling.md#signing-and-notarization)) |

## Build Script (`yard_build_config`)

An application's `build.rs` is a single call:

```rust
fn main() {
    yard_build_config::build_script::run()
}
```

At build time it reads and validates `product.toml` (or `PRODUCT_CONFIG`), emits `cargo:rerun-if-changed` directives for the config and icon, and sets compile-time environment variables:

- `PRODUCT_NAME`
- `PRODUCT_VERSION`
- `PRODUCT_IDENTIFIER`
- `PRODUCT_URL_SCHEME`
- `PRODUCT_ICON` (absolute path to the PNG)
- `PRODUCT_UPDATE_URL` (empty string when `update_url` is omitted)

It also prepares platform icon assets in `OUT_DIR`:

- **Linux and FreeBSD:** writes a `256 × 256` `window_icon.png` for the runtime window icon.
- **Windows:** writes a multi-size `app.ico` (`16`, `24`, `32`, `48`, `64`, `256` px) and `app.rc`, and compiles and embeds the icon and version resource into the executable on Windows hosts only (cross-compiling for Windows from another host emits a Cargo warning and omits the resource).

## Product Identity and Derived Names (`yard_product`)

`yard_product::Product` holds `name`, `version`, and `identifier` without depending on GPUI. Every engine derives its names from `Product`:

- **Environment prefix (`Product::environment_prefix`):** `name` converted to uppercase with every non-ASCII-alphanumeric character replaced by `_` (for example, `"My App"` becomes `MY_APP`).
- **Home override variable (`Product::home_variable`):** `<PREFIX>_HOME`.
- **Lifecycle trace variable (`Product::lifecycle_variable`):** `<PREFIX>_LIFECYCLE`.
- **Diagnostic log filter variable:** `<PREFIX>_LOG`, constructed by `yard_log` from `product.environment_prefix()`.
- **Linux file and folder name (`Product::file_name` / `yard_product::file_name`):** `name` lowercased with runs of non-ASCII-alphanumeric characters collapsed into a single `-`.
- **Windows instance name (`yard_product::instance_name`):** derived from `identifier` for the single-instance mutex and named pipe.

`Product::new(name, version, identifier)` validates the identifier inside the `product` lifecycle trace group during startup.

### Side-by-Side Channels

To ship separate release channels side by side (for example, a preview channel alongside stable), give each channel its own product config with its own `name` (and distinct `identifier` and `url_scheme`). Because directories and log file names derive from `name`, each channel keeps separate configuration, data, and logs automatically.

## Application Directories

`yard_paths::Paths::from_env(&product)` resolves the three root directories of an application.

When `<PREFIX>_HOME` is set and non-empty (relative paths are resolved against the current working directory), it overrides all operating system defaults and XDG variables:

- Config: `<home>/config`
- Data: `<home>/data`
- Logs: `<home>/logs`

Otherwise, platform defaults apply:

| Directory | macOS | Linux / FreeBSD (`<name>` = `file_name`) | Windows (`XDG_*` ignored) |
|---|---|---|---|
| Config | `$XDG_CONFIG_HOME/<Name>` (if absolute) or `~/.config/<Name>` | `$XDG_CONFIG_HOME/<name>` or `~/.config/<name>` | `%APPDATA%\<Name>` |
| Data | `~/Library/Application Support/<Name>` | `$XDG_DATA_HOME/<name>` or `~/.local/share/<name>` | `%LOCALAPPDATA%\<Name>` |
| Logs | `~/Library/Logs/<Name>` | `<data>/logs` | `%LOCALAPPDATA%\<Name>\logs` |

### Files Owned by `yard_paths`

- **Config directory:** `settings.json`, `keymap.json`, `themes/` (`Paths::themes_dir()`)
- **Data directory:** `layout.json`, `layouts/<name>.json`, `window.json`, `session.json`, `state.json`, `store/<extension>.json`, `instance/instance.sock`, `instance/instance.lock`
- **Logs directory:** `<Name>.log`, `<Name>.log.old`, `lifecycle.jsonl`

`yard_paths::write_atomic(path, bytes) -> io::Result<()>` creates missing parent directories, writes to a temporary file in the target directory, syncs the file, renames it over the destination, and on Unix syncs the parent directory (skipped on Windows, where directories cannot be opened as files and NTFS journals the rename). Each call runs inside a `file-write{path}` trace group; failures log a `WARN` naming the path and error, remove the temporary file, and return the `io::Error` to the caller.

## Composition Root and Startup

The composition root wires the product config, startup, menus, and compiled-in extensions together. `yard_app` reads nothing at build time: the calling application constructs its `yard_app::Application` with `yard_app::application!()`, which expands in the application crate to read its `PRODUCT_*` variables and (on Linux and FreeBSD) embed `window_icon.png` from `OUT_DIR`, then passes it with the extension list and menu builder to `yard_app::run(application, extensions, menus)` (in the sample application `app/yard`, `yard::startup::application()` and `yard::startup::EXTENSIONS` provide this composition). `yard_app::run` initializes engines in order:

1. Starts logging (`yard_log::start`), validates `Product::new`, resolves `Paths::from_env`, and checks single-instance forwarding on Linux and Windows.
2. Opens log files (`yard_log::open_files`).
3. Initializes GPUI with `yard_ui::Assets`, initializes settings (`load` + `watch`), initializes themes (`load_user_themes(cx, &paths.themes_dir())` + `apply_theme_setting`), configures shell paths (`set_layout_file`, `set_named_layout_files`, `set_state_file`, `set_store_dir`), window title (`set_window_title`), and URL scheme (`set_url_scheme`), binds default shell and text-input keys, and installs the quit handler.
4. Opens the URLs that arrived before this point, loads every listed extension (`Extension::new(cx, name)`), opens the URLs that arrived while the extensions loaded, marks the extensions loaded (`extensions_loaded`, which delivers the URLs `open_urls` queued until then), loads and watches `keymap.json`, sets menus (`set_menus`), and registers the window opener (`set_window_opener`).
5. Restores windows from `session.json` (or opens one window from `window.json` and `layout.json`), starts the update check if `update_url` is set, and calls `yard_log::startup_finished()`.
