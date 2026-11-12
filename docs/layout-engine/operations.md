# Layout feature 8: operations

## What it adds

While the application runs, the tree changes only through operations: add, remove, move, hide, and show.
- The engine checks each operation against the rules before it applies it.
- A refused operation changes nothing and is logged.
- After an operation, the window shows the new tree:
  - new nodes get views;
  - removed nodes lose theirs;
  - moved and hidden nodes keep theirs.

## Operations

| Operation | What it does |
|---|---|
| `add(node, component, parent, slot)` | Places a new node at the end of a slot, as `place` does at startup |
| `remove(node)` | Takes a node and everything under it out of the tree |
| `move(node, parent, slot, index)` | Takes a node, with everything under it, to a slot of another node, or to another place in its own slot. `index` is the position in the target slot; past the end means last. |
| `hide(node)`, `show(node)` | As in feature 7 |

An extension asks for them through its handle. `place_root` and `place` stay for startup. After startup, they are the same rules as `add`.

## Rules

The model in `riso_layout` checks what it can know without components. The shell checks the rest, because it knows the components.

| Rule | Checked by | Log when refused |
|---|---|---|
| An ID or slot name breaks the slug rule | shell | `ERROR`: a bug of the extension |
| `add`: the node ID is already in the tree, or the parent is not | model | `INFO` |
| `add`: the component is not registered, is not in the extension's namespace or `riso`, or the parent's component does not declare the slot | shell | `INFO` |
| `remove`, `move`, `hide`, `show`: the node is not in the tree, or is the root | model | `INFO` |
| `move`: the target parent is not in the tree | model | `INFO` |
| `move`: the target parent is the node itself or is under it, which would make a cycle | model | `INFO` |
| `move`: the target parent's component does not declare the slot | shell | `INFO` |

- The rule "no cycle" of feature 2 is now checked, because `move` is the first operation that can break it.
- Every operation that is refused leaves the tree exactly as it was.
- Which extension may change which node is not restricted. Any extension may remove, move, hide, or show any node, as any part of Zed may close or move any item. Only `add` keeps feature 3's namespace rule, because it creates a node of a component.

## How the window follows

- **Views are kept by node ID for the life of the window.**
  - A container asks its slots for its children at render time, from the tree. Adding, removing, and moving therefore show up without rebuilding containers.
  - A moved node keeps its view and its state.
  - A removed node's view, and the views under it, are dropped.
  - An added node's view is created when it is added, inside `component-create`.
- **Shares and tab selections follow nodes.**
  - A split keeps a share for each node that is still its child.
  - A tabs container whose selected child was moved away or removed selects its first shown child.
- **The window redraws** after every applied operation.

## Logging

- Each operation runs in a group, `layout-op{op, node, extension}`.
- Inside it:
  - the model logs the change at `debug`, or the refusal at `INFO`;
  - the shell logs each view it creates, in `component-create`;
  - the shell logs each view it drops, at `debug`: `view dropped {node}`.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_layout/tree` | Dev version: `Operation`, `Tree::apply` with the model's rules, and the cycle check |
| `riso_shell/shell` | Dev version: `add`, `remove`, `move_node`, and `set_hidden` on `Extension`, all through one path that checks the shell's rules, applies the operation, and reconciles views. Slots read the tree at render time. |
| `riso/startup` | Dev copy that uses the dev shell |

## Test extension: `examples/layout_tour`

`layout_tour.controls` gets a list of buttons, given by the case, each calling one operation.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=operations cargo run dev` | A row: `controls`, card `a`, and `right`, a column with cards `b` and `c`. |
| Click `add d` | Card `d` appears as the last child of the row. `component-create{node=d}` sits inside `layout-op{op=add, node=d}`. |
| Click `b`'s count twice, then `move b to root` | `b` leaves the column and becomes the last child of the row, still at `count: 2`. |
| Click `remove a` | `a` goes away, and `view dropped {node=a}` is logged. |
| Click `move right into c` | Refused with an `INFO`: `c` is under `right`, so the move would make a cycle. Nothing changes. |
| Click `move c to b.side` | Refused with an `INFO`: the card declares no slot `side`. |
| Click `remove root` | Refused with an `INFO`. |
| Click `add d` again | Refused with an `INFO`: `d` is already in the tree. |

## Under review

- `riso_layout/tree` and `riso_shell/shell`, in their dev versions
- `riso/startup`, as a dev copy

## Not in this feature

- Dragging nodes with the mouse: feature 9.
- Undo.
- Keeping the user's operations apart from the default layout, and saving them: features 11 and 12.
- Restrictions on which extension may change which node.
