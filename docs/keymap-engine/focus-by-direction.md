# Keymap feature 4: focus-by-direction

## What it adds

Four actions move the focus to the node on the left, on the right, above, or below the focused one, by the nodes' places on screen. With their keys, the user walks through the cards of a layout without the mouse.

## The actions and their keys

| Action | macOS | Linux and Windows | As Zed's |
|---|---|---|---|
| `riso::FocusLeft` | `cmd-k cmd-left` | `ctrl-k ctrl-left` | `workspace::ActivatePaneLeft` |
| `riso::FocusRight` | `cmd-k cmd-right` | `ctrl-k ctrl-right` | `workspace::ActivatePaneRight` |
| `riso::FocusUp` | `cmd-k cmd-up` | `ctrl-k ctrl-up` | `workspace::ActivatePaneUp` |
| `riso::FocusDown` | `cmd-k cmd-down` | `ctrl-k ctrl-down` | `workspace::ActivatePaneDown` |

Each key is two keystrokes. GPUI waits up to a second for the second one. The keys are bound with the other defaults, in `bind_default_keys`, in the context `Shell`.

## How the target is chosen

As Zed chooses a pane:

- **The candidates are the shown leaf nodes:** shown nodes with no shown child node. A row and a column are never targets, nor a tabs container. Only the selected tab of a tabs container is shown, and only the zoomed node's subtree when a node is zoomed. A container can still have the focus, after a click on a tab bar, and is then where the move starts.
- **The place is the node's bounds in the last draw.** Every node's wrapper records them while it draws.
- **The shell takes the point 4px past the focused node's edge, in the direction,** at the middle of that edge. The target is the leaf node whose bounds contain it.
- **No node there** means nothing moves: there is no wrapping around.

## Rules and logging

Each move runs in the group of the other actions, `layout-action{action, node}`:

| Situation | What happens | Log |
|---|---|---|
| A leaf node is in that direction | It gets the focus | `debug`: `done`, with `target` |
| No node has the focus | Nothing | `info`: `refused`, reason `no node has the focus` |
| No node in that direction | Nothing | `info`: `refused`, reason `no node on the left`, `on the right`, `above`, or `below`, with the focused node's `bounds` and the probe `point` |
| The focused node has no bounds yet | Nothing | `info`: `refused`, reason `no bounds for the focused node` |

At `trace`, each candidate considered is logged with its bounds, so that a refusal can be explained from the log.

The move of the focus then logs `focus-change{node}`, as any move of the focus does.

## What code can do

```rust
pub fn node_bounds(cx: &App, node: &str) -> Option<Bounds<Pixels>>;
```

`node_bounds` gives a node's bounds in the last draw, or nothing if it was not drawn. Tests use it, and so will features that place things by a node, such as an inspector.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` | Dev version beside production. The four actions, the bounds of each drawn node, the choice of the target, and their keys in `bind_default_keys` |
| `riso/startup` | Unchanged: it already calls `bind_default_keys` |

## Tests

- GPUI tests, after a draw:
  - in a row of `a`, `b`, `c`: from `b`, left gives `a` and right gives `c`; from `a`, left is refused;
  - in a row of `a` and a column `right` of `b` above `c`: from `a`, right gives `b` or `c`, whichever is at the middle of `a`'s right edge; from `b`, down gives `c`; from `c`, left gives `a`;
  - in tabs, only the selected tab's card is a target;
  - from a focused tabs container, a move starts from its bounds;
  - with no node focused, each is refused.
- On macOS, the keys `cmd-k cmd-right` and the others move the focus.
- A unit test of the bindings: the keys of the table, in the context `Shell`.
- A logging test alone in its file: every row, with its level and its group.

## Test extension: `examples/layout_tour`

The case `nested-split`, a row of `a` and a column `right` of `b` above `c`, shows the moves. Use a fresh home: `RISO_HOME=/tmp/riso-layout`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=nested-split cargo run dev`, click `a`, then `cmd-k cmd-right` | `b` or `c` says `focused`, whichever is at the middle of `a`'s right edge. |
| Click `b`, then `cmd-k cmd-down` | `c` says `focused`. `cmd-k cmd-left`: `a`. |
| From `a`, `cmd-k cmd-left` | Refused: `no node on the left`. |

## Under review

- `riso_shell/shell`, in its dev version, with `riso/startup`'s dev copy to use it

## Not in this feature

- Moving the focus into a dock or a panel: riso has none.
- Moving a node itself by direction, and resizing a split by keys: later.
