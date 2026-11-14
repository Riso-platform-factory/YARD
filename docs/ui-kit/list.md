# UI kit feature 6: list

## What it adds

The base components of a list, wrapped from Zed's `ui` crate: a list, its items, its header, and its separator. An item can be selected or disabled, holds its text with a place for an icon or another element before and after it, and tells its owner when it is clicked. They look and behave as Zed's, because they are Zed's.

## How it works

As Zed's `crates/ui/src/components/list/`:

- **`List`** is a column of children with an optional header. With no child, it shows its empty message: `No items` unless another is given, in Zed's muted label.
- **`ListItem`** is one row, full width, with Zed's height for its spacing (`Dense` by default, `ExtraDense`, `Sparse`). Its children are its text, usually a `Label`. Before them, a start slot; after them, at the right edge, an end slot.
  - **Selected** (`toggle_state(true)`): Zed's selected background, `ghost_element_selected`.
  - **Disabled**: it does not react to the pointer and its click does nothing; Zed draws its children as they are given, so a disabled item's label is given the disabled color by its owner, as in Zed.
  - **Hover and press**: Zed's ghost element colors, when the item is selectable (the default).
  - **Inset**: the row is drawn with Zed's inset padding and rounded corners, as in Zed's pickers.
  - **Indent**: `indent_level` steps of Zed's default step, for nested rows.
  - **Click**: `on_click` runs when the item is clicked and is not disabled.
- **`ListHeader`** is a row with a label in Zed's header style, with an optional start slot and end slot.
- **`ListSeparator`** is Zed's thin line between groups of items.

## What code can do

```rust
// riso_ui
pub enum ListItemSpacing { Dense, ExtraDense, Sparse }

pub struct List;
impl List {
    pub fn new() -> Self;
    pub fn header(self, header: ListHeader) -> Self;
    pub fn empty_message(self, message: impl Into<SharedString>) -> Self;
}   // ParentElement

pub struct ListItem;
impl ListItem {
    pub fn new(id: impl Into<ElementId>) -> Self;
    pub fn toggle_state(self, selected: bool) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
    pub fn selectable(self, selectable: bool) -> Self;
    pub fn spacing(self, spacing: ListItemSpacing) -> Self;
    pub fn inset(self, inset: bool) -> Self;
    pub fn indent_level(self, level: usize) -> Self;
    pub fn start_slot(self, element: impl IntoElement) -> Self;
    pub fn end_slot(self, element: impl IntoElement) -> Self;
    pub fn on_click(self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self;
}   // ParentElement

pub struct ListHeader;
impl ListHeader {
    pub fn new(label: impl Into<SharedString>) -> Self;
    pub fn start_slot(self, element: impl IntoElement) -> Self;
    pub fn end_slot(self, element: impl IntoElement) -> Self;
    pub fn inset(self, inset: bool) -> Self;
}

pub struct ListSeparator;
```

These are riso's own types; no type of Zed appears in their API. Each is a `RenderOnce` element.

## Logging

| Situation | Log |
|---|---|
| An item is clicked | A group `list-item-click{id}` at `debug` around the owner's handler, as `button-click` is for a button |
| A disabled item is clicked | Nothing |
| A list, an item, a header, or a separator is drawn | Nothing: they are drawn on every frame, and the view that draws them logs its own drawing |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/list` (new, dev) | `List`, `ListItem`, `ListItemSpacing`, `ListHeader`, `ListSeparator`, wrapping Zed's, with Zed's notice |

## Tests

- GPUI tests, in a window of a known size:
  - a list of three items and a header draws the header first and the items in order, each full width, with Zed's height for `Dense`, `ExtraDense`, and `Sparse`;
  - an empty list draws its empty message, and the default `No items`;
  - clicking an item runs its handler once; clicking a disabled item runs nothing;
  - the start slot is drawn before the text and the end slot at the right edge of the row;
  - a selected item and an unselected one draw different backgrounds (checked through a debug selector on each item and the frame's painted quads, or through the colors the wrapper gives Zed: the test says which);
  - `indent_level(2)` moves the item's content right by two of Zed's steps.
- A logging test alone in its file: `list-item-click{id}` around the handler's own line, and nothing for a disabled item.

## Test extension: `examples/ui_gallery`

A new page, `list`, beside the others. It shows, top to bottom: a list with a header `Themes` and five items, the second selected and the fourth disabled (its label in the disabled color), each with an icon in its start slot and the last with a key-like label in its end slot; a separator; a list with `Sparse` items, inset, as Zed's pickers draw them; a list with nested items at indent levels 0, 1, and 2; and an empty list with its message. Clicking an enabled item selects it.

| Command and action | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, open the `list` tab | The page as described, in the dark theme. |
| Hover the items | Zed's hover color on enabled items; none on the disabled one. |
| Click an item | It becomes the selected one. `list-item-click{id=…}` in the log. Clicking the disabled item does nothing. |
| `{"theme": {"mode": "light"}}` | The page follows the theme. |
| Beside Zed's theme selector (`cmd-k cmd-t`) | The inset sparse list matches its rows in height, padding, rounding, and selected color. |

## Under review

- `riso_ui/list`, new

## Not in this feature

- Scrolling and lists too long to draw whole: `scroll`.
- Moving the selection with the keyboard, and confirming: with `command-palette` and `theme-list`, which own their selection.
- Zed's disclosure toggle on items and headers, the sub-header, and the bullet item: when a feature first needs them (the `tree-item` part).
