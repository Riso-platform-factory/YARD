# Layout feature 2: slots

## What it adds

A component type declares named slots. A node can be placed in a slot of another node, so the tree has more than a root. A component's view shows the nodes of its slots where it chooses. A placement or a tree that breaks a rule is refused, and the log says why.

## Model

- A component type declares its slots when it is registered: a list of names, each a slug, such as `body`. A component may declare none.
- A slot holds any number of nodes, in the order in which they were placed.
- A node is placed either as the root, as in feature 1, or in a slot of a node that is already placed, named by that node's ID and the slot's name.
- The tree is the root and, under each node, the nodes of each of its slots.

## Rules

There are two times when the rules are checked.

**When a node is placed**, the tree alone is enough to check these:

| Situation | What happens | Log |
|---|---|---|
| An ID or a slot name breaks the slug rule | Refused | `ERROR`: a bug of the extension |
| The node ID is already in the tree | Refused; the first stays | `INFO`: two placements conflict, as with a second root |
| The parent node is not in the tree, because it was never placed or has not been placed yet | Refused | `INFO` |

**When the window opens**, the components are known, so these are checked:

| Situation | What happens | Log |
|---|---|---|
| A component declares the same slot twice | The registration is refused | `ERROR`, at registration |
| A node's component is not registered | The node and everything under it are not shown; its siblings are | `ERROR` |
| A node is placed in a slot that its parent's component does not declare | The node and everything under it are not shown | `ERROR` |

- A node is placed only under a node that already exists, and it starts with no children. So no placement can create a cycle. The rule "no cycle" is checked from feature 8, `operations`, when nodes can move.
- Placing a parent before its children is the extensions' job. Extensions load in the order of the startup's list, so an extension that places nodes in another one's slots loads after it.

## How a component shows its slots

- The function that creates a view also receives a handle to its node's slots.
- In its render, the view asks the handle for the views of one slot, in order, and puts them where it wants.
- The engine does not decide the layout inside a component. Arranging children in a row or a column is feature 4, `split`.

## Lifecycle

- When the window opens, the shell creates the view of every node that is shown, starting from the root, a parent before its children.
- Each view is created once and kept while the window is open.

## Logging

- `component-register` gains a field `slots`.
- A new group, `node-place`, has the fields `node`, `component`, `parent`, and `slot`. It sits inside the extension's `init`, like `root-place`.
- `component-create` groups nest as the tree does. A child's group is inside its parent's, so the trace has the tree's shape.
- Every refusal is logged inside the group of the work that was refused.

## Changing parts that are in production

This feature changes `yard_layout/tree` and `yard_shell/shell`, which are in production. The changes are not small: the factory function gains the slots handle, and the tree gains its nodes.

The new versions are written in `src/dev/` of their crates. The root file of each crate compiles either the production module or the dev one, never both:
- the production module only without `dev`;
- the dev module only with `dev`.

So:
- `cargo run` stays as approved;
- `cargo run dev` runs the new versions;
- promotion moves each dev file over its production file.

This is how the startup was changed in the earlier features, applied to a crate.

The palette gains the color `border`, so that a card inside a card can be seen. It is added to `yard_ui/theming` in place, as `text` was, and stamped again when you approve.

## Test extension: `examples/layout_tour`

The extension is loaded again for the review.

The card gains a slot `body`.
- It draws a border, with its ID and size at the top.
- Under them, it shows the nodes of its `body` in a column, each taking an equal share.

The cases of feature 1 keep working.

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=nested cargo run dev` | The card `root` holds `left` and `right`, and `left` holds `inner`. Each card shows its own ID and size. In the trace, the `component-create` groups nest as the cards do. |
| `LAYOUT_TOUR=duplicate-id cargo run dev` | `left` is placed twice. The second `node-place` is refused with an `INFO`, and one `left` is shown. |
| `LAYOUT_TOUR=missing-parent cargo run dev` | A node is placed in `nowhere.body`. It is refused with an `INFO`, and only `root` is shown. |
| `LAYOUT_TOUR=missing-slot cargo run dev` | A node is placed in `root.side`, which the card does not declare. There is an `ERROR` in `window-open`, and only `root` is shown. |
| `LAYOUT_TOUR=missing-child cargo run dev` | `left` names an unregistered component, and `inner` is placed under it. There is an `ERROR`; neither `left` nor `inner` is shown, and `right` is. |
| `LAYOUT_TOUR=duplicate-slot cargo run dev` | A component that declares `body` twice. Its registration is refused with an `ERROR`. |

## Under review

`yard_layout/tree` and `yard_shell/shell`, in their dev versions, and the color added to `yard_ui/theming`. They are reviewed and promoted together.

## Not in this feature

- An extension placing nodes in a component it does not own, and the slot staying empty without that extension: feature 3.
- A slot that holds only one node.
- Props.
- Rows, columns, and resizing: features 4 and 5.
- Moving and removing nodes: feature 8.
