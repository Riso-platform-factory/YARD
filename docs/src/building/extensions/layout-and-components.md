# Layout and Components

The layout engine (`yard_layout` and `yard_shell`) manages a per-window tree of nodes. Each node has a unique slug ID, a component ID (`<extension>.<slug>`), optional JSON props, and named slots that hold child nodes.

### Identifier Grammar

Node IDs, slot names, and both parts of a component ID (`<extension>.<component>`) must be valid **slugs**: an ASCII lowercase letter (`a`–`z`) followed by zero or more ASCII lowercase letters, digits (`0`–`9`), `-`, or `_`. Malformed identifiers are refused as extension errors (`ERROR`).

## Built-In Frame and Components

Before any extension runs, the shell creates the protected window frame:

| Node ID | Component | Slots | Role |
|---|---|---|---|
| `window` | `yard.window` | `title_bar`, `body`, `status_bar` | Tree root |
| `title_bar` | `yard.title_bar` | `left`, `center`, `right` | Top window bar |
| `body` | `yard.body` | `children` | Main window content |
| `status_bar` | `yard.status_bar` | `left`, `right` | Bottom bar (hidden when empty) |

Extensions cannot remove, move, hide, show, set props on, or zoom frame nodes, nor place nodes directly into `window.*` or `body.children` (use `place_root` for the body root). The body root placed via `place_root` also cannot be removed, moved, hidden, shown, or zoomed.

The shell also registers four built-in components that extensions can place:

