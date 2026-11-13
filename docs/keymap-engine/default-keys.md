# Keymap feature 3: default-keys

## What it adds

The shell binds keys to its actions at startup, the keys Zed uses on each OS. `shift-escape` zooms the focused node, and `cmd-}` shows the next tab on macOS.

## The keys

| Action | macOS | Linux and Windows | As Zed's |
|---|---|---|---|
| `riso::ToggleZoom` | `shift-escape` | `shift-escape` | `workspace::ToggleZoom` |
| `riso::SelectNextTab` | `cmd-}`, `alt-cmd-right` | `ctrl-pagedown` | `pane::ActivateNextItem` |
| `riso::SelectPreviousTab` | `cmd-{`, `alt-cmd-left` | `ctrl-pageup` | `pane::ActivatePreviousItem` |
| `riso::Hide` | none | none | Zed has no such action with a key |

## How

- **The keys are bound in the key context `Shell`,** which the shell's root sets. They therefore work wherever the focus is in the window, also when no node has it, where the action is refused and logged as `layout-actions` says.
- **An extension can bind other keys to the same actions, or other actions to the same keys,** from a deeper key context of its own. GPUI then prefers the deeper one. At the same depth, the last binding wins.
- **The shell binds its keys in the startup's `app-init`,** before any extension runs. So an extension's own bindings, and later the user's, come after and win.
- **Each OS's keys are chosen when riso is built,** for the OS it is built for.

## What code can do

```rust
pub fn bind_default_keys(cx: &mut App);
```

The startup calls it once, in `app-init`.

## Logging

| Situation | Log |
|---|---|
| The defaults are bound | A group `keys-bind{count}` in `app-init`, with a `debug` line per binding: its keys, its action, and its context |
| A key runs an action | The action's own `layout-action{action, node}`, as before |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` | Dev version beside production. The root's key context `Shell`, and `bind_default_keys` |
| `riso/startup` | Dev copy. It calls `riso_shell::dev::bind_default_keys` in `app-init` |

## Tests

- GPUI tests, pressing keys with GPUI's simulated keystrokes on macOS:
  - `shift-escape` zooms the focused card, and again zooms out;
  - `cmd-}` and `alt-cmd-right` select the next tab, and `cmd-{` and `alt-cmd-left` the previous one;
  - `shift-escape` with no node focused changes nothing.
- A unit test of the bindings for each OS: the keys of the table, in the context `Shell`.
- A logging test alone in its file: `keys-bind{count}`, and a `debug` line per binding.

## Test extension: `examples/layout_tour`

The case `actions` is used with keys instead of its buttons. Use a fresh home: `RISO_HOME=/tmp/riso-layout`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=actions cargo run dev` | `keys-bind{count=5}` in `app-init`. |
| Click card `b`, then press `cmd-}` three times | `c`, `d`, then `b`. `cmd-{`: `d`. |
| Click card `a`, then `shift-escape` | `a` fills the window. `shift-escape` again: back. |
| Press `shift-escape` before clicking anything | Refused: `no node has the focus`. |

## Under review

- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Keys for extensions' own actions: `extension-actions`.
- The user's keys: `user-keymap`.
- Keys for keyboards that are not US keyboards.
