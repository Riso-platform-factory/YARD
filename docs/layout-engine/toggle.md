# Layout feature 27: toggle

## What it adds

A component of the shell, `yard.toggle`: a button that shows or hides the node its props name, as Zed's status bar buttons show and hide its docks' panels. With `alone`, showing that node hides the other nodes shown in its slot, so that one panel at a time takes a side. An extension places a toggle like any node, usually in the status bar, and needs no code of its own to make it work.

## How it works

As Zed's `PanelButtons` (`crates/workspace/src/dock.rs`, around line 1542):

- **Its props** (feature 13): `node`, the node it shows and hides (required); `icon`, the name of an icon of the UI kit (required); `tooltip`, a text (default: the node's title, feature 14, or its ID); `alone`, a boolean (default `false`). Props that cannot be read are logged with feature 13's `WARN`, and a toggle with no readable `node` or `icon` draws nothing.
- **Its look:** an `IconButton` of the UI kit with its `icon`, `IconSize::Small`, toggled (`toggle_state`) while its node is shown and not empty (feature 23), with its `tooltip`, as Zed draws a panel button. A toggle whose node is not in the tree is drawn disabled.
- **A click** on a toggle whose node is hidden shows it with a `show` operation. With `alone`, the other nodes shown in the same slot of the same parent are first hidden, each with a `hide` operation, and then the node is shown. A click on a toggle whose node is shown hides it.
- **Operations** go through the same path as any operation, with extension `yard`, so they are recorded, saved, restored, and reset, and the rules hold: hiding a required node (feature 15) is refused with its `INFO`, and the toggle's state follows what the tree really is.
- **The toggle never hides itself or a node above it:** a toggle that names itself or an ancestor draws disabled, with a `WARN` once.

## What code can do

Nothing new in the API: `yard.toggle` is a component of the shell, placed with `place_with_props` or `add_with_props` (feature 13), as any node with props:

```rust
ext.place_with_props(cx, "toggle_left", "yard.toggle", "status_bar", "left",
    json!({"node": "left", "icon": "info", "tooltip": "Left Panel", "alone": true}));
```

The dev shell has every item of the production shell, unchanged; it registers one more component.

## Logging

| Situation | Log |
|---|---|
| A toggle is clicked | A group `toggle{toggle, node, shown}` (`shown` is what it will become), holding the `layout-op` groups of its operations |
| A toggle names a node that is not in the tree, itself, or an ancestor | `WARN` once in the diagnostic log; the toggle draws disabled |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_shell/shell` dev version | The component `yard.toggle`, its props, its look, and its click |
| `yard/startup` | Dev copy that uses the dev shell. While `toggle` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- GPUI tests, with the dev shell:
  - a toggle shows its node when hidden and hides it when shown, with one `show` or `hide` operation, and is drawn toggled while the node is shown;
  - with `alone`, showing a node hides the others shown in its slot, before showing it;
  - a toggle of a required node cannot hide it: the `INFO`, and the toggle stays toggled;
  - a toggle whose node is not in the tree is disabled, with one `WARN`;
  - its state follows a hide or show made by another path (an extension's call, a snap, a reset);
  - the operations are saved and restored.
- **Logging**, alone in its file: `toggle{toggle, node, shown}` holds its `layout-op` groups.

## Test extension: `examples/layout_tour`

A new case, `toggle`. The body holds a row: a column `side` with two cards, `left` (shown) and `extra` (hidden at start), then `main`, then `right`. The status bar holds, on its left, a toggle for `left` (icon `info`, `alone: true`) and one for `extra` (icon `plus`, `alone: true`), and on its right a toggle for `right` (icon `warning`). `left` and `extra` share the slot of `side`, so showing one hides the other; `right` is alone in nothing.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-toggle LAYOUT_TOUR=toggle cargo run dev` | The status bar shows three buttons; those of `left` and `right` are toggled, that of `extra` is not. |
| Click the first toggle | `left` hides; the button is no longer toggled; `toggle{node=left, shown=false}` with one `layout-op{op=hide}`. |
| Click it again | `left` shows. |
| Click `extra`'s toggle | `left` hides and then `extra` shows, in `side`; two `layout-op` groups inside `toggle{node=extra, shown=true}`. |
| Quit and start again, then `reset layout` | The same state; after the reset, `left` and `right` shown and `extra` hidden. |

## Under review

- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- A key per toggle, as Zed binds `cmd-b` to the left dock: with the keymap engine, when a feature needs it.
- Icons the UI kit does not have yet: they are added when a feature names them.