- `yard.row` (slot: `children`): horizontal split container. Props: `shares` (`{ "<child>": <positive number> }`) and `sizes` (`{ "<child>": <positive pixels> }`).
- `yard.column` (slot: `children`): vertical split container. Same props as `yard.row`.
- `yard.tabs` (slot: `children`): tabbed container.
- `yard.toggle` (no slots): a status-bar or toolbar icon button that toggles another node's visibility. Props:
  - `node` (required): target node ID;
  - `icon` (required): name of a bundled `yard_ui::IconName` (such as `"info"`, `"plus"`, `"warning"`);
  - `tooltip` (optional): tooltip text (defaults to the target node's title or ID);
  - `alone` (optional bool, default `false`): when `true`, showing the target hides other visible siblings in the target's slot.
  A toggle that references a missing node, itself, or an ancestor draws disabled and logs a `WARN` once.

## Registering Components and Props

Register a component without props using `Extension::register`:

```rust
ext.register(cx, "my_ext.panel", &["header", "content"], |node, slots, window, cx| {
    let header_views = slots.views("header");
    cx.new(|cx| MyPanel::new(node, header_views, cx)).into()
});
```

Register a component with typed JSON props using `Extension::register_with_props::<P, V>`, where `P: serde::Serialize + serde::de::DeserializeOwned + Default + 'static` and `V: yard_shell::PropsView<P> + 'static`. The closure constructs `V` directly (`Fn(P, &NodeId, Slots, &mut Window, &mut Context<V>) -> V`), and `PropsView::set_props` handles updates on the existing view:

```rust
#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct CardProps {
    title: String,
}

impl yard_shell::PropsView<CardProps> for Card {
    fn set_props(&mut self, props: CardProps, _cx: &mut gpui::Context<Self>) {
        self.props = props;
    }
}

ext.register_with_props::<CardProps, Card>(
    cx,
    "my_ext.card",
    &[],
    |props, node, _slots, _window, _cx| Card::new(node, props),
);
```

- Props passed to `place*_with_props`, `add_with_props`, or `set_props` must serialize as a JSON object (otherwise an `ERROR` is logged and nothing changes).
- Props are read field-by-field starting from `P::default()`: valid fields are retained even if another field fails decoding. An invalid field falls back to its default with a `WARN`; an unknown key logs a `WARN` and is ignored.
- In `set_props`, passing `null` for a key removes that override so its default applies again. After an applied `set_props`, the shell calls `PropsView::set_props` and notifies the existing view instead of recreating it.
- Components that host child nodes read their children through `Slots::nodes(slot)` and `Slots::views(slot)`.

## Component Capabilities

These declarations can be called before or after `register` and must target a component in the extension's namespace:

- **Single-child slots (`slot_for_one`):** `ext.slot_for_one(cx, component, slot)` restricts a slot to at most one node (hidden nodes still occupy the slot). A second placement, add, or move is refused with `INFO`.
- **Required components (`require`):** `ext.require(cx, component)` prevents nodes of that component (and any ancestor containing one) from being hidden, snapped shut, closed, or removed.
- **Tab titles and icons (`register_title`):** Implement `yard_shell::Titled` (`fn title(&self, cx: &App) -> SharedString`, optional `fn icon(&self, cx: &App) -> Option<IconName>`) on the view and register it with `ext.register_title::<V>(cx, component)`. Tabs read the title and icon on render so changes appear automatically when the view notifies.
- **Size hints (`size_hints`):** Declare minimum and optional preferred sizes in `rem` units:
  ```rust
  ext.size_hints(
      cx,
      "my_ext.sidebar",
      yard_shell::SizeHints {
          min: gpui::size(gpui::rems(12.), gpui::rems(6.25)),
          preferred: gpui::size(Some(gpui::rems(15.)), None),
      },
  );
  ```
  Default minimum is `5rem × 6.25rem`. Non-positive (zero, negative, or non-finite) dimensions and preferred dimensions smaller than their corresponding minimum are refused with an `ERROR`. A child with a preferred size along a split's axis starts fixed at that size unless overridden by the split's `sizes` or `shares` props.
- **Empty nodes (`register_empty`):** Implement `yard_shell::Emptiable` (`fn is_empty(&self, cx: &App) -> bool`) and call `ext.register_empty::<V>(cx, component)`. Splits and tabs give empty nodes no space without recording a hide operation; containers whose children are all empty (or hidden alongside at least one empty child) also become empty. Query emptiness with `yard_shell::is_empty(cx, node)`.

## Placing and Modifying Nodes

During startup (`init`), build the default layout with placement methods:

- `ext.place_root(cx, node, component)` and `place_root_with_props(...)`: places the single root inside `body.children`. A second `place_root` call is an expected refusal (`INFO`) that leaves the first root in place.
- `ext.place(cx, node, component, parent, slot)`: appends a node to a parent's slot (including `title_bar` and `status_bar` slots, or another extension's slot). Placing a duplicate node ID is an expected refusal (`INFO`) that leaves the existing node in place.
- `ext.place_at(..., at: Position)`, `place_with_props(...)`, and `place_with(..., Placement { props, at })`: place at a specific `yard_layout::Position` (`Last`, `First`, `Index(n)`, `Before(NodeId)`, `After(NodeId)`):
  - `Before(anchor)` and `After(anchor)` require `anchor` to be in the target slot and refuse moving a node beside itself.
  - `Index(n)` past the end of the slot places the node last.

