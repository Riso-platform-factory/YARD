# Window engine feature 9: new-window

## What it adds

The user opens another window with `riso::NewWindow`, as Zed's `workspace::NewWindow`: from the key `cmd-shift-n`, from the menu `File > New Window`, or from the command palette. The new window shows the default layout and is rearranged on its own, with its own user layer (feature 8). The application quits when the last window closes, as today.

## How it works

- **The action.** `riso::NewWindow` is an action of the shell, beside its layout actions, bound by default to `cmd-shift-n` (`ctrl-shift-n` elsewhere), as Zed binds `workspace::NewWindow`.
- **Opening.** The composition root opens windows, so it gives the shell how: `riso_shell::set_window_opener(cx, open)`, where `open` opens a window as the startup opens the main one (the product's title bar, the shell, the appearance). The shell's handler of `riso::NewWindow` calls it inside `window-new`. Without an opener, the action is refused with a `WARN` once.
- **The new window:**
  - shows the default layout, with the named layout `riso.default`, an empty user layer of its own, and no zoom;
  - opens where the OS puts a new window (GPUI's default place, cascaded from the active one), at the default size; its place is not saved (feature 10 saves every window's);
  - becomes the active window, so the next calls act on it (feature 8);
  - is not the main window: its user layers stay in memory.
- **The menu.** The application's menu bar gains a `File` menu, after the application menu, with `New Window`, as Zed's.
- **Closing.** Closing a window drops its layout (feature 8). Closing the last one quits the application, as today.

## What code can do

```rust
// riso_shell::dev
gpui::actions!(riso, [NewWindow]);
pub fn set_window_opener(cx: &mut App, open: impl Fn(&mut App) + 'static);
```

## Logging

| Situation | Log |
|---|---|
| `riso::NewWindow` | A group `window-new`, holding the new window's `window{id}` and `shell-create{root, window}` |
| No opener | `WARN` once in `window-new` |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` dev version | `NewWindow`, its default key, `set_window_opener`, and the handler |
| `riso/menus` (production, in place) | A `File` menu with `New Window`. Stamped again. |
| `riso/startup` | Dev copy that gives the shell its opener, opening each window as it opens the main one. While `new-window` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **Menus, unit test:** the menus list `File > New Window` with `riso::NewWindow`, after the application menu.
- **Shell, GPUI tests, with the dev shell:**
  - dispatching `riso::NewWindow` calls the opener once; with an opener that opens a shell window, a second window exists, shows the default layout with an empty user layer, and is the active window;
  - two windows rearranged differently stay different; the main window's saved file holds only the main window's operations;
  - `cmd-shift-n` is bound to `riso::NewWindow`;
  - without an opener, one `WARN`, and nothing opens.
- **Logging**, alone in its file: `window-new` holding `shell-create{root, window}`.

## Test extension: `examples/layout_tour`

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-newwin LAYOUT_TOUR=user-layer cargo run dev`, press `cmd-shift-n` | A second window with the default layout; `window-new`. |
| Drag tab `b` onto the top edge of `right` in the second window | Only the second window splits; the first is as it was. |
| `File > New Window` | A third window. |
| Close the windows one by one | Each window's views are dropped; the application quits with the last. |

## Under review

- `riso/menus`, in place
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Saving and reopening every window: feature 10.
- A window's own named layout chosen when it opens.
