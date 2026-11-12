# Layout feature 11: user-layer

## What it adds

The engine keeps the default layout apart from what changed it while the application runs. The changes form the user layer, a list of operations in the order they were applied. Resetting the layout drops the user layer and brings back the default layout.

## Model

- **The default layout** is the tree as the extensions built it when the application started: every `place_root` and `place` made in the extensions' `init`. The default layout is fixed when the window opens.
- **The user layer** is every operation applied after that, in order: an extension's `add`, `remove`, `move_node`, `set_hidden`, and the shell's moves from a drag.
  - A refused operation is not recorded.
  - The current tree is always the default layout with the user layer applied.
- **Kept beside the user layer, not in it:** the shares of a split, the selected tab, and the zoom. They are view state. Feature 12 decides what to save of them.
- **The model lives in `riso_layout`.** A new part, `riso_layout/layers`, holds the default tree, the list of operations, and the current tree. Recording and resetting are checked there, without GPUI.

## Reset

An extension asks for a reset through its handle, `reset_layout()`. Any extension may ask, as any part of an application may offer a "reset layout" command.

| Before the reset | After the reset |
|---|---|
| A node the user layer added | Removed, and its view is dropped |
| A node the user layer removed | Back in its default place, with a new view; its old state is gone |
| A node moved, hidden, or shown | Back in its default place and state, with the same view and state |
| Shares of a split | Equal again, as the default layout has them |
| The selected tab | The first tab again |
| A zoom | Cleared |
| The user layer | Empty |

Resetting with an empty user layer changes nothing, and logs that.

## Logging

- Recording an applied operation logs `recorded`, with the number of operations in the user layer, at `debug`. It is logged inside the operation's `layout-op` group.
- A reset runs in a group, `layout-reset{extension}`. Inside it:
  - `debug` gives the number of operations dropped;
  - every view created logs in `component-create`;
  - every view dropped logs `view dropped`.
- An empty reset logs `nothing to reset` at `debug`.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_layout/layers` (new, dev) | The default tree, the user layer, recording, reset, and the current tree |
| `riso_layout/tree` (production, in place) | `Tree` derives `Clone`, so that the default tree can be kept. Stamped again with this feature. |
| `riso_shell/shell` | Dev version beside production. Its registry keeps a `Layers` instead of a bare `Tree`. It fixes the default when the window opens, records every applied operation, offers `reset_layout`, and reconciles views, shares, tabs, and zoom after a reset. |
| `riso/startup` | Dev copy that uses the dev shell |

## Test extension: `examples/layout_tour`

The controls gain a `reset layout` button and a line `user layer: N`. `N` is the number of recorded operations, read through a small shell query, `user_layer_len()`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=user-layer cargo run dev` | A row: `controls`, then two tabs containers, `left` with `a` and `b`, and `right` with `c` and `d`. `user layer: 0`. |
| Count `b` to 2. Drag tab `a` onto `right`'s bar, drag the row's line, select `c`, hide `d`. | `user layer: 2`: the move and the hide. The resize and the tab selection are not operations. |
| Click `reset layout` | `left` has `a` and `b` again, `right` has `c` and `d`, the row is in equal shares, the first tabs are selected, `b` still shows `count: 2`, and `user layer: 0`. `layout-reset` logs 2 operations dropped. |
| Click `reset layout` again | `nothing to reset`. |

## Under review

- `riso_layout/layers`
- `riso_layout/tree`, in place
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Saving the user layer and applying it at the next start: feature 12.
- Undoing one operation.
- Several named layouts.
