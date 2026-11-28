# autostart: start the application at login

Many desktop applications offer "Open at login": a mail client, a sync tool, a tray utility. Each system has its own place for this:
- the Login Items of System Settings on macOS;
- the `Run` key and Task Manager's Startup apps on Windows;
- the XDG autostart folder on Linux.

The user can see and change it there too.

This slice lets an extension read and change whether the application starts at login, through its handle. The entry is named after the application, never after YARD or an extension, so the user sees the application's own name. Zed has no such feature at rev `ddb90e5`. The design follows Electron's `setLoginItemSettings`, Tauri's autostart plugin and the `auto-launch` crate, and Apple's `SMAppService`. The sources are in the proposal.

## What an extension writes

```rust
match ext.launch_at_login(cx) {
    Ok(AutostartStatus::Enabled) => show_checked(true),
    Ok(AutostartStatus::NeedsApproval) => show_hint("Allow it in System Settings › Login Items"),
    Ok(_) => show_checked(false),
    Err(error) => show_error(error.to_string()),
}
ext.set_launch_at_login(true, cx)?; // returns the status after the change
```

- **`Extension::launch_at_login(cx: &App) -> Result<AutostartStatus, AutostartError>`** reads the current state.
- **`Extension::set_launch_at_login(enabled: bool, cx: &App) -> Result<AutostartStatus, AutostartError>`** changes it, and returns the state after the change. Asking for the state it is already in succeeds without changing anything.
- **There is one setting per application**, not per extension. Any extension may read or change it, for example a settings page or a tray menu. The group records which extension did.
- **Both calls run on the UI thread.** Each is one short system call, or one small file or registry write.
- **`AutostartStatus`** (`Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`):
  - `Enabled`;
  - `Disabled`;
  - `NeedsApproval`: the system knows the entry, but the user must allow it (macOS "requires approval"; Windows, turned off in Task Manager);
  - `NotSupported`: an unbundled macOS build, macOS before 13, or another system.
- **`AutostartError`** (`Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Display`, `Error`), with fixed texts that never hold a path:
  - `NotSupported` ("launch at login is not supported here");
  - `NeedsApproval` ("launch at login needs the user's approval in the system settings");
  - `Failed` ("launch at login failed").

## Each system

- **macOS 13 and later:** `SMAppService.mainApp`, called through `objc2` (already in `Cargo.lock`), with no new crate.
  - The status maps as: `enabled` is `Enabled`; `notRegistered` and `notFound` are `Disabled`; `requiresApproval` is `NeedsApproval`. A bundled application that has never been registered reads `notFound` (checked on macOS 26.6 with a review bundle), and `register` then succeeds. Any other value is `Failed`.
  - Enabling calls `register`. "Already registered" is success, and "denied by the user" is `NeedsApproval`. When the status is already `requiresApproval`, enabling returns `NeedsApproval` without calling `register`: only the user can approve it, in System Settings.
  - Disabling calls `unregister`. "Not registered" is success.
  - The application must run from its `.app` bundle: an executable whose canonical path is outside `<Name>.app/Contents/MacOS/` (`cargo run`) gives `NotSupported` without touching the system's login items. If the path cannot be resolved, the result is `Failed`, also without touching them.
  - Before macOS 13, `NotSupported`.
- **Windows:**
  - Enabling writes the value `<identifier>` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, set to the quoted path of the running executable. Disabling deletes it; a missing value is success.
  - If Task Manager's `StartupApproved\Run` value for `<identifier>` says the user turned it off, the status is `NeedsApproval`, and enabling returns the error `NeedsApproval`. The application never changes `StartupApproved\Run`:
    - with the `Run` value present, enabling changes nothing;
    - with it missing (after disabling or reinstalling), enabling writes it again, so the application reappears in Task Manager for the user to turn on, and returns `NeedsApproval`.
  - The command is at most 260 characters, Windows' limit for `Run`. A longer one is `Failed`, with nothing written.
  - The Windows installer that `cargo xtask bundle windows` makes deletes the `Run` value when the application is uninstalled.
