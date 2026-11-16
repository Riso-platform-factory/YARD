# Window engine feature 8: window-tree

## What it adds

The layout of a window belongs to that window: its tree, its user layers, its zoom, its focus, the named layout it shows, and the views of its nodes. What extensions register stays with the application and is shared by every window: components, titles, emptiness, sizes, required nodes, slots for one, contexts they provide, item slot declarations, named layouts, and actions. With one window, nothing changes on screen; the log names the window. This is the ground for feature 9, which opens more windows, each with its own user layer, as decided.

## How it works

- **The default layout** is built once, by the extensions at startup, as today, before any window opens. Every window starts from it.
- **A window's layout.** Each window has, of its own:
  - its tree with its layers: the default, the named layout it shows (feature 30), and that layout's user layer;
  - its user layer for each named layout it has shown;
  - its views, focus handles, bounds, zoom, selected tabs, split sizes in view state, item copies (feature 28), and emptiness;
  - the named layout it shows.
  An operation, a reset, a switch, or a change of items in one window never changes another window.
- **Which window a call acts on.** A call that is given a window (`reset_layout(window, cx)`, `show_layout(window, cx, …)`, `add` from a handler that has the window, …) acts on that window. A call without one (`remove`, `move_node`, `set_hidden`, `set_props`, `set_zoomed`, `is_hidden`, `node_bounds`, `inspect`, …) acts on the active window, the one with the keyboard focus. With no active window, it acts on the window that was active last. Before any window opens, placing nodes builds the default layout, as today. A call made while no window is open after one has closed is refused with an `INFO`: `no window`.
- **Files.** The window opened first is the main window: its user layers are saved to `layout.json` and the named layouts' files, as today, and restored when it opens. Other windows (feature 9) keep their user layers in memory; feature 10 saves them. When the main window closes and another stays, no window becomes the main one: the files keep the main window's last state, and the others stay in memory.
- **Observers.** `observe_layout` (feature 31) is called after a change of any window's tree; `inspect` reads the active window's.
- **Window models.** A model an extension gives to the window level (`provide_window`, feature 18) belongs to the application: every window, open now or later, finds the same model.
- **Closing a window** drops its views, its focus handles, its item copies' observers, and everything else it holds, with `view dropped` for each view, inside `window-close{window}`.

## What code can do

Nothing new in the API. Every existing call keeps its signature and acts on the window above.

The dev shell has every item of the production shell; what changes is where the state lives.

## Logging

| Situation | Log |
|---|---|
| A window's shell is created | `shell-create{root, window}`: the group of feature 1 names the window |
| Work on one window's layout (an operation, a reset, a restore, a switch, a change of items, a drop) | Inside a group `window{id}`, holding the groups it held before, unchanged |
| A call with no window to act on | `INFO` `no window` in the call's group |
| A window closes | A group `window-close{window}`, with `view dropped` for each of its views |

The groups and lines inside `window{id}` are as before, so every earlier log, read from the operation's own group inward, is unchanged.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` dev version | The application's registry keeps what extensions register; each window's shell holds its layout; calls act on the active window; `window{id}` and `window-close{window}` |
| `riso/startup` | Dev copy that uses the dev shell. While `window-tree` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **GPUI tests, with the dev shell:**
  - with one window, every earlier behaviour holds: the dev shell passes the earlier shell tests (`devshell-check`);
  - two windows opened in a test (`open_window` with the shell twice): each starts from the default layout; an operation, a reset, a zoom, a switch of named layout, and an item change in one leave the other as it is; their views are distinct entities;
  - a call acts on the active window; after it loses the focus to the other window, calls act on that one;
  - only the main window's user layer is saved;
  - closing a window drops its views and its observers, and the other window keeps working; a call with no window logs the `INFO`.
- **Logging**, alone in its file: an operation's `layout-op` is inside `window{id}` with the active window's ID; `shell-create{root, window}`; `window-close{window}` with `view dropped`.

## Test extension: `examples/layout_tour`

No new case: every case runs as before. `RISO_HOME=/tmp/riso-wtree LAYOUT_TOUR=user-layer cargo run dev`, drag a tab and reset: the same as in feature 26, with `window{id=…}` around each operation in the log.

## Under review

- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Opening another window: feature 9.
- Saving other windows' layouts: feature 10.
