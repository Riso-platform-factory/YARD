# tray: an icon in the menu bar or notification area, with a menu of actions

Many desktop applications keep an icon where the system shows status:
- the menu bar's right side on macOS;
- the notification area of the taskbar on Windows;
- a StatusNotifierItem shown by the desktop's tray on Linux.

The icon shows that the application is running. Its menu reaches the application's actions even when no window has focus.

This slice lets one extension put such an icon up, with a tooltip and a menu built from the same `gpui::MenuItem`s as the application menu, and be told when the user clicks it.

GPUI has no tray support at Zed rev `ddb90e5`, and Zed has none. YARD calls each system directly, as `yard_shell/secrets` does:
- **macOS:** `NSStatusBar` and `NSMenu` through `objc2-app-kit`, already in `Cargo.lock`.
- **Windows:** `Shell_NotifyIconW` and a popup menu through the `windows` crate, already in `Cargo.lock`.
- **Linux:** the StatusNotifierItem and dbusmenu D-Bus interfaces through `ksni` 0.3 with its `async-io` feature, over the `zbus` already in `Cargo.lock`. No GTK is involved. `ksni` is under the Unlicense, a public-domain dedication compatible with YARD's GPL; the user approved this dependency on 2026-11-28.

Electron's `Tray` and Tauri's `TrayIconBuilder` were read for comparison. The sources are in the proposal.

## What an extension writes

```rust
let icon = include_bytes!("../assets/tray.png"); // a small PNG, black on transparent for a macOS template image
ext.set_tray(
    Tray::new(TrayIcon::png(icon).template(true))
        .tooltip("Notes is running")
        .menu(vec![
            MenuItem::action("New Note", NewNote),
            MenuItem::separator(),
            MenuItem::action("Quit", yard_shell::Quit),
        ]),
    cx,
)?;
ext.on_tray_activate(cx, |cx| show_main_window(cx));
```

- **`Extension::set_tray(tray: Tray, cx: &mut App) -> Result<(), TrayError>`** puts the icon up, or replaces the icon, tooltip, and menu of the extension's own icon.
- **`Extension::remove_tray(cx: &mut App)`** takes it down. It does nothing if this extension has no icon.
- **`Extension::on_tray_activate(cx, handler: impl Fn(&mut App) + 'static)`** registers one handler, which runs when the user activates the icon. A second registration by the same extension replaces the first, with a `debug` line.
- **`Tray`** has an icon, an optional tooltip, and a menu (`Vec<gpui::MenuItem>`, empty by default).
- **`TrayIcon::png(bytes)`** is a PNG, decoded with the `image` crate the workspace already has. `.template(true)` marks it as a macOS template image, drawn in the menu bar's own colour. It is ignored elsewhere.
- **`TrayError`** (`Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Display`, `Error`), with fixed texts:
  - `AlreadyOwned` ("another extension owns the tray icon");
  - `InvalidIcon` ("tray icon is not a valid PNG");
  - `Unavailable` ("no tray on this desktop");
  - `Failed` ("tray icon failed").

## The rules

- **One icon per application.** The first extension to call `set_tray` owns it until it calls `remove_tray`. Another extension's `set_tray` is `AlreadyOwned`. A menu bar has little room, and users expect one icon per application.
- **The icon** must decode as a PNG of at most 256×256 pixels (`InvalidIcon`). The system scales it to its size: 18 points high in the macOS menu bar, the small-icon size on Windows (`SM_CXSMICON`, which follows the DPI), and the size the Linux tray asks for.
- **The menu** takes `MenuItem::action`, `separator`, and `submenu`. An action item's checked and disabled states are shown when `gpui::MenuItem` carries them. An action item with an `os_action` is still an action, and is dispatched like the others. Any other kind of item, such as a system menu (`MenuItem::os_submenu`, the macOS Services menu), is left out, with an `info` line.
- **Choosing a menu item dispatches its action**, as the application menu does:
  - to the active window if there is one;
  - otherwise to the open window that was active most recently, since clicking the tray often takes activation away from every YARD window (the taskbar on Windows);
  - otherwise to the first open window;
  - otherwise to the application.

  So `yard_shell::Quit` and an extension's window actions both work. The dispatch runs on the UI thread, after the system's menu callback has returned.
- **Activation:**
  - On Windows, a left click activates and a right click opens the menu.
  - On Linux, the tray's activate request activates. The menu is published over dbusmenu, and the desktop's tray shows it (KDE, and GNOME with the AppIndicator extension, both do). The StatusNotifierItem `ContextMenu` call, which asks the item to draw its own menu, is not supported: `ksni` answers it with an error.
  - On macOS, a click opens the menu when there is one, as the system does. With an empty menu, a click activates.