- **Linux:**
  - Enabling writes `$XDG_CONFIG_HOME/autostart/<identifier>.desktop` (default `~/.config/autostart/`) atomically, through `yard_paths`. It holds `Type=Application`, `Name=<product name>`, `Exec=` with the quoted path of the running executable, and `X-GNOME-Autostart-enabled=true`.
  - Disabling removes the file; a missing file is success.
  - Only a file with a `[Desktop Entry]` group (an optional UTF-8 BOM before it is ignored), `Type=Application`, and an `Exec` key reads as `Enabled`, unless it has `Hidden=true` or `X-GNOME-Autostart-enabled=false`.
  - An empty, malformed, or unreadable file reads as `Disabled`, and enabling replaces it. Malformed means: a line that is not blank, a comment, a group header, or `key=value` with a valid key name (letters, digits, `-`, and an optional `[locale]`); or an `Exec` that is empty or has an unclosed quote.
  - Disabling leaves a file that is already hidden or disabled as it is, so it never uncovers a system-wide entry of the same name in `/etc/xdg/autostart/`.
  - An executable path the Desktop Entry spec cannot hold in `Exec` (one with `=`) is `Failed`, with nothing written.
- **Elsewhere:** reading gives `NotSupported`, and changing gives the error `NotSupported`.

## Setup and tests

- **`yard_shell::set_autostart_backend(cx, AutostartBackend::system(identifier, product_name))`** is called once at startup. Before it, both calls return `Err(Failed)` with one `ERROR` per application.
- **`AutostartBackend::memory()`** is for tests. It starts `Disabled`, follows `set_launch_at_login`, and offers `set_status(AutostartStatus)` and `fail_next(AutostartError)`. Clones share one state.
- **While under review**, `review.rs` sets the system backend. **At promotion:**
  - `yard_app/startup` sets it from the product;
  - `yard_test/harness` sets a memory backend through `yard_app::Backends`, a new `autostart_backend` field.

## Logging

- **`autostart-status{extension}`** around a read, with `debug!(status, "checked")`.
- **`autostart-set{extension, enabled}`** around a change, with `debug!(status, "updated")`.
- **`NotSupported` and `NeedsApproval`** are `info!(reason, "refused")` when changing. They are the system's answer, not a failure.
- **`Failed`** is `warn!` plus one `log::warn!`, with the system's numeric status where there is one. These events never carry the executable's path or the user's home. On Linux, the file is written through `yard_paths`, whose own write events name the file, as for every part that writes through it.

## Parts

- **`yard_shell/autostart`** (`crates/yard_shell/src/dev/autostart.rs`, with `autostart/{macos,windows,linux}.rs`):
  - the two methods, `AutostartStatus`, `AutostartError`, `AutostartBackend` with its system and memory backends, and `set_autostart_backend`;
  - the rules and the logging.

  It adds no crate. It uses:
  - `objc2` and `objc2-foundation` on macOS;
  - the `windows` feature `Win32_System_Registry` on Windows;
  - `yard_paths` for the Linux file.
- **`tooling/xtask` (dev tooling):** the Windows installer's `[Registry]` entry deletes the `Run` value at uninstall.
- **Acceptance tests**, written first by a worker that does not write the code:
  - against `AutostartBackend::memory()`: read, enable, disable, the repeats, `set_status`, `fail_next`, and the missing-backend `ERROR`;
  - in their own file, run 20 times: the logging;
  - unit tests in the part's `tests.rs` for:
    - the Linux file's content and parsing, and a round trip through a temporary folder: enable, read, disable, a hidden file left alone, a malformed file replaced, and a path with `=` refused;
    - Windows quoting, the 260-character limit, and the `StartupApproved` byte;
    - the macOS bundle-path check and the status mapping.
- **Test extension `examples/autostart_tour`**, named in `review.rs`.
  - Without `AUTOSTART_TOUR` it does nothing.
  - With `AUTOSTART_TOUR=1`, it shows a card with the status and **Enable**, **Disable**, and **Refresh**.
  - It disables at quit if it enabled during the run.
  - The README names, for each system, where to look: Login Items and `sfltool dumpbtm` on macOS (with a bundle from `cargo xtask bundle macos --dev`); `reg query` and Task Manager on Windows; the `.desktop` file and `desktop-file-validate` on Linux.

## Known limits

- **A development build on macOS reports `NotSupported`**: `SMAppService` needs the application's bundle. Test it with a review bundle.
- **The entry starts the executable that set it.** If the application is moved or updated in place under another path, the entry points at the old path. Enabling again does not rewrite an entry that is already enabled, so the user, or the application, disables and enables it again.
- **A Linux review bundle (`--dev`)** starts its inner executable, not the wrapper that adds `dev`.

## Not in this slice

- Starting hidden or knowing the application was started at login. That needs roadmap #30, `app-lifecycle`.
- A helper that opens the system's Login Items settings.
- MSIX `StartupTask` (#38) and the Flatpak Background portal (#39).
