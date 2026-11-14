# UI kit feature 8: scroll

## What it adds

Two areas that scroll vertically, with Zed's scrollbar: one for any content, and one for a list too long to draw whole, which draws only the rows that can be seen, as Zed's pickers do with GPUI's `uniform_list`.

## How it works

As Zed's `crates/ui/src/components/scrollbar.rs` and its pickers:

- **`ScrollArea`** holds any children in a column, as tall as its content, inside a box of the height its owner gives it. The wheel and the trackpad scroll it.
- **`ScrollList`** holds `count` rows of the same height. On each frame it asks its owner to draw only the rows in view, by their range of indices. A thousand rows cost what twenty cost.
- **The scrollbar** is Zed's: a thin thumb at the right edge, Zed's regular width, in the theme's scrollbar colors. As Zed's default (`auto`), it follows macOS's setting "Show scroll bars": when the system hides scroll bars until scrolling, the thumb appears while the area scrolls or its content changes, and fades after a moment; otherwise it stays. Dragging the thumb scrolls; clicking the track jumps toward the click. There is no scrollbar when the content fits.
- **The owner holds the scroll state,** a GPUI handle (`ScrollHandle` for an area, `UniformListScrollHandle` for a list), so it keeps the position from frame to frame and can scroll by code: `UniformListScrollHandle::scroll_to_item` brings a row into view, as Zed's pickers do when the selection moves with the keys.

## What code can do

```rust
// riso_ui
pub struct ScrollArea;
impl ScrollArea {
    pub fn new(id: impl Into<ElementId>, handle: &gpui::ScrollHandle) -> Self;
}   // ParentElement, Styled (the owner gives the height)

pub struct ScrollList;
impl ScrollList {
    pub fn new(
        id: impl Into<ElementId>,
        count: usize,
        handle: &gpui::UniformListScrollHandle,
        rows: impl Fn(std::ops::Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
    ) -> Self;
}   // Styled (the owner gives the height)
```

These are riso's own types; they use GPUI's handles, and no type of Zed. Each is a `RenderOnce` element.

## Logging

| Situation | Log |
|---|---|
| A list draws its rows | `trace`: `rows drawn` with `id`, `first`, and `count`, once per frame: it follows the scrolling |
| An area or a list is drawn | Nothing else: they are drawn on every frame |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/scroll` (new, dev) | `ScrollArea` and `ScrollList`, using Zed's `WithScrollbar` and `Scrollbars` inside, with Zed's notice |

## Tests

- GPUI tests, in a window of a known size:
  - an area 100 px tall with content 500 px tall: scrolling with the wheel (`simulate_scroll_wheel`) moves the content; the handle's offset follows; the content stops at its ends;
  - an area whose content fits draws no scrollbar thumb;
  - a list of 1000 rows of 20 px in 200 px: its owner is asked for about ten rows, not a thousand (the range's length is at most the visible count plus GPUI's overdraw); after scrolling to the end, the range ends at 1000;
  - `scroll_to_item(500)` on the list's handle brings row 500 into view at the next frame;
  - dragging the thumb of the list moves the visible range.
- A logging test alone in its file: `rows drawn` at `trace` with the exact fields, once per frame.

## Test extension: `examples/ui_gallery`

A new page, `scroll`. On the left, a `ScrollArea` 300 px tall with forty paragraphs of text. On the right, a `ScrollList` 300 px tall of a thousand `ListItem`s, `Row 0` to `Row 999`, with a button `go to 500` above it that scrolls the list to row 500.

| Command and action | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, open the `scroll` tab, scroll each side | Both scroll smoothly. With macOS's "Show scroll bars: automatically", the thumb appears while scrolling and fades; with "Always", it stays. |
| Scroll the list fast to the end | `Row 999` at the bottom. With `RISO_LIFECYCLE=info,riso_ui=trace`, `rows drawn` in the trace with small counts. |
| Click `go to 500` | `Row 500` is in view. |
| Drag the list's thumb | The rows follow. |
| `{"theme": {"mode": "light"}}` | The thumb follows the theme. |

## Under review

- `riso_ui/scroll`, new

## Not in this feature

- Horizontal scrolling, and scrollbars on both axes: when a feature first needs them.
- Lists whose rows have different heights (GPUI's `list`): by need.
- Zed's scrollbar setting in riso's settings: the system's behavior is followed.
