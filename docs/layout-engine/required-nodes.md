# Layout feature 15: required-nodes

## What it adds

A component can be declared required, such as the editor area of an application, which must always be on screen. A node of a required component cannot be hidden or removed, and neither can a node above it, since hiding or removing that would take it off screen too. Such a request is refused with an `INFO`, and nothing changes. A required node can still be moved, zoomed, and have its props set.

Zed has the same rule without naming it: its center pane group cannot be closed or hidden, only the docks around it.

## How it works

- **Declaring.** `Extension::require(cx, component)` declares a component of the extension required. It is refused with an `ERROR`, a bug of the extension, when the component ID is invalid, is not in the extension's namespace, or is already required. It can be called before or after the component is registered.
- **The rule** is a rule of the shell, because the model does not know components. It is checked with the shell's other rules, before the model applies an operation:

| Operation | Refused when | Log |
|---|---|---|
| `hide(node)` | `node`, or a node under it, is of a required component | `INFO`: `{node} cannot be hidden: it holds the required node {required}` (or `it is required`) |
| `remove(node)` | the same | `INFO`: `{node} cannot be removed: …`, in the same words |
| `show`, `move`, `add`, `set-props`, zoom | never, by this rule | |

  `{required}` names the first required node found under `node`, in the order of the tree.
- **Everywhere an operation comes from.** The rule holds for an extension's call, for the `yard::Hide` action of the keymap, for a drag, and for a saved operation being restored at startup: a saved `hide` or `remove` that the rule refuses is skipped with an `INFO`, as feature 12 skips any refused operation.
- **The default layout** is not checked: a default layout that hides a required node is the extension's choice at startup, before the window opens.

## What code can do

```rust
// yard_shell::dev
impl Extension {
    pub fn require(&self, cx: &mut App, component: &str);
}
```

The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| A component is declared required | A group `component-require{component}`, with `debug` `required`; an `ERROR` for a refusal |
| A `hide` or `remove` refused by the rule | `INFO` in the operation's group, `layout-op{op, node, extension}`, with the text above |
| A saved operation refused by the rule at startup | `INFO` `skipped`, with the same reason, in `layout-restore` |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_shell/shell` dev version | `require`, and the rule in the shell's rules for `hide` and `remove` |
| `yard/startup` | Dev copy that uses the dev shell. While `required-nodes` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- GPUI tests, with the dev shell:
  - hiding and removing a node of a required component are refused, with the `INFO`, and the tree is unchanged;
  - hiding and removing its parent, and its grandparent, are refused, naming the required node;
  - a sibling of the required node, which holds no required node, can be hidden and removed;
  - the required node can be moved, and its props set;
  - the `yard::Hide` action, dispatched with the required node focused, is refused (it has no default key);
  - a saved `hide` of a required node is skipped at restore with an `INFO`, and the next operations are still applied;
  - `require` before and after `register` both work;
  - `require` refused for another extension's component, an invalid ID, and a second time, each with an `ERROR`.
- **Logging**, alone in its file: `component-require{component}` with `required`; a refused `hide` logs its `INFO` inside `layout-op{op=hide}`.

## Test extension: `examples/layout_tour`

A new case, `required`. The layout is the controls, then a row with `side` (a card) and `main`, a column holding `editor`, a card of the component `layout_tour.editor`, which the case declares required, and `notes` (a card).

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-required LAYOUT_TOUR=required cargo run dev` | `side`, then `main` with `editor` and `notes`. `component-require{component=layout_tour.editor}` with `required`. |
| Click `hide editor` | Refused: an `INFO`, `editor cannot be hidden: it is required`. Nothing changes. |
| Click `remove main` | Refused: `main cannot be removed: it holds the required node editor`. |
| Click `hide notes`, then `hide side` | Both are hidden. |
| Click `move editor to root` | `editor` moves to the row; it can be moved. |

## Under review

- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- A node that cannot be moved, or a slot that cannot be emptied: when a feature needs them.
- Showing a required node that the default layout hid: the user shows it as any other.
