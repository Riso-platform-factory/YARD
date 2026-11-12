# Layout feature 9: drag-to-move

## What it adds

The user drags a tab with the mouse and drops it somewhere else in the layout. The node moves there, with its view and its state. While the user drags, the place where the node would land is highlighted. The move is an ordinary `move` operation, checked by the same rules as any other.

## What can be dragged, and where to

This follows Zed's `pane.rs`: tabs are dragged, and tab bars and the edges of an item are where they can be dropped.

- **What can be dragged.** Any tab of a `riso.tabs` container. What moves is the tab's child node.
- **Onto a tab bar.**
  - Dropping on a tab bar moves the node into that tabs container's `children`.
  - Dropped on a tab, the node lands at that tab's position.
  - Dropped on the empty part of the bar, it lands last.
  - The tab or the bar under the cursor is highlighted.
- **Onto an edge.**
  - Every child of a split has edge zones. An edge zone is the outer 20% of the child's shorter side, Zed's default `drop_target_size`.
  - Only the two edges along the split's axis are drop targets: left and right in a row, top and bottom in a column.
  - Dropping on an edge moves the node into that split, just before or just after the child.
  - The half of the child on that side is highlighted, as Zed highlights where a split would go.
- **Anywhere else, nothing happens.** That includes the middle of an item, an edge across the split's axis, and outside a tabs container or split. Making a new split on drop, as Zed does, is not part of this feature.
- **The highlight color** is the theme's `drop_target_background`. It is added to `riso_ui`'s palette as `drop_target`.

## Rules

A drop asks for `move(node, parent, slot, index)`, with the rules of feature 8. Two of them matter here:

- **A cycle.** Dragging the tab of a tabs container's child onto that same container's own edge is a cycle when the node is the container's ancestor. It is refused with an `INFO`. The drag ends and nothing moves.
- **Dropping where it already is.** Dropping a tab on its own position changes nothing, and is not logged as a move.

## Who asks for the move

A drag is the user's action, not an extension's. The shell asks for the move under its own name, `riso`, the namespace no extension may use. So the group is `layout-op{op=move, node, extension=riso}`.

## Logging

- Starting a drag logs `drag started {node}` at `debug`.
- Moving over targets logs at `trace` only. That is high-frequency work.
- A drop that asks for a move runs `layout-op` as above.
- A drop that does nothing logs `drop ignored {node}` at `debug`.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/tabs` | Dev version beside production. A tab can be made draggable, carrying a value, and a tab or tab bar can accept a drop, with Zed's highlight. |
| `riso_ui/theming` | Production, in place. The palette gains `drop_target`. It is stamped again with this feature. |
| `riso_shell/shell` | Dev version beside production. Tabs are drag sources. Tab bars and split children's edges are drop targets. A drop asks for `move` as `riso`. |
| `riso/startup` | Dev copy that uses the dev shell |

## Test extension: `examples/layout_tour`

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=two-tabs cargo run dev` | A row with two tabs containers. `left` has tabs `a` and `b`, `right` has tabs `c` and `d`. |
| Count `a` to 2, then drag tab `a` onto `right`'s tab bar, after `d` | While dragging, `right`'s bar is highlighted. After the drop, `right` has `c`, `d`, `a`, and `a` still shows `count: 2`. `layout-op{op=move, node=a, extension=riso}`. |
| Drag tab `c` onto the right edge of `left` | While dragging, the right half of `left` is highlighted. After the drop, the row is `left`, `c`, `right`, and `c` is a card of its own, a third of the row. |
| Drag tab `b` onto the middle of card `c` | No highlight. `drop ignored`, and nothing moves. |
| `LAYOUT_TOUR=tabs-in-split cargo run dev`, then drag tab `b` onto the left edge of card `a` | The row becomes `b`, `a`, `right`. |

## Under review

- `riso_ui/tabs`, `riso_shell/shell`: dev versions
- `riso_ui/theming`: in place
- `riso/startup`: dev copy

## Not in this feature

- Making a new split on drop.
- Dragging cards that are not tabs.
- Dragging between windows.
- Keyboard moves.
