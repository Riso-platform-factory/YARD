# Slice 1 of the release system: packaging

The first slice of `docs/platform/release-system.md`. A developer's `product.toml` goes in. An installable application for macOS, Linux, or Windows comes out, and it runs there the way it runs on macOS today. Nothing is released yet.

Everything here follows Zed, except where it says otherwise and why. The configs in this slice are test inputs: YARD packages nothing real.

## What the developer writes

The keys stay the ones `product.toml` has today. What changes is that every system reads them:

```toml
name = "Lab Notes"               # the app's name on every system
version = "0.3.0"
identifier = "test.yard.labnotes" # reverse-domain: bundle id, Wayland app_id, Windows AppUserModelID
url_scheme = "labnotes"
icon = "icon.png"                 # one square PNG, at least 512 × 512; every icon format is made from it
```

`PRODUCT_CONFIG=<path>` still names another file at build time.

## The parts

### 1. `yard_build_config`: the config, typed

- **Reading.** A new engine crate, without GPUI. It reads `product.toml` into a Rust type with `serde` and `deny_unknown_fields`. It then checks today's rules:
  - non-empty single-line strings;
  - the reverse-domain identifier;
  - the lowercase URL scheme;
  - a square PNG of at least 512 pixels;
  - the update address.
- **Errors.** Each refusal names the file and the key.
- **One home for each rule.** `build.rs` reads only through the crate. The identifier rule lives in `yard_product` (`is_identifier`), and the crate calls it, so the build and the running app accept the same identifiers.
- **Who reads it.** `build.rs` and `xtask` both read the config through this crate. The script that read TOML on its own is deleted.

### 2. `cargo xtask bundle <macos|linux|windows> [--config <file>] [--out <folder>]`

