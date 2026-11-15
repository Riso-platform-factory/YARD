# Layout feature 16: slot-count

## What it adds

A slot says whether it holds one node or many. Every slot holds many, as today, unless its component declares it a slot for one, such as the place for a panel's single content or a title bar's center. A second node in a slot for one is refused with an `INFO`, whether it is placed, added, moved, or restored, and nothing changes.

## How it works

- **Declaring.** `Extension::slot_for_one(cx, component, slot)` declares a slot of a component of the extension a slot for one. It is refused with an `ERROR`, a bug of the extension, when the component ID or the slot name is invalid, the component is not in the extension's namespace, or the slot is already declared for one. It can be called before or after the component is registered. When the component is registered, a slot for one that the component does not declare is an `ERROR`, and the declaration is dropped.
- **The rule** is a rule of the shell, because the model does not know components. It is checked with the shell's other rules, before the model applies a change:

| Change | Refused when | Log |
|---|---|---|
| `place` at startup, `add` | the target slot is for one and already holds a node | `INFO`: `{parent}.{slot} holds one node, and already holds {node}` |
| `move` | the same, unless the node moved is the one the slot holds (moving it within its own slot is allowed) | the same `INFO` |
| a saved `add` or `move`, at restore | the same | `INFO` `skipped`, with the same reason, as feature 12 skips any refused operation |

- A drag onto a tab bar or an edge is a `move`, so the rule holds there too: the drop is refused, and the dragged node stays where it was.
- A hidden node still holds its slot: a slot for one whose node is hidden refuses another.

## What code can do

```rust
// riso_shell::dev
impl Extension {
    pub fn slot_for_one(&self, cx: &mut App, component: &str, slot: &str);
}
```

The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| A slot is declared for one | A group `slot-declare{component, slot}`, with `debug` `slot for one`; an `ERROR` for a refusal |
| A placement refused by the rule | `INFO` in `node-place{node, …}` |
| An operation refused by the rule | `INFO` in its `layout-op{op, node, extension}` |
| A saved operation refused at startup | `INFO` `skipped`, in `layout-restore` |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` dev version | `slot_for_one`, and the rule in the shell's rules for `place`, `add`, and `move` |
| `riso/startup` | Dev copy that uses the dev shell. While `slot-count` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- GPUI tests, with the dev shell:
  - placing a second node in a slot for one at startup is refused with the `INFO`, and the first stays;
  - `add` and `move` into a full slot for one are refused, and the tree is unchanged;
  - an empty slot for one takes a node, by `add` and by `move`;
  - moving the slot's own node to index 0 of the same slot is allowed;
  - a slot for one whose node is hidden still refuses another;
  - a slot not declared for one takes many, as before;
  - a saved `move` into a full slot for one is skipped at restore with an `INFO`, and the next operations are still applied;
  - `slot_for_one` before and after `register` both work; declared for a slot the component does not have is an `ERROR` at registration;
  - `slot_for_one` refused for another extension's component, an invalid ID or slot name, and a second time, each with an `ERROR`.
- **Logging**, alone in its file: `slot-declare{component, slot}` with `slot for one`; a refused `add` logs its `INFO` inside `layout-op{op=add}`.

## Test extension: `examples/layout_tour`

A new case, `slot-count`. Its card `layout_tour.frame` has a slot `content` for one, which shows its node under a header. The layout is the controls, then a row with `frame` (a `layout_tour.frame` holding `first`, a card) and `loose`, a card.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-slot-count LAYOUT_TOUR=slot-count cargo run dev` | `frame` showing `first`, and `loose`. `slot-declare{component=layout_tour.frame, slot=content}`. The case also tries to place a second card, `extra`, in `frame.content` at startup: an `INFO`, `frame.content holds one node, and already holds first`, and no `extra`. |
| Click `add second to frame` | Refused with the `INFO`. |
| Click `move loose into frame` | Refused with the `INFO`. |
| Click `move first to root` | `first` leaves the frame, which is now empty. |
| Click `move loose into frame` | `loose` is in the frame. |

## Under review

- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Declaring the shell's own slots (`riso.window`, `riso.body`) for one: when a feature needs it.
- A slot that holds at most some other number of nodes.
