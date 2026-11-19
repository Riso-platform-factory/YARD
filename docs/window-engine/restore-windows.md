# Window engine feature 10: restore-windows

## What it adds

The windows that are open when the application quits are opened again at the next start, each with its place, its size, its state, the named layout it showed, and its own user layers, as Zed reopens its windows from its session. Until now only the main window came back.

## How it works

As Zed's session (`crates/workspace/src/persistence.rs` and `session`, as reference):

- **The session file.** `session.json` in the data directory, named by `yard_paths` (`Paths::session_file`), written with foundation 6's atomic write. JSON: a `version` (1) and the `windows`, in the order they were opened, the main window first. Each window holds:
  - its `bounds` and `display`, as `window.json` holds them (feature 7);
  - the named layout it shows (feature 30);
  - its user layer for each named layout it has shown, as lists of operations in feature 12's format.
- **When it is saved.** When a window's bounds or layout change (at most once every 100 ms, as feature 7), when a window opens or closes, and when the application quits, before its windows close, so that the session lists every window open at quit. A window closed during the run leaves the session, except the last one: closing it quits the application, so it stays in the session and opens again at the next start, as quitting with it open would.
- **At the start.** The startup reads the session before opening any window:
  - a valid session with at least one window: each window opens in order with its bounds (feature 7's rule for a screen that is gone), the named layout it showed, and its user layers, each operation checked as restore checks it (feature 12: a refused one is skipped with an `INFO`). The first is the main window.
  - no file, a broken file, another `version`, or no window: the start is as today (one window, from `layout.json` and `window.json`), with feature 12's `WARN` for a broken file.
- **The main window's files.** `layout.json`, the named layouts' files, and `window.json` keep holding the main window's state, as features 7, 12, and 30 say; the session holds every window and wins at the start when it is valid. A window opened from the session keeps the user layers the session gave it for every named layout: when it switches to one, it uses that layer, and reads the layout's file only for a named layout the session holds no layer for.
- **Every window saves.** With the session, every window's layout is kept, not only the main window's; feature 8's "other windows keep their user layers in memory" ends here.

## What code can do

```rust
// yard_paths (dev part `session_file`)
impl Paths {
    pub fn session_file(&self) -> PathBuf;
}

// yard_shell (dev part `session`)
/// The windows of the saved session, or `None` to start as before.
pub fn saved_session(cx: &App, file: &Path) -> Option<Vec<SessionWindow>>;
/// Keeps `file` up to date with every open window, as described above.
pub fn track_session(cx: &mut App, file: PathBuf);
/// The shell of a window recreated from the session: as `new_shell`, but
/// showing the window's named layout with its user layers.
pub fn new_session_shell(window: &mut Window, cx: &mut App, saved: &SessionWindow) -> Entity<Shell>;
```

`SessionWindow` holds what the startup needs to open one window: its bounds and display (as `saved_window_bounds` gives them), its named layout, and its user layers. The startup opens each window as the window opener of feature 9 does, with the saved bounds, and builds its shell with `new_session_shell`.

## Logging

| Situation | Log |
|---|---|
| Reading at the start | A group `session-restore{file}`, inside `app-startup`, ending with `debug` `restored {windows}`, or `debug` `no saved session`, or the `WARN` |
| Each window opened from the session | Its `window-open`, with `window{id}` and `layout-restore` as for the main window, and feature 12's `skipped` for a refused operation |
| A save | A group `session-save{windows}` holding `file-write{path}` |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_paths/session_file` (new, dev) | `Paths::session_file`: `session.json` in the data directory |
| `yard_shell/session` (new, dev) | `saved_session`, `track_session`, `SessionWindow`, and what the shell needs to open a window with given layers |
| `yard_shell/shell` dev version | A window can be created with a named layout and user layers given; every window's layout changes reach the session |
| `yard/startup` | Dev copy that reads the session and opens its windows, else starts as today, and tracks the session. While `restore-windows` is under review, `cargo run dev` loads `review` and `layout_tour` on it |

## Tests

- **Paths, unit test:** `session_file()` is `session.json` in the data directory.
- **Shell, tests:**
  - the format: windows with bounds, display, named layout, and user layers are written and read back the same;
  - a missing, broken, or other-version file, or no windows, gives `None` with the log above;
  - GPUI tests: two windows rearranged differently, the session saved; the windows recreated from it each show their own layout; a refused operation in one is skipped with the `INFO`; a window closed during the run is not in the next save; closing the last window keeps it in the session; quitting saves every open window once.
- **Logging**, alone in its file: `session-restore{file}` with `restored {windows}`; `session-save{windows}` holding `file-write`.

## Test extension: `examples/layout_tour`

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-session LAYOUT_TOUR=user-layer cargo run dev`, press `cmd-shift-n`, rearrange each window differently, move the second one, quit | `session-save{windows=2}`. |
| Start again | Both windows are back, each where it was, each with its own arrangement; `restored {windows=2}`. |
| Close the second window, quit, start | One window. |

## Under review

- `yard_paths/session_file`, `yard_shell/session`, and `yard_shell/shell`, `yard/startup` in their dev versions

## Not in this feature

- What each component showed, and selected tabs: the state engine (step 7, after this).
- Restoring windows on a different set of screens beyond feature 7's rule.