- **The tool.** `tooling/xtask`, with the alias `cargo xtask` in `.cargo/config.toml`, as in Zed.
- **What it does.** It builds the production binary with `PRODUCT_CONFIG` set to the config. It makes every icon format from the one PNG (YARD's own choice: Zed commits its icons in each format). It then lays out the package below.
- **Kept from `script/bundle-macos`:**
  - the review bundle (`--dev --env KEY=VALUE`), with its separate name, identifier, and scheme;
  - the stages check before building;
  - the executable path Cargo reports;
  - restoring the previous package when a replacement fails.

| System | What comes out (`<out>/`) | From Zed |
|---|---|---|
| macOS | `<Name>.app`, signed ad hoc, and `<Name>-<version>.zip` made with `ditto` | `script/bundle-mac` |
| Linux | `<name>-<version>-linux-<arch>.tar.gz`, holding `<name>.app/` (see below) | `script/bundle-linux` |
| Windows | `<Name>-<version>-<arch>-setup.exe` made by Inno Setup, and `<Name>-<version>-windows-<arch>.zip` | `script/bundle-windows.ps1`, `zed.iss` |

**The Linux package.** Zed's layout and install script:

```text
<name>.app/
  bin/<name>                                   the app
  share/applications/<identifier>.desktop      Name, Exec=<name> %U, Icon, StartupNotify,
                                               MimeType=x-scheme-handler/<url_scheme>;
  share/icons/hicolor/{256,512}x{256,512}/apps/<identifier>.png
  install.sh                                   copies <name>.app to ~/.local, links ~/.local/bin/<name>,
                                               puts the .desktop file in ~/.local/share/applications with
                                               absolute paths, and runs update-desktop-database when present
```

- It is built for the runner's `x86_64-unknown-linux-gnu` on Ubuntu 22.04, the oldest GitHub-hosted image. That image sets the oldest glibc the app runs on.
- It is stripped of debug data, as Zed's is.
- `install.sh` sits inside the archive. Zed's `install.sh` is downloaded from Zed's server, and a YARD app has no server, so the script ships with the package.

**The Windows installer.** Zed's `zed.iss`, cut to what YARD has:

- **Install location.** A per-user install (`PrivilegesRequired=lowest`) to `%LOCALAPPDATA%\Programs\<Name>`, with a Start-menu shortcut whose AppUserModelID is the identifier.
- **The URL scheme, in the registry:** `HKCU\Software\Classes\<url_scheme>` with `URL Protocol`, `DefaultIcon`, and `shell\open\command "<app>.exe" "%1"`.
- **The installer's own settings:**
  - `AppId` is derived from the identifier.
  - `AppMutex` names the same mutex the app creates. Both take it from one function, unlike Zed, whose installer names a different one.
  - Uninstall removes all of the above.
- **The executable.** The `.exe` carries the icon and its version through a resource file made by `build.rs` (Zed's `windows_resources`). In a production build it runs without a console (`windows_subsystem = "windows"`).
- **Not copied from Zed:** file associations, context menus, PATH, a WSL wrapper.
- Signing comes with the `code-signing` slice.

### 3. The app on Linux and Windows

These change engines that are in production today. Each change goes to the user for approval before it is stamped again.

- **Window controls (`yard_ui`'s title bar).** They are drawn as Zed's `platform_title_bar` draws them:
  - **Linux.** Client-side decorations: minimize, maximize, and close in the desktop's button layout. Double-click maximizes, right-click opens the window menu, and the edges resize, as Zed's `client_side_decorations` does.
  - **Windows.** The three caption buttons are drawn with Segoe Fluent Icons, and GPUI's `WindowControlArea` hit-testing does the rest.
  - **The GPL boundary.** What is copied from Zed's GPL crates (`platform_title_bar`, `workspace`) stays inside `yard_ui`, with its notices, as the GPL boundary requires.
- **Window options.**
  - `app_id` is the identifier, so Wayland and X11 match the window to its `.desktop` file.
  - The X11 icon comes from the config.
  - On Windows, `cx.set_app_identity` uses the identifier, as the shortcut does.
- **One instance (`yard_shell`).** A second start hands its arguments to the first, which brings its window to the front, then the second exits:
  - **Linux:** a Unix datagram socket in the app's data directory. A stale socket is removed.
  - **Windows:** a named mutex and a named pipe, both named from the identifier.
  - **No separate CLI binary** (YARD's own choice). Zed sends through a separate `cli` binary; YARD's app does it itself.
- **URLs.** GPUI does not deliver `on_open_urls` on Linux or Windows. The URL arrives as an argument, of the first start or forwarded by a second one. It goes to the same handler that macOS's `on_open_urls` feeds, so an extension's `on_open_url` works the same on the three systems.
- **Directories (`yard_paths`), as Zed's `paths`:**
  - **Linux:** `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`, `$XDG_STATE_HOME`, and `$XDG_CACHE_HOME`, each with `/<name>`.
  - **Windows:** `%APPDATA%\<Name>` for settings, and `%LOCALAPPDATA%\<Name>` for data, logs, and cache.
- **Updates.** On Linux and Windows, `yard_update` compiles and does nothing, with a `debug` line, until the `release-updates` slice. The macOS-only code (`renamex_np`, `ditto`, `codesign`) is put behind `cfg(target_os = "macos")`.

## Configs under test

The configs live in `crates/yard_build_config/tests/configs/`, where `xtask` reads them too. Every one is a test input, and each is a different app:

| Config | What it proves |
|---|---|
| `plain` | Name `Lab`, identifier `test.yard.lab`, scheme `yardlab`: the basic case |
| `spaced` | Name `Lab Notes Pro`, with spaces, in the bundle, the `.desktop` file, the installer, the shortcuts, and the directories |
| `other` | Another identifier, scheme, and icon: two apps installed side by side do not meet (directories, socket, mutex, scheme) |
| `bad-*` | A missing key, a bad identifier, an icon that is not square or too small, an unknown key: each refused, naming the file and the key |

## How it is checked

**Tests, in the repository:**
- `yard_build_config`: every refusal.
- `xtask`: the package laid out from each config, with no build, on any system. The `Info.plist` values, the `.desktop` fields, the `.iss` values, the Linux layout, and the icon sizes are each compared with the config.
- The engines: the instance socket and pipe; the URL forwarded from a second start to the handler; the directories per system.

**On GitHub Actions runners, in a throwaway repository** (`macos-latest`, `ubuntu-22.04`, `windows-latest`), for `plain`, `spaced`, and `other`:

| | What runs |
|---|---|
| macOS | Bundle, then `codesign --verify`, then `plutil` against the config; open the app and take a screenshot; `open <scheme>://…` reaches the extension |
| Linux | Bundle, then `desktop-file-validate`; run `install.sh`, then start the app under Xvfb and take a screenshot; `xdg-open <scheme>://…` reaches the running instance (read in its log); a second start exits and the first one logs the forwarded arguments |
| Windows | Bundle; install silently (`/VERYSILENT`), then read the registry keys; start the app and take a screenshot; `start <scheme>://…` reaches the running instance; uninstall silently and check that the keys are gone |

The lead reads every log and screenshot, and runs the macOS bundles locally too.

**Not checked by a machine:** how the window controls look next to Zed's on Linux and Windows. The lead compares the runners' screenshots with Zed's own on each system.

## Order inside the slice

Each part goes up for the user's review on its own:

1. `yard_build_config`, and `build.rs` reading through it.
2. `cargo xtask bundle macos`, replacing `script/bundle-macos`, with the configs under test.
3. The app on Linux, then `bundle linux`.
4. The app on Windows, then `bundle windows`.
5. The runner checks above, on the throwaway repository.

## Not in this slice

- Releasing, CI in the template, channels, updates on Linux and Windows, code signing, the DMG, ARM builds for Linux and Windows: later slices.
- Flatpak, `.deb`, AppImage, MSI. Zed ships a tarball and an Inno Setup installer; others can be added later.
