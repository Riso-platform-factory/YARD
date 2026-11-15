# Layout feature 17: placement-order

## What it adds

A placement, an `add`, or a `move` can say where in the slot the node goes by naming another node: first, before a named node, or after it, instead of a position by number. An extension can then put its item before another extension's item without knowing how many items are there, as Zed's status bar items are placed beside named neighbors. A position names a node that is in the target slot; one that is not refuses the change with an `INFO`.

## The model

In `riso_layout`, without GPUI:

- **`Position`** is one of `Last` (the default), `First`, `Index(n)` (as today; past the end means last), `Before(node)`, and `After(node)`.
- **`Operation::Add` and `Operation::Move`** carry a `Position` instead of `Move`'s index. `Add` was always last; it now takes a position too.
- **`Tree::place_at(node, parent, slot, position)`** places a node at startup at a position. `Tree::place` stays, and means `Last`.
- **Resolving a position** happens when the change is applied, against the slot as it is then:
  - `Before(x)` and `After(x)`: `x` must be in the target slot, or the change is refused with an `INFO`: `{x} is not in {parent}.{slot}`;
  - for a `move`, `x` may not be the node moved itself, which is refused with an `INFO`: `{node} cannot be placed beside itself`;
  - for a `move` within its own slot, the position is resolved after the node is taken out, so `After(x)` always means right after `x`.
- **The user layer** records the position as it was asked. A drag still records the index where the tab was dropped, as today.

## What is saved

`layout.json` keeps `version` 1, and old files still read:

- a `move` or an `add` has at most one of `index` (a number, as today), `first` (`true`), `before` (a node ID), or `after` (a node ID); none means last;
- more than one of them is a file that cannot be read, as feature 12 says: a `WARN`, and the default layout.

A saved `before` or `after` whose node is no longer in the slot is skipped at restore with an `INFO`, as any refused operation.

## In the shell

- **`place_at(cx, node, component, parent, slot, at)`**, **`add_at(window, cx, node, component, parent, slot, at)`**, and **`move_to(cx, node, parent, slot, at)`** take a `Position`. The existing `place`, `add`, `move_node`, and the `*_with_props` placements keep their meaning: last, last, `Index`, and last.
- **`Placement`**, for a placement that needs both props and a position: `place_with(cx, node, component, parent, slot, Placement { props, at })` and `add_with(window, cx, …, Placement { props, at })`.
- **At startup,** extensions are loaded in the order of the composition root, so a placement can name only nodes placed before it. One that names a node not yet placed is refused with the `INFO`, and the node is not placed.

## What code can do

```rust
// riso_layout (production, in place)
pub enum Position { Last, First, Index(usize), Before(NodeId), After(NodeId) }
pub enum Operation {
    Add { node: Node, parent: NodeId, slot: SlotName, at: Position },
    Move { node: NodeId, parent: NodeId, slot: SlotName, at: Position },
    /* … unchanged … */
}
impl Tree { pub fn place_at(&mut self, node: Node, parent: &NodeId, slot: SlotName, at: Position) -> Result<(), PlaceError>; }

// riso_shell::dev
pub struct Placement { pub props: serde_json::Value, pub at: riso_layout::Position }
impl Extension {
    pub fn place_at(&self, cx: &mut App, node: &str, component: &str, parent: &str, slot: &str, at: Position);
    pub fn place_with(&self, cx: &mut App, node: &str, component: &str, parent: &str, slot: &str, placement: Placement);
    pub fn add_at(&self, window: &mut Window, cx: &mut App, node: &str, component: &str, parent: &str, slot: &str, at: Position);
    pub fn add_with(&self, window: &mut Window, cx: &mut App, node: &str, component: &str, parent: &str, slot: &str, placement: Placement);
    pub fn move_to(&self, cx: &mut App, node: &str, parent: &str, slot: &str, at: Position);
}
```

`Placement::props` is `Value::Null` for no props, or an object, as feature 13 reads it. The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| A change at a named position | As today, in `node-place` or `layout-op`; the model logs the resolved index at `debug`: `placed {index}` or `moved {index}` |
| A position whose node is not in the slot, or a node beside itself | `INFO` in the same group |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_layout/tree` (production, in place) | `Position`, `Add` and `Move` with a position, `place_at`, and the resolving rules. Stamped again. |
| `riso_layout/saved` (production, in place) | `first`, `before`, `after` on `add` and `move`. Stamped again. |
| `riso_shell/shell` (production, in place) | Only what the changed variants need: `move_node` and drags build `Position::Index`, and placements build `Position::Last`. Stamped again. |
| `riso_shell/shell` dev version | `Placement`, `place_at`, `place_with`, `add_at`, `add_with`, `move_to` |
| `riso/startup` | Dev copy that uses the dev shell. While `placement-order` is under review, `cargo run dev` loads `review`, `layout_tour`, and `layout_guest` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **Model:**
  - `First`, `Before`, `After`, `Index`, and `Last` put a node where they say, for `place_at`, `add`, and `move`;
  - a `move` within its own slot with `After(x)` lands right after `x`, whether the node was before or after `x`;
  - `Before(x)` with `x` in another slot, or not in the tree, is refused and the tree is unchanged;
  - a `move` beside itself is refused;
  - the user layer records the position as asked.
- **Saved format:** each position encodes and decodes back; a file of feature 12 still decodes; two positions on one operation are refused.
- **Shell, GPUI tests, with the dev shell:**
  - a second extension places its node before the first extension's node at startup;
  - a placement naming a node not yet placed is refused with the `INFO`;
  - `add_with` places a node with props at a position;
  - `move_to` with `First` moves a tab to the front of its bar;
  - a saved `move` with `after` is restored to the same place; one whose anchor is gone is skipped with an `INFO`.
- **Logging**, alone in its file: `placed {index}` inside `node-place`; the `INFO` of a missing anchor inside `layout-op{op=move}`.

## Test extension: `examples/layout_tour` and `examples/layout_guest`

A new case, `placement-order`. `layout_tour` places, in a row, the controls, then cards `a`, `b`, and `c`. `layout_guest`, the second extension of the tour, loaded after it, places its card `guest` before `b`, without knowing how many cards there are.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-order LAYOUT_TOUR=placement-order cargo run dev` | The row: controls, `a`, `guest`, `b`, `c`. `placed {index=2}` in `node-place{node=guest}`. |
| Click `c first` | `c` moves to the front of the row, before the controls. |
| Click `a after b` | `a` moves right after `b`. |
| Click `b before ghost` | Refused with an `INFO`: `ghost is not in root.children`. |
| Click `a after a` | Refused with an `INFO`: `a cannot be placed beside itself`. |
| Quit and start again | The same order: the saved moves are restored by the names they gave. |
| `reset layout` | Controls, `a`, `guest`, `b`, `c`. |

## Under review

- `riso_layout/tree`, `riso_layout/saved`, and production `riso_shell/shell`, in place
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- A placement that waits until the named node is placed, so that the order of extensions does not matter: when a feature needs it.
- Undo.