At runtime (once a window is open), mutate the layout through recorded operations (which enter the window's user layer and persist):

- `add`, `add_at`, `add_with`, `add_with_props`
- `remove`
- `move_node` (by index) and `move_to` (by `Position`)
- `set_hidden`
- `set_props`
- `set_zoomed` / `zoom_out`
- `reset_layout(window, cx)`

Calls that accept a `&mut Window` act on that window; calls without a window act on the active (or last-active) window.

## Contexts

A component can provide a shared GPUI model (`Entity<M>`) to its entire subtree, or an extension can provide one for the whole application:

- `ext.provide::<M>(cx, component, |node, cx| cx.new(|_| MyModel::new(node)))`: creates one `Entity<M>` per node of `component`, kept alive as long as the node's view lives. A component may both require and provide the same context type `M`.
- `ext.provide_window::<M>(cx, model)`: provides a fallback `Entity<M>` shared across all windows. Registering a duplicate window provider for `M` logs an `ERROR` and leaves the first provider in place.
- `ext.require_context::<M>(cx, component)`: refuses placing, adding, or moving a node of `component` anywhere without an ancestor (or window) providing `M`. When moving a container, context requirements are verified for the entire moved subtree, including all descendants.
- `yard_shell::context::<M>(cx, node) -> Option<Entity<M>>`: resolves the nearest ancestor model of type `M`, falling back to the window model.

## Item Slots and Item Tabs

An item slot repeats its template children once per item in a dynamic model without recording layout operations:

1. Implement `yard_shell::Items` (`fn items(&self) -> Vec<SharedString>`) on a model provided via context (`provide` or `provide_window`). Item keys must be unique slugs; duplicate or malformed keys are skipped with a `WARN`.
2. Declare the slot on a specific node the extension placed:
   ```rust
   ext.item_slot::<MyItems>(cx, "docs_row", "children");
   ```
3. Place template nodes in that slot. For each item key `<key>`, the shell instantiates copies with ID `<template>@<key>`. Inside a copy's view, read its key with `yard_shell::item(cx, node)`.

Item-slot rules:

- Only the template node itself is copied, without its descendants.
- Reordering keys in `Items::items` preserves existing views for stable keys.
- When the required `Items` model is missing from context, no copies are shown.
- Layout edits (`add`, `remove`, `move`, `hide`, `show`, `set_props`) and `set_zoomed` directed at copy IDs (`<template>@<key>`) are refused (`INFO`).

For a tabbed item container with per-tab close buttons and a trailing `+` new-item button, implement `yard_shell::ItemTabs` (`close_item` and `new_item`) on the model and call:

```rust
ext.item_tabs::<MyItems>(cx, "docs_tabs");
```

The node must be a `yard.tabs` container. Clicking `+` or a tab's close button only asks the model (`new_item` or `close_item`), which may refuse or notify later. When a pending New Item request (`+`) is consumed by the next change to the item list, the shell selects the first newly added key.

## Named Layouts

Register alternative layout presets as recorded operation sequences over the default layout (see [Named Layouts](../../layout/named-layouts.md)):

```rust
ext.named_layout(cx, "my_ext.focus", |layout| {
    layout.set_hidden("sidebar", true);
    layout.add("scratch", "my_ext.card", "root", "children");
});
```

Switch a window's active layout with `ext.show_layout(window, cx, "my_ext.focus")` (or `"yard.default"`). Query the current layout with `yard_shell::shown_layout(cx, window)`.

## Inspecting and Observing the Tree

- `yard_shell::inspect(cx) -> Vec<NodeInfo>`: returns the active window's tree in depth-first order, including each node's `id`, `depth`, `slot`, `component`, `extension`, `props`, `hidden` flag, and `Origin`:
  - `Origin::Default`
  - `Origin::NamedLayout(SharedString)` (carries the named layout's name)
  - `Origin::UserLayer`
  - `Origin::Copy(SharedString)` (carries the item key)
- `yard_shell::observe_layout(cx, f) -> Subscription`: fires once after any tree change (operations, resets, restores, named layout switches, or item-slot updates), not on view-state changes such as tab selection or live handle dragging. Dropping the returned `Subscription` stops callbacks.
- For visual debugging alongside `examples/layout_tour`, `examples/layout_inspector` mounts a live tree inspector panel when `LAYOUT_INSPECTOR=1` is set.

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `component-register` | `component`, `slots` | Registering a component |
| `root-place` | `node`, `component` | Placing the body root |
| `node-place` | `node`, `component`, `parent`, `slot` | Placing a node during startup |
| `layout-op` | `op`, `node`, `extension` | Applying a runtime layout operation |
| `props-read` | `node`, `component` | Parsing typed props for a view |
| `size-hints` | `component` | Declaring component size hints |
| `context-declare` | `component`, `model`, `kind` | Declaring `provide`, `provide_window`, or `require_context` |
| `component-require` | `component` | Marking a component as required |
| `slot-declare` | `component`, `slot` | Marking a slot single-child (`slot_for_one`) |
| `title-register` | `component` | Registering `Titled` |
| `empty-register` | `component` | Registering `Emptiable` |
| `item-slot-declare` | `node`, `slot`, `model` | Declaring an item slot or item tabs |
| `items-changed` | `slot`, `added`, `removed` | Reconciling changed item-slot keys |
| `item-copy` | `slot`, `key` | Instantiating a new item-slot copy |
| `item-close` | `tabs`, `key` | Asking an `ItemTabs` model to close a tab |
| `item-new` | `tabs` | Asking an `ItemTabs` model to create a new item |
