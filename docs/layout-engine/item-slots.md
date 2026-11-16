# Layout feature 28: item-slots

## What it adds

A slot can be an item slot: instead of showing its nodes once, it shows one copy of them for each item of a model, such as one editor per open document. Each copy receives its item as a context, so the same component shows a different item in each copy. Items come and go when the model changes, without layout operations: nothing is recorded, nothing is saved, and the copies' views are created and dropped as the items appear and leave.

This is how Zed's pane holds one item view per open buffer, while its layout knows only the pane.

## How it works

- **The model.** An item model is an `Entity<M>` where `M` implements `Items`: `items(&self) -> Vec<SharedString>`, the keys of its items in order. A key is stable for an item's life and unique in the list.
- **Declaring.** `Extension::item_slot::<M>(cx, component, slot)` declares that the slot `slot` of a component of the extension is an item slot over a model of type `M`. The model is found as feature 18's contexts are: the nearest node above, or the node itself, whose component gives an `M`, else the window's. It is refused with an `ERROR` for an invalid ID or slot name, a component outside the extension's namespace, a slot the component does not declare (checked at registration, as feature 16 does), or a second declaration.
- **Templates and copies.**
  - The nodes placed in an item slot are its templates. They stay in the tree as any node does, and operations on them work as usual: moving, hiding, or removing a template changes every copy of it.
  - For each item and each shown template, in the items' order and then the templates' order, the slot shows a copy. A copy has the ID `<template>@<key>`; it is not a node of the tree, and the layout's operations do not apply to it (one that names it is refused with an `INFO`: `{id} is a copy of an item slot`).
  - A copy's view is created with the template's component and props, as the template's would be, and `riso_shell::item(cx, copy)` returns its key. `riso_shell::context` from a copy finds the models above its item slot, as from the template.
- **When the items change.** The shell watches the model: when it notifies, the item slot compares the keys with the last ones. A new key gets copies, created inside `item-copy{slot, key}`; a key that left has its copies' views dropped, with `view dropped {node}` as feature 8; a key that stayed keeps its copies and their state, even when the order changes.
- **No model, no copies.** An item slot whose model is not found shows nothing, with a `WARN` once.
- **Containers.** An item slot can be a slot of any component, the shell's included: the copies are its children in the slot's order, so a `riso.row`'s item slot gives one column of the row per item, and a `riso.tabs`' gives one tab per item, titled by its component (feature 14).

## What code can do

```rust
// riso_shell::dev
pub trait Items: 'static {
    fn items(&self) -> Vec<SharedString>;
}
impl Extension {
    pub fn item_slot<M: Items>(&self, cx: &mut App, component: &str, slot: &str);
}
pub fn item(cx: &App, node: &str) -> Option<SharedString>;
```

The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| An item slot is declared | A group `item-slot-declare{component, slot, model}`, with `debug` `declared`; an `ERROR` for a refusal |
| The items change | A group `items-changed{slot, added, removed}`, holding an `item-copy{slot, key}` for each new key and `view dropped` for each copy dropped |
| An operation names a copy | `INFO` in its `layout-op`, with the text above |
| No model above an item slot | `WARN` once |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` dev version | `Items`, `item_slot`, `item`, copies of templates, watching the model, and refusing operations on copies |
| `riso/startup` | Dev copy that uses the dev shell. While `item-slots` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- GPUI tests, with the dev shell:
  - a `riso.row` with an item slot over three names shows three copies of its template, in the names' order, each with its key from `item`;
  - adding a name makes one copy; removing one drops its copy's view; reordering keeps the copies' views and state;
  - nothing is recorded in the user layer when items change;
  - hiding the template hides every copy; removing it removes them;
  - an operation that names a copy is refused with the `INFO`;
  - an item slot with no model shows nothing, with one `WARN`;
  - a copy's `context` finds the model above its slot;
  - the declaration's refusals each log an `ERROR`.
- **Logging**, alone in its file: `items-changed{slot, added, removed}` holds `item-copy` for each new key and `view dropped` for each left one.

## Test extension: `examples/layout_tour`

A new case, `item-slots`. Its component `layout_tour.names` gives a model `Names`, a list of names, and is a column with the controls; its child is a `riso.row` whose slot `children` is an item slot over `Names`, with one template, a card `name` that shows the name from `item`. The list starts with `ada`, `bob`, `cy`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-items LAYOUT_TOUR=item-slots cargo run dev` | Three cards in a row: `ada`, `bob`, `cy`. `item-slot-declare`. |
| Click `count` on `bob`'s card twice, then `add dee` | A fourth card `dee`; `items-changed{added=dee}` with one `item-copy{key=dee}`; `bob` still shows `count: 2`. |
| Click `remove ada` | `ada`'s card goes; `view dropped {node=name@ada}`. |
| Click `reverse` | The cards are in the reverse order; `bob` still shows `count: 2`. |
| `user layer: 0` all along | Nothing was recorded. |

## Under review

- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- A tab bar over an item slot with a close and an add button: feature 29, `item-tabs`.
- Saving the items: they are the extension's model, saved by it if it wants.
