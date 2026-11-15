# Layout feature 18: contexts

## What it adds

A component can give a model to everything under its nodes, such as a project to the panels inside a project's area. A component below asks for the model by its type and receives the one of the nearest node above it that gives one. An extension can also give a model to the whole window. A component can declare that it requires a model; a node of it cannot be placed or moved to a place where no node above it, and not the window, gives that model.

This is how a Zed panel reaches its `Workspace` and its `Project` without being handed them one by one: they are above it. Here the layout says what is above.

## How it works

- **Giving.** `Extension::provide::<M>(cx, component, create)` declares that each node of a component of the extension gives a model of type `M`. `create` makes the model, an `Entity<M>`, once per node, when the node's view is created; the model lives as long as the node's view. `Extension::provide_window::<M>(cx, model)` gives a model to the whole window.
- **Asking.** `riso_shell::context::<M>(cx, node)` returns the model of type `M` of the nearest node above `node` whose component gives one, or the window's, or none. A view asks when it needs the model, for example each time it renders, so that after a move it receives the model of its new place.
- **Requiring.** `Extension::require_context::<M>(cx, component)` declares that a component needs a model of type `M` above its nodes.
- **The rule** is a rule of the shell, checked with its other rules from the declarations, not from created models:

| Change | Refused when | Log |
|---|---|---|
| `place` at startup, `add` | the component of the node, or of a node under it, requires a model that no node above the target place gives, and the window does not | `INFO`: `{node} requires a {model} model, and none is above {parent}`, where `{node}` is the requiring node (the node changed, or the first requiring node under it in tree order), `{parent}` is the parent the changed node would have, and `{model}` is the type's short name |
| `move` (also a drag, and a restored move) | the same, for the node's new place | the same `INFO` |

- **Declarations** are refused with an `ERROR`, a bug of the extension, when the component ID is invalid, is not in the extension's namespace, or the same declaration was already made: `provide` of a type the component already gives, or `require_context` of a type it already requires. A component may both give and require the same type: its nodes then need one above them, and give their own to the nodes below. They can be made before or after the component is registered. A window model given twice for one type is an `ERROR`; the first stays.

## What code can do

```rust
// riso_shell::dev
impl Extension {
    pub fn provide<M: 'static>(&self, cx: &mut App, component: &str,
        create: impl Fn(&NodeId, &mut App) -> Entity<M> + 'static);
    pub fn provide_window<M: 'static>(&self, cx: &mut App, model: Entity<M>);
    pub fn require_context<M: 'static>(&self, cx: &mut App, component: &str);
}
pub fn context<M: 'static>(cx: &App, node: &str) -> Option<Entity<M>>;
```

The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| A component gives or requires a model, or the window is given one | A group `context-declare{component, model, kind}` (`kind` is `provide`, `require`, or `window`), with `debug` `declared`; an `ERROR` for a refusal |
| A node's model is created | `debug` `context created {node, model}`, in `component-create` |
| A view asks for a model | `trace` `context {node, model, from}`, where `from` is the node that gives it, `window`, or `none`: views ask on every frame |
| A change refused by the rule | `INFO` in its group (`node-place` or `layout-op`), with the text above |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` dev version | `provide`, `provide_window`, `require_context`, `context`, models kept per node and dropped with its view, and the rule in the shell's rules for `place`, `add`, and `move` |
| `riso/startup` | Dev copy that uses the dev shell. While `contexts` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- GPUI tests, with the dev shell:
  - two nodes under one giving node receive the same model, and a change to it shows in both;
  - a node under two giving nodes of the same type receives the nearest one's;
  - a node with no giving node above receives the window's model, or none;
  - after a move under another giving node, `context` returns that node's model;
  - placing, adding, and moving a requiring node where no model is above is refused with the `INFO`, and the tree is unchanged; moving a giving node's parent that carries its requiring children along is allowed;
  - a requiring node placed where the window gives the model is allowed;
  - a saved move refused by the rule is skipped at restore with an `INFO`;
  - the model of a node is dropped when its node is removed;
  - the declarations' refusals each log an `ERROR`.
- **Logging**, alone in its file: `context-declare{component, model, kind}` with `declared`; `context created` inside `component-create`; a refused `move` logs its `INFO` inside `layout-op{op=move}`.

## Test extension: `examples/layout_tour`

A new case, `contexts`. Its component `layout_tour.counter` gives a model `Count`, a number, and shows its own number under a header with a `+1` button. Its component `layout_tour.count_card` requires a `Count` and shows the number it receives. The layout is the controls, then a row with `counter` holding `left` and `right`, two count cards, and `outside`, a plain card.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-contexts LAYOUT_TOUR=contexts cargo run dev` | `counter` with `left` and `right`, both showing `0`, and `outside`. `context-declare` for `provide` and `require`; `context created {node=counter, model=Count}`. |
| Click `+1` twice | `left` and `right` both show `2`. |
| Click `move left to root` | Refused: `left requires a Count model, and none is above root`. |
| Click `move outside into counter` | `outside` moves into the counter: it requires nothing. |
| `reset layout` | The default layout; the counter keeps `2`. |

## Under review

- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Models that are saved with the layout: a model is the extension's state, and the extension saves it if it wants.
- Giving a model to one slot of a node rather than to all of them.