- **At quit** the icon is removed, and from then on `set_tray` returns `Unavailable`, so nothing that runs later at quit can put it back. On Windows the icon is put back when Explorer restarts (the `TaskbarCreated` message), redrawn at the current DPI.
- **A stale menu never runs a newer action.** Each `set_tray` gives every item a new identity on every system, so a menu that was open before an update, or an event still queued from it, is ignored.
- **The shell tracks the most recently active window** for this: each YARD window, when it becomes active, becomes the most recent one, and a closed window is forgotten. On Windows the window active at the click is used first when there was one.
- **Labels render the same on every system:** `&` is escaped on Windows, and `_` on Linux, where both mark a keyboard accelerator. A non-square icon keeps its aspect ratio.
- **Linux without a tray:** with no StatusNotifierWatcher on the session bus (GNOME without the AppIndicator extension, most CI), `set_tray` returns `Unavailable`. Nothing is shown, and the application goes on.
- **Before the tray backend is set up** (`yard_shell::set_tray_backend`), `set_tray` returns `Unavailable` with one `ERROR` per application.

## Setup and tests

- **`yard_shell::set_tray_backend(cx, TrayBackend::system(product_name))`** is called once at startup. The product name is the Linux item's title and the tooltip default.
- **`TrayBackend::memory()`** is for tests. It records the current icon (its size and template flag), tooltip, and menu (labels, action names, separators, submenus). `simulate_activate()` and `simulate_menu(path)` deliver events through the same path as the system backends, and `fail_next(TrayError)` injects a failure. It is `Clone`, and clones share one state.
- **While the part is under review**, `review.rs` sets the system backend. **At promotion:**
  - `yard_app/startup` sets it from the product;
  - `yard_test/harness` sets a memory backend in `TestApp::boot`, through `yard_app::init_with_backends`, which replaced `init_with_secret_store`.

  Both are shown and stamped again then.

## Logging

- **`tray-set{extension, items}`** surrounds `set_tray`: `debug` "shown" or "updated"; `info` "refused" with `reason` for `AlreadyOwned` and `InvalidIcon`; `warn` plus one `log::warn!` for `Unavailable` and `Failed`.
- **`tray-remove{extension}`**, `debug` "removed".
- **`tray-activate{extension}`** around a delivered activation: `debug` "handled", or "no handler".
- **`tray-menu-action{extension, action}`** around scheduling a chosen item's action: `debug` "dispatched" with `target` = `window`, `last active window`, `first window`, or `application`. GPUI runs a window's action just after, in the window's own `extension-action` group.
- Menu labels and the tooltip are the extension's own text and may be logged. Nothing else of the user's data passes through.

## Parts

- **`yard_shell/tray`** (`crates/yard_shell/src/dev/tray.rs`, declared in `src/dev.rs`):
  - `Tray`, `TrayIcon`, `TrayError`, `TrayBackend` with its system and memory backends, and `set_tray_backend`;
  - the three methods, the rules, and the logging;
  - the system code for each OS in `#[cfg(target_os = …)]` sections.

  It adds to `yard_shell`'s dependencies, with the versions already locked except `ksni`:
  - `image` (workspace, PNG);
  - on macOS, `objc2`, `objc2-foundation`, and `objc2-app-kit` with the features it needs;
  - on Windows, the `windows` features for `Shell_NotifyIconW`, menus, and icons;
  - on Linux, `ksni` 0.3 with `default-features = false, features = ["async-io"]`.
- **Acceptance tests**, written first by a worker that does not write the code, against `TrayBackend::memory()`:
  - set, update, and remove;
  - ownership and `AlreadyOwned`;
  - `InvalidIcon` for non-PNG bytes and a 257-pixel PNG;
  - the recorded menu, including a submenu and a separator;
  - `simulate_menu` dispatching an action to a window, and `yard_shell::Quit` to the application;
  - `simulate_activate` reaching the handler, and "no handler";
  - each injected failure;
  - the missing-backend `ERROR`;
  - in their own file, run 20 times: the groups, fields, and levels.
- **Test extension `examples/tray_tour`**, named in `review.rs`.
  - Without `TRAY_TOUR` it does nothing.
  - With `TRAY_TOUR=1`, it places a card with buttons:
    - **Show tray:** an icon drawn in code and saved as a PNG, a template on macOS; tooltip "YARD tray tour"; menu "Say hello", a separator, a submenu "More" with "Inner", "New Window", and "Quit";
    - **Change menu:** adds "Again", and changes the tooltip;
    - **Remove tray**;
    - **Invalid icon:** shows `InvalidIcon`.
  - The card counts "Say hello", "Inner", and activations.
  - The README says what to look for on each system: macOS light and dark menu bars; Windows left and right click; Linux on KDE, and on GNOME with and without the AppIndicator extension.

## Known limits

- **GNOME shows no tray** without the AppIndicator extension. `set_tray` then returns `Unavailable`, and an extension should not rely on the icon to bring the application back.
- **macOS has no separate activation when there is a menu**: the click opens the menu, as for every status item.
- **The legacy X11 system tray (XEmbed)** is not supported, only StatusNotifierItem.
- **On Windows, the click-time window is resolved on the UI thread just before the popup opens.** If focus moves to another window in that moment, the action follows the usual order (active window, most recently active window, first window, application).

## Not in this slice

- Keeping the application running with no window, closing to the tray, and the macOS Dock's reopen. These are roadmap #30, `app-lifecycle`.
- A window or popover anchored to the icon.
- Balloon notifications. Notifications are `os-notifications` (#11).
- Icons on menu items, and radio groups (`gpui::MenuItem` has neither).
- A default icon from the product's build config. An extension gives its own PNG.
