# Window engine feature 7: window-bounds

## What it adds

The window opens where the user left it: its place, its size, and whether it was maximized or full screen are saved when they change, and used at the next start. If the screen it was on is gone, the window opens at the default place, as Zed does.

## How it works

As Zed's workspace (`crates/workspace/src/workspace.rs`: `observe_window_bounds` around line 2041, `save_window_bounds` around line 7742; `crates/zed/src/zed.rs` `build_window_options`, line 360):

- **What is saved.** The window's bounds as GPUI gives them (`Window::inner_window_bounds`): windowed, maximized, or full screen, each with the bounds it has when windowed, and the UUID of the screen the window is on. Nothing is saved for a window whose screen has no UUID.
- **When.** When the window's bounds change while the window is active, the save waits 100 ms and then writes once, so a drag or a resize writes once at its end and not on every frame, as Zed's `bounds_save_task_queued`. A change while another save waits does not start a second one.
- **Where.** In `window.json` in the data directory, named by `riso_paths` (`Paths::window_file`), written with foundation 6's atomic write. The format is JSON: a `version` (1), the screen's `display` UUID, and the `bounds`: a `state` (`windowed`, `maximized`, or `fullscreen`) with `x`, `y`, `width`, and `height` in points.
- **At the start.** The startup reads the file before it opens the window:
  - no file: the window opens at the default place and size, as today;
  - a file that cannot be read, is not valid JSON, or has another `version`: the default, with a `WARN`, and the file is left until the next save replaces it;
  - a screen with the saved UUID is among the screens: the window opens on that screen with the saved bounds and state;
  - no screen has that UUID: the window opens at the default place and size, with an `INFO`, as Zed's `build_window_options` finds no display.
- **Only the main window.** The startup opens one window; several windows come with feature 9, and their bounds with feature 10.

## What code can do

```rust
// riso_paths (dev part `window_file`)
impl Paths {
    pub fn window_file(&self) -> PathBuf;
}

// riso_shell (dev part `window_bounds`)
/// The saved bounds and screen for the next window, or `None` for the default.
pub fn saved_window_bounds(cx: &App, file: &Path) -> Option<(gpui::WindowBounds, gpui::DisplayId)>;
/// Saves `window`'s bounds to `file` when they change, as described above.
pub fn track_window_bounds(window: &mut Window, cx: &mut App, file: PathBuf);
```

The startup passes them into `WindowOptions` (`window_bounds`, `display_id`) and calls `track_window_bounds` on the opened window.

## Logging

| Situation | Log |
|---|---|
| Reading at the start | A group `window-bounds-restore{file}`, inside `window-open`, ending with `debug` `restored {state, display}`, or `debug` `no saved window bounds`, or the `WARN` or `INFO` above |
| A save | A group `window-bounds-save{state}` holding foundation 6's `file-write{path}`; a failure is its `WARN` |
| The bounds change while a save waits | `trace` `window bounds changed` |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_paths/window_file` (new, dev) | `Paths::window_file`: `window.json` in the data directory |
| `riso_shell/window_bounds` (new, dev) | `saved_window_bounds`, `track_window_bounds`, the file format, the 100 ms wait |
| `riso/startup` | Dev copy that reads the saved bounds before opening the window and tracks them after. While `window-bounds` is under review, `cargo run dev` uses it, with `review` and the command palette, as the production startup loads them |

## Tests

- **Paths, unit test:** `window_file()` is `window.json` in the data directory.
- **Shell, tests:**
  - the format: windowed, maximized, and full-screen bounds with a display UUID are written and read back the same;
  - a missing file gives `None` with `no saved window bounds`; a broken file and another `version` give `None` with a `WARN`;
  - a saved UUID that no screen has gives `None` with the `INFO` (GPUI's test platform's screens);
  - GPUI tests: changing a window's bounds several times within 100 ms writes the file once; an inactive window's changes are not saved.
- **Logging**, alone in its file: `window-bounds-restore{file}` with `restored`; `window-bounds-save{state}` holding `file-write`.

## Test extension: `examples/window_tour`

No new case: any run shows it.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-bounds cargo run dev`, move and resize the window, wait, quit | One `window-bounds-save{state=windowed}` per move or resize, not one per frame; `window.json` holds the bounds. |
| Start again | The window is where it was, with its size; `restored {state=windowed, display=…}`. |
| Click the window's green button (full screen), quit, start | The window opens full screen; leaving full screen gives back the earlier size. |
| Change `display` in `window.json` to another UUID, start | The window opens at the default place; the `INFO`. |

## Under review

- `riso_paths/window_file`, `riso_shell/window_bounds`, and `riso/startup` in its dev version

## Not in this feature

- Several windows and their bounds: features 9 and 10.
- Full screen as macOS window tabs, and the native window state Zed saves for its own windows.
