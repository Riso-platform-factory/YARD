# Layout feature 29: item-tabs

## What it adds

A tabs container over an item slot becomes a tab bar of its model's items, as Zed's pane is a tab bar of its open items: each tab has a close button that asks the model to close that item, and the bar ends with a button that asks the model for a new item. The buttons only tell the model; the tabs follow the items as feature 28 makes them follow, so the model may refuse, ask first, or add an item somewhere else.

## How it works

As Zed's `Pane::render_tab_bar` and `render_tab` (`crates/workspace/src/pane.rs`), with its default settings:

- **The model.** An item model whose tabs can be closed and added implements `ItemTabs`, on top of feature 28's `Items`: `close_item(&mut self, key, window, cx)` and `new_item(&mut self, window, cx)`. Both only ask: what the model does shows when it notifies, during the call or later.
- **Declaring.** `Extension::item_tabs::<M: ItemTabs>(cx, node)` declares that the slot `children` of the node `node`, a `yard.tabs` the extension placed, is an item slot over `M` (feature 28's `item_slot::<M>(cx, node, "children")`) and that its tabs are item tabs. It is refused with an `ERROR` for the refusals of feature 28, and for a node whose component is not `yard.tabs`.
- **The close button** is the one of feature 24, on each copy's tab: shown on hover, `Close Tab` tooltip, a click or a middle click. It calls `close_item` with the copy's key, inside `item-close{tabs, key}`. Nothing is recorded.
- **The new button** is Zed's: an `IconButton` with the `Plus` icon, `IconSize::Small`, at the end of the tab bar, after the tabs and outside the scrolling row (feature 25), with the tooltip `New Item`. A click calls `new_item` inside `item-new{tabs}`.
- **Groups.** `item-close` and `item-new` last while the click is handled, including the effects GPUI runs right after it: a model that notifies during its call has its `items-changed` inside the group. A model that notifies later has its `items-changed` as a group of its own, as any notify. When the items have not changed by the end of the group, it logs `items unchanged`.
- **Selection.** After a `new_item`, the first key that appears in the next change of the items is selected, whenever that change comes, as Zed activates a newly opened item. When the selected item leaves, the tabs container selects as feature 24 does after a close. Selection is view state, as before.
- **A model that does nothing** leaves the tabs as they are: a close the model refuses keeps the tab.

## In the UI kit

`yard_ui`'s `TabBar` gains `end_child(element)`: elements after the scrolling row, at the bar's end, spaced as Zed's `TabBar` end slot. A tab bar without it lays out as today.

## What code can do

```rust
// yard_ui (production, in place)
impl TabBar {
    pub fn end_child(self, child: impl IntoElement) -> Self;
}

// yard_shell::dev
pub trait ItemTabs: Items {
    fn close_item(&mut self, key: &SharedString, window: &mut Window, cx: &mut Context<Self>)
    where
        Self: Sized;
    fn new_item(&mut self, window: &mut Window, cx: &mut Context<Self>)
    where
        Self: Sized;
}
impl Extension {
    pub fn item_tabs<M: ItemTabs>(&self, cx: &mut App, node: &str);
}
```

The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| Item tabs are declared | The group `item-slot-declare{node, slot, model}` of feature 28, with `debug` `item tabs`; an `ERROR` for a refusal |
| A close button is clicked | A group `item-close{tabs, key}`, holding what the model's notify during the call causes (`items-changed`, `view dropped`) |
| The new button is clicked | A group `item-new{tabs}`, holding what the model's notify during the call causes (`items-changed`, `item-copy`), and `debug` `tab selected {tabs, tab}` for the new item |
| The items have not changed by the end of the group | `debug` `items unchanged` in the group |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/tabs` (production, in place) | `TabBar::end_child`. Stamped again. |
| `yard_shell/shell` dev version | `ItemTabs`, `item_tabs`, the close button of copies' tabs, the new button, and the selection of a new item |
| `yard/startup` | Dev copy that uses the dev shell. While `item-tabs` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **UI kit, GPUI tests:** a tab bar with an `end_child` draws it after the tabs, outside the scrolling row (it stays in view when the row scrolls); a bar without it lays out as today.
- **Shell, GPUI tests, with the dev shell:**
  - a `yard.tabs` with item tabs over three names shows three tabs, each with a close button on hover, and a new button at the bar's end;
  - clicking a tab's close button, or middle-clicking the tab, calls `close_item` with its key once; when the model removes the key, the tab goes and its view is dropped; when it does not, the tab stays;
  - clicking the new button calls `new_item` once; the new key's tab appears and is selected;
  - closing the selected tab selects as feature 24 does;
  - nothing is recorded or saved;
  - `item_tabs` on a node that is not a `yard.tabs` is refused with an `ERROR`.
- **Logging**, alone in its file: `item-close{tabs, key}` and `item-new{tabs}` hold the groups their model's notify causes, and `items unchanged` when it changes nothing.

## Test extension: `examples/layout_tour`

A new case, `item-tabs`. As `item-slots`, with a `yard.tabs` node `names_tabs` in place of the row, declared with `item_tabs`. Its model `Names` closes an item by removing its key, except `ada`, which it keeps, and makes a new item `new-<n>` with the smallest free `n`. The card is titled by its item's key (feature 14), so its tab reads the key.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-itemtabs LAYOUT_TOUR=item-tabs cargo run dev` | Three tabs, `ada`, `bob`, `cy`, and a `+` at the bar's end. |
| Rest on `bob`'s tab, click its close button | `bob`'s tab goes; `item-close{key=bob}` holding `items-changed{removed=bob}` and `view dropped {node=name@bob}`. |
| Click `ada`'s close button | `ada` stays; `items unchanged`. |
| Click `+` twice | Tabs `new-1` and `new-2` appear, each selected when it appears. |
| `user layer: 0` all along | Nothing was recorded. |

## Under review

- `yard_ui/tabs`, in place
- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- Zed's menu behind the new button, its split button, and its navigation buttons.
- Asking before closing an item with unsaved changes: the model's, which may do it in `close_item`.
- Dragging items between item tabs.
