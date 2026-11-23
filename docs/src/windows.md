# Windows

An application built on YARD draws each window around a fixed shell frame: a title bar at the top, a body in the middle, and an optional status bar at the bottom. Users can open multiple windows, rearrange each independently, and have all open windows restored on the next launch.

## Title Bar

Every window draws its own title bar using the active theme's title bar background. Its height is `1.75rem` with a minimum of `34px`, scaling with the UI font size (not UI density).

The title bar has three slots laid out horizontally: `left`, `center`, and `right`. When nothing is shown in `left`, the title bar displays the window title (by default, the product name; applications can override it with `yard_shell::set_window_title(cx, title)`).

Window controls and interactions follow the operating system:

- **macOS:** A transparent system title bar with traffic-light buttons at `(9, 9)`. Space is reserved on the left for the traffic lights unless the window is fullscreen. Double-clicking the bar performs the system title bar action (zoom or minimize).
- **Linux:** Client-side decorations draw rounded corners and window buttons according to the desktop's button layout. Dragging the bar moves the window; clicking an item in the bar interacts with the item.
- **Windows:** Drawn minimize, maximize/restore, and close buttons appear on the right (hidden in fullscreen). The bar acts as the native window drag area.

## Status Bar

The status bar sits across the bottom of the window below the body, with two horizontal slots: `left` and `right`. Items are drawn in slot order from left to right. The left side shrinks and clips if space runs short; the right side never shrinks. Padding and spacing follow the active [UI density](./themes.md#ui-density).

When neither slot has any visible item, the status bar is not drawn and takes no space, so the body extends to the bottom of the window.

## Menu Bar

The application's composition root supplies the native menu bar with `yard_shell::set_menus(cx, Vec<gpui::Menu>)`. Entries run actions and display their bound keys. The sample application includes:

- **Application menu** (`<Product Name>`): Quit (`cmd-q` on macOS, `ctrl-q` on Linux and Windows). Global `yard::Quit` works even when no window is open.
- **File:** New Window (`cmd-shift-n` on macOS, `ctrl-shift-n` on Linux and Windows).
- **View:** Zoom, Hide Node, Select Next Tab, Select Previous Tab, and Focus Left/Right/Up/Down.

## Multiple Windows

Each open window owns its own [layout](./layout.md) tree, user layers, active [named layout](./layout/named-layouts.md), zoom state, tab selections, focus, and component views. Rearranging, zooming, or resetting one window does not affect other windows.

- Open a new window with `cmd-shift-n` (macOS), `ctrl-shift-n` (Linux and Windows), **File > New Window**, or the [Command Palette](./command-palette.md) (`yard::NewWindow`).
- The composition root registers how new windows are opened via `yard_shell::set_window_opener(cx, open)`. If `yard::NewWindow` runs with no opener registered, it is refused with a once-only `WARN`.
- A new window starts with the default layout (`yard.default`) and an empty user layer.
- Closing a window drops its views and state. Closing the last open window quits the application. Any shell call that requires an open window after the last one closes is refused with an `INFO` log (`no window`).

## Window Bounds and Session Restore

YARD remembers window positions, sizes, and layouts across restarts in the application's data directory (both `window.json` and `session.json` use format `"version": 1`):

- **`window.json`** stores the main window's `display` (display UUID string) and `bounds` (`state` as `"windowed"`, `"maximized"`, or `"fullscreen"`, plus `x`, `y`, `width`, and `height` in points). Updates are debounced by `100ms` and flushed on quit.
- **`session.json`** stores every open window in order (main window first) under `windows`. Each entry holds `bounds` (`state`, `x`, `y`, `width`, `height`), optional `display` UUID, active named `layout`, and per-layout user `layers`. Updates are debounced by `100ms` and flushed on quit. Closing a window removes it from the session, except the last window, which remains so it reopens on the next start.
- **`state.json`** stores per-window component state, selected tabs, and zoom state in the same window order as `session.json`.

On startup, a usable `session.json` (valid version `1` with at least one window) takes precedence over `window.json` and `layout.json`, reopening every window in order with its saved bounds, named layout, and user layers. If `session.json` is missing, unreadable, empty, or uses an unsupported version, YARD falls back to opening a single window using `window.json` and `layout.json`. If a saved display UUID is no longer connected, the window opens at the operating system's default placement.

## Overlays: Modals, Toasts, and Prompts

Three layers sit above the window body:

- **Modals:** One modal can be open per window at a time. Opening a different modal type replaces the active modal; toggling the same type closes it. A modal takes keyboard focus, optionally dims the window behind it, and closes on `escape` (`yard::Dismiss`), clicking outside, or toggling again. Closing restores focus to the previously focused element.
- **Toasts:** Short non-focus-stealing notifications (`info`, `warning`, or `error`) shown one at a time over the window, optionally carrying one action button. Showing a new toast replaces any currently visible toast. A toast dismisses automatically after `10s`; hovering pauses the timer and resumes with at least `800ms` remaining. A toast can also be dismissed with its close button, a middle click, or by triggering its action.
- **Prompts:** Native operating system confirmation dialogs with a message, optional detail text, and answer buttons. The extension's callback receives a zero-based answer index (`Some(usize)`) or `None` if the prompt goes away without an answer (for example, if code closes its window). On macOS, prompts queue one at a time per window, `Return` chooses the first button, and `Escape` chooses a button named `Cancel`.

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `window-frame` | — | Creating the window's frame nodes (`window`, `title_bar`, `body`, `status_bar`) |
| `window-new` | — | Opening a new window via `yard::NewWindow` |
| `window-close` | `window` | Window released and cleaned up |
| `menus-set` | `count` | Setting native application menus |
| `app-quit` | — | Running global `yard::Quit` |
| `window-bounds-save` | `state` | Writing `window.json` |
| `window-bounds-restore` | `file` | Reading `window.json` on startup |
| `session-save` | `windows` | Writing `session.json` |
| `session-restore` | `file` | Reading `session.json` on startup |
| `modal-show` | `modal` | Opening a modal |
| `modal-hide` | `modal`, `reason` | Closing or replacing a modal |
| `toast-show` | `kind` | Showing a toast |
| `toast-hide` | `kind`, `reason` | Hiding, expiring, or replacing a toast |
| `toast-action` | `kind`, `label` | Running a toast's action button |
| `prompt` | `level`, `buttons` | Showing a native prompt until answered or dismissed |
