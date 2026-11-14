# Window engine feature 6: app-menu

## What it adds

The application's menu bar, as Zed's. The composition root gives the menus as lists of actions, through GPUI's `Menu` and `MenuItem`, as Zed's `app_menus` does. Choosing an item runs its action, and an item shows its action's key. riso's product gets two menus: its own, with `Quit`, and `View`, with the shell's actions. riso gains the action `Quit` and its key, which it has not had: without a menu or a key, cmd-q did nothing.

## How it works

As Zed's `crates/zed/src/zed/app_menus.rs` and GPUI's `App::set_menus`:

- **The composition root gives the menus.** It passes a function that builds them to the startup, as it passes the extensions. The startup sets them once, after every extension's `init`, so that every action and every key is known.
- **An item runs its action** in the focused window, as if its key were pressed: GPUI sends it to the focused element and its ancestors. An action that nothing there handles makes its item disabled, as macOS shows it.
- **An item shows its key.** GPUI reads the first binding of the item's action when the menus are set, and macOS draws it at the right of the item. An action without a key shows none.
- **Quit.** riso's action `riso::Quit` quits the application, with its key `cmd-q` on macOS and `ctrl-q` elsewhere, as Zed's `zed::Quit`. It works with or without a window, and wherever the focus is: its key has no key context, and the startup handles it for the whole application.
- **riso's menus,** built by riso's own composition root:
  - the application's menu, named after the product: `Quit riso`;
  - `View`: `Zoom` (`riso::ToggleZoom`), `Hide Node` (`riso::Hide`), a separator, `Select Next Tab`, `Select Previous Tab`, a separator, `Focus Left`, `Focus Right`, `Focus Up`, `Focus Down`.
- On macOS, the system adds its own items to the menus, such as the Window menu's: they are not riso's.

## What code can do

```rust
// riso_shell
gpui::actions!(riso, [Quit]);   // bound by bind_default_keys to cmd-q (macOS) or ctrl-q, with no key context

/// Handles `riso::Quit` for the whole application: it quits.
pub fn handle_quit(cx: &mut App);
/// Sets the application's menus.
pub fn set_menus(cx: &mut App, menus: Vec<gpui::Menu>);

// app/riso: the startup, called by the composition root
pub(crate) fn run(
    product: riso_log::Product,
    extensions: &[(&'static str, Init)],
    menus: fn(&riso_log::Product) -> Vec<gpui::Menu>,
);

// app/riso: riso's own menus
pub(crate) fn menus(product: &riso_log::Product) -> Vec<gpui::Menu>;
```

The startup calls `handle_quit` in `app-init`, beside `bind_default_keys`, and `set_menus` with what `menus` builds after `extensions-load`. An application built on riso writes its own composition root, and gives its own menus the same way.

## Logging

| Situation | Log |
|---|---|
| The startup sets the menus | A group `menus-set{count}` inside `app-startup`, after `extensions-load`, with one `debug` line per menu: `menu` with `name` and `items`, the number of its items, separators included |
| The default keys are bound | `keys-bind` counts one more key, `cmd-q` (or `ctrl-q`) for `riso::Quit`, with no context, logged as its other keys are |
| The user quits through `Quit`, from the menu or its key | A group `app-quit` around the request to quit, before `app-shutdown` |
| An item's action runs | What that action already logs, as from its key: for example `layout-action` for `Zoom` |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` | Dev version beside production: the action `Quit` and its key in `bind_default_keys`; `handle_quit`, with the `app-quit` group; `set_menus`, with the `menus-set` group |
| `riso/startup` | Dev version beside production: the parameter `menus`; the calls to `handle_quit` and `set_menus` |
| `riso/menus` (new, dev) | riso's own menus |

## Tests

- GPUI tests of the shell:
  - `set_menus` gives GPUI the menus unchanged: `App::get_menus` on GPUI's test platform gives their names and every item's label and action, in order, separators included;
  - `bind_default_keys` binds `riso::Quit` to `cmd-q` on macOS, with no context;
- A unit test inside `riso/menus`, since `app/riso` is a binary that integration tests cannot reach: the names of riso's menus, and the label and action of every item, in order, separators included.
- A logging test of the shell, alone in its file: the `menus-set` group and its lines, and `keys-bind` with the new key. After `handle_quit`, dispatching `riso::Quit` to the application, with and without a window, opens `app-quit` once each time: GPUI's test platform does nothing when asked to quit, so the group is how the test sees the quit.
- **Existing tests:** only a test that counts or lists the default keys changes: it now has `cmd-q` for `riso::Quit`. Each change is listed when it is made.

## Test extension

The menus are the composition root's, so `cargo run dev` shows riso's own menus, with the parts under review. `LAYOUT_TOUR=zoom` places cards to act on. Use a fresh home: `RISO_HOME=/tmp/riso-window`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=zoom cargo run dev`, open the `View` menu | `Zoom` with `⇧⎋`, `Hide Node` with no key, `Select Next Tab` with `⌘}`, `Focus Left` with no single key (its binding is two keystrokes), and the separators. `menus-set{count=2}` in the log at startup. |
| Click card `b`, then choose `View > Zoom` | `b` fills the body, as with `shift-escape`. `layout-action` in the log. Choose it again: `b` goes back. |
| Open the application's menu | `Quit riso` with `⌘Q`. |
| Press cmd-q | riso quits. `app-quit`, then `app-shutdown`. |
| Start again, choose `Quit riso` from the menu | riso quits the same way. |

## Under review

- `riso_shell/shell`, in its dev version
- `riso/startup`, in its dev version
- `riso/menus`, new

## Not in this feature

- Zed's other application items: `Hide`, `Hide Others`, `Show All`, `Services`, `About`: when an application first needs them.
- `File`, `Edit`, and `Window` menus, and closing a window from a menu: with `new-window`, and with the first text input.
- Extensions adding their own items to the menus: when an extension first needs it.
- Asking before quitting when work would be lost: with the state engine.
