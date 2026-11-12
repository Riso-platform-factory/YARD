# Layout feature 10: zoom

## What it adds

One node can be zoomed, so that it takes the whole window. While it is zoomed, the rest of the layout is not drawn, but every view keeps its state. Zooming out brings the layout back exactly as it was.

## Model

This follows Zed's `dock.rs` and workspace.

- At most one node of the window is zoomed. Zed's workspace keeps one zoomed item, and zooming another replaces it.
- The zoom is window state kept by the shell. It is not part of the tree:
  - zooming changes no node, no share, and no tab selection;
  - zooming does not go through `apply`.
- An extension zooms through its handle:
  - `set_zoomed(node)` zooms a node;
  - `zoom_out()` brings the layout back.
- Any extension may zoom any node, as any panel of Zed may ask to zoom in.
- `zoomed()` tells which node is zoomed, if any, as `is_hidden` tells whether a node is hidden, so that a component can show the right control.

## Rules

| Situation | What happens | Log |
|---|---|---|
| The node is not in the tree | Refused | `INFO` |
| The node is the root | Refused: it already fills the window | `INFO` |
| The node is hidden, or under a hidden node | Refused: it is not shown | `INFO` |
| Another node is zoomed | It is replaced | `debug`: which node was zoomed before |
| `zoom_out` when nothing is zoomed | Nothing changes | `debug` |
| After any operation, the zoomed node is no longer shown (removed, hidden, or moved under a hidden node) | The window zooms out | `debug`: `zoom cleared`, inside that operation's `layout-op` group |
| The node has no view, because its component or slot was refused when the window opened | Refused, or cleared when the window opens | `INFO` when refused, `debug` when cleared |
| An ID breaks the slug rule | Refused | `ERROR` |

## How the window looks

- While a node is zoomed, the shell draws only that node's view, filling the window on the theme's background.
- The views of every other node stay alive and are not drawn. This is how a hidden node is kept in feature 7.
- Zooming out draws the whole tree again, with the same views, shares, and tab selections.

## Logging

- `set_zoomed` runs in a group, `zoom{node, extension}`, and `zoom_out` in a group `zoom-out{extension}`.
- Inside each, the shell logs `zoomed` or `zoomed out` at `debug`, or the refusal.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` | A dev version beside production: the zoomed node, `set_zoomed` and `zoom_out` on `Extension`, drawing the zoomed view only, and clearing the zoom when an operation removes or hides the zoomed node |
| `riso/startup` | A dev copy that uses the dev shell |

## Test extension: `examples/layout_tour`

The card gains a line, `zoom` or `zoom out`. Clicking it zooms its own node, or zooms out.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=zoom cargo run dev` | A row: card `a`, and tabs `right` with cards `b` and `c`. |
| Count `a` to 2, drag the row's line, select tab `c`, then click `zoom` on `c` | Card `c` fills the window, under `zoom{node=c}`. |
| Same, click `zoom out` | The row is back with the same line position, `c` still selected, and `a` still at `count: 2`. |
| `LAYOUT_TOUR=zoom-root cargo run dev` | The tour asks to zoom `root` at startup. It is refused with an `INFO`. |

## Under review

`riso_shell/shell` and `riso/startup`, in their dev versions.

## Not in this feature

- A keybinding or action to zoom, as Zed's `ToggleZoom`.
- Saving which node is zoomed.
- Animations.
