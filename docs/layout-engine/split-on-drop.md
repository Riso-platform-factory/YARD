# Layout feature 26: split-on-drop

## What it adds

Dropping a tab on any edge of a container's child puts a new split around that child, with the dropped node in a new tabs container beside it, on the side of the edge, as Zed splits a pane when a tab is dropped on its edge. Dropping a tab on the center of a tabs container's content moves the node into that container. Until now (feature 9), only the two edges along a split's axis were drop targets, and a drop there moved the node into the existing split.

## How it works

As Zed's `Pane::handle_drag_move` and `handle_tab_drop` (`crates/workspace/src/pane.rs`):

- **Where a drop lands.** Over a node shown in a tabs container or in a split, the pointer is in one of five zones:
  - the four edge zones, each the outer 20% of the node's shorter side, Zed's `drop_target_size`, as feature 9;
  - the center, the rest.

  While dragging, the zone is highlighted: an edge highlights the half of the node on that side, as Zed's overlay does, and the center highlights the whole node.
- **An edge along the parent split's axis** (left or right in a row, top or bottom in a column) keeps feature 9's meaning: the node moves into that split, before or after the child. Nothing new is made.
- **Any other edge** splits. The target is the node itself when its parent is a split (an edge across the split's axis), and the tabs container when the node is shown in one (any of its four edges), as Zed splits the pane, not the item. A new `yard.row` (for left and right) or `yard.column` (for top and bottom) takes the target's place in its slot, holding the target and a new `yard.tabs` on the side of the edge, which holds the dropped node. The new split gives its two children equal shares.
- **The center** of a node shown in a tabs container moves the dropped node into that container, last, and selects it. The center of a node in a split, not in tabs, does nothing, as today.
- **As operations.** A split on drop is recorded as the operations of feature 8 that make it, in this order: `add` the new split at the target's index in the target's slot, `move` the target into it, `add` the new tabs container on the edge's side, `move` the dropped node into it. They are recorded, saved, restored, and reset like any others. If any of them is refused, the ones before it are undone, so a drop is all or nothing, and the refusal is logged once with its `INFO`.
- **New node IDs** are `split-<n>` and `tabs-<n>`, with `n` the smallest number for which the ID is free.
- **Rules.** The shell's rules hold for each operation: required nodes (feature 15) may be split around, since nothing is hidden or removed; a slot for one (feature 16) refuses the new split if it would hold two nodes; contexts (feature 18) are checked for the dropped node at its new place. A drop of a node onto itself or into its own subtree is refused with an `INFO`, as a cycle.
- **Empty leftovers.** When the dropped node leaves a tabs container empty, that container stays, as today; feature 23's emptiness is for components that say so, and an empty tabs container is not one of them.

## What code can do

Nothing new is public: the change is in how the dev shell's tabs containers and splits handle drops.

## Logging

| Situation | Log |
|---|---|
| A split on drop | A group `split-on-drop{node, target, edge}`, holding the four `layout-op` groups of the operations |
| A refusal | `INFO` in the operation's group, and `debug` `split on drop undone {done}` with the number of operations undone |
| The zone under the pointer changes during a drag | `trace` `drop zone {target, zone}` |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_shell/shell` dev version | Five drop zones on every node shown in a tabs container or a split, the split on drop, the center drop into tabs, and the undo of a partial drop |
| `yard/startup` | Dev copy that uses the dev shell. While `split-on-drop` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- GPUI tests, with the dev shell:
  - a tab dropped on the top edge of a card in a row makes a `yard.column` in the card's place, with a new `yard.tabs` holding the tab above the card; the row's other children are unchanged;
  - the left edge of a card in a row moves the tab into the row before the card, as feature 9;
  - an edge of a tab's content in a tabs container splits around the tabs container, not around the shown child;
  - the center of a tabs container's content moves the tab into it, last, selected;
  - the four operations are recorded in order, saved, restored, and reset;
  - a split refused by a rule leaves the tree as it was, with nothing recorded;
  - new IDs are the smallest free `split-<n>` and `tabs-<n>`.
- **Logging**, alone in its file: `split-on-drop{node, target, edge}` holds four `layout-op` groups.

## Test extension: `examples/layout_tour`

The case `user-layer` (feature 11) is used as it is: a row with `left` (tabs `a`, `b`) and `right` (tabs `c`, `d`).

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-splitdrop LAYOUT_TOUR=user-layer cargo run dev`, drag tab `b` onto the top edge of `right`'s content | While dragging, the top half of `right`'s content is highlighted. After the drop, a column stands where `right` was: a new tabs container with `b` above `right`; `split-on-drop{node=b, target=right, edge=top}` with four `layout-op` groups; `user layer: 4`. |
| Drag tab `d` onto the center of `left`'s content | `d` moves into `left`, last, and is selected. |
| Quit and start again | The same layout. |
| `reset layout` | `left` with `a`, `b` and `right` with `c`, `d` again. |

## Under review

- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- Removing a tabs container that a drop left empty: when a feature needs it.
- Dragging a node that is not a tab.
