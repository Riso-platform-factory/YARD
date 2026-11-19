# Keymap feature 5: extension-actions

## What it adds

An extension declares GPUI actions of its own, binds their default keys, and handles them. It does so in one of two ways:

- **In a component,** where an action runs only while that component has the focus.
- **For the whole window,** through its handle, where an action runs wherever the focus is.

## Declaring actions

An extension declares its actions with GPUI's `actions!` macro, in a namespace of its own, which is its name: `actions!(keys_tour, [Increment, Ping])`. GPUI registers every action name when the application starts, and stops the application if two actions have the same name. So the namespace keeps extensions apart. The names `yard` and `zed` are taken.

## Binding keys

```rust
impl Extension {
    pub fn bind_keys(&self, cx: &mut App, bindings: &[(&str, Box<dyn Action>, &str)]);
}
```

- Each binding is its keys, its action, and its key context.
- **A key for a component's action** uses the component's own key context, such as `keys_tour`. It works only while that component has the focus.
- **A key for a window-wide action** uses the context `Shell`, as the shell's own keys do.
- **An extension may bind its keys to any action,** the shell's included, for example a key of its own for `yard::ToggleZoom`.
- **The extension's bindings come after the shell's,** because extensions run after the startup binds the defaults. At the same depth, the later binding wins, so an extension can override a shell key. A deeper context wins over `Shell`.
- **Keys and contexts are checked.** A binding whose keys or context cannot be read is refused with an `ERROR` that names it, and the others are still bound. An extension never stops the application with a bad key.

## Handling an action in a component

A component handles its action on its own element, as any GPUI view does. Its element carries:

- the node's focus handle, from `yard_shell::focus_handle`, tracked with `track_focus`;
- its key context, `key_context("keys_tour")`;
- `on_action`.

GPUI sends an action only along the focused element's ancestors. So the element that handles it must track the node's handle itself. The shell needs nothing more.

The shell's own actions still reach the node's wrapper above it.

## Handling an action for the whole window

```rust
impl Extension {
    pub fn on_action<A: Action>(&self, cx: &mut App, handler: impl Fn(&A, &mut Window, &mut App) + 'static);
}
```

- The shell's root handles the action for every window, wherever the focus is, as Zed's workspace does for its own actions, and calls the extension's handler.
- **The action must be in the extension's namespace.** Handling another's action is refused with an `ERROR`: `extension keys_tour cannot handle yard::Hide`. An action of the shell stays the shell's.

## Logging

| Situation | Log |
|---|---|
| An extension binds keys | A group `keys-bind{extension, count}` inside `extension-init`, with a `debug` line per binding: keys, action, context |
| A binding cannot be read | `ERROR` with the keys and the reason, inside `keys-bind`; the others are bound |
| An extension registers a window-wide handler | `debug`: `action handled {extension, action}`, inside `extension-init` |
| Another namespace's action | `ERROR`: `extension <name> cannot handle <action>` |
| A window-wide action runs | A group `extension-action{extension, action}` around the handler, so the handler's own lines are inside it |
| A component's action runs | The extension's own code logs it. The shell does not see it |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_shell/shell` | Dev version beside production. `Extension::bind_keys`, `Extension::on_action`, the root's handling of extensions' window-wide actions, and the logging |
| `yard/startup` | Dev copy, with the dev shell |

## Test extension: `examples/keys_tour`

A new test extension, `keys_tour`. With `KEYS_TOUR=all`, it declares `keys_tour::Increment` and `keys_tour::Ping`, and places a row with two cards, `a` and `b`, of its component `keys_tour.card`. A card shows its node, its count, and `focused` when its node has the focus.

| Action | Key | Context | Effect |
|---|---|---|---|
| `keys_tour::Increment` | `ctrl-alt-i` | `keys_tour` | The focused card's count goes up by one |
| `keys_tour::Ping` | `ctrl-alt-p` | `Shell` | The page logs `ping`, and each card shows the number of pings |

`cmd-i` and `cmd-p` are Zed's keys, so the tour uses `ctrl-alt-i` and `ctrl-alt-p`. Use a fresh home: `YARD_HOME=/tmp/yard-keys`.

| Command and action | What to look for |
|---|---|
| `KEYS_TOUR=all cargo run dev` | `keys-bind{extension=keys_tour, count=2}` and `action handled {extension=keys_tour, action=keys_tour::Ping}` inside `extension-init`. |
| Click card `a`, then `ctrl-alt-i` twice | `a` counts 2; `b` stays 0. |
| Click card `b`, then `ctrl-alt-i` | `b` counts 1. |
| Before clicking anything, `ctrl-alt-i` | Nothing: no card has the focus. |
| `ctrl-alt-p`, with or without a focused card | `extension-action{extension=keys_tour, action=keys_tour::Ping}` with `ping` inside; both cards show 1 ping. |
| `shift-escape` on a focused card | The shell's zoom still works. |

## Tests

- GPUI tests, on macOS, with the tour's component in a test layout:
  - `ctrl-alt-i` counts on the focused card only, and does nothing with no focus;
  - `ctrl-alt-p` runs `Ping` from any focus and with none;
  - a binding with unreadable keys is refused, and the other bindings work;
  - `on_action` for another namespace's action is refused, and the action still runs as before;
  - an extension's key in `Shell` for a shell action overrides the shell's key at the same depth.
- A logging test alone in its file: every row of the table.

## Under review

- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- The user's keys: `user-keymap`.
- A trace of which action a key resolved to: later.
- Listing actions by name: `command-palette`.
