# Layout feature 25: tab-overflow

## What it adds

A tab bar scrolls sideways when its tabs do not fit, and selecting a tab scrolls it into view, as Zed's tab bar does. Until now, tabs that did not fit were cut off at the bar's end.

## How it works

As Zed's `TabBar` (`crates/ui/src/components/tab_bar.rs`) and `Pane::update_active_tab` (`crates/workspace/src/pane.rs`):

- **Scrolling.** The tabs sit in a row that scrolls along the bar (`overflow_x_scroll`) with a scroll handle owned by the tabs container. Tabs keep their own widths; they do not shrink to fit. The bar's own drop area at its end stays at the end of the tabs.
- **The wheel and the trackpad** scroll the row: a horizontal gesture scrolls it, and so does a vertical wheel over the bar, as GPUI's `overflow_x_scroll` and Zed's tab bar do.
- **Into view.** When a tab is selected, by a click, by the `riso::SelectNextTab` and `riso::SelectPreviousTab` actions, after a drop, or after a close (feature 24), the row scrolls just enough to show it whole, as `scroll_to_item` does. Selecting a tab already in view does not scroll.
- **What is kept.** The scroll position is view state, as the selected tab is: not recorded, not saved.

## In the UI kit

`riso_ui`'s `TabBar` gains `track_scroll(handle)`, with GPUI's `ScrollHandle`, and lays its tabs in a row that scrolls along it. A tab bar without it lays them out as today.

## What code can do

```rust
// riso_ui (production, in place)
impl TabBar {
    pub fn track_scroll(self, handle: &gpui::ScrollHandle) -> Self;
}
```

The dev shell has every item of the production shell, unchanged; the change is in its tabs container.

## Logging

| Situation | Log |
|---|---|
| A selected tab is scrolled into view | `debug` `tab scrolled into view {tabs, node}`, only when the row actually moves |
| The row is scrolled by the user | `trace` `tab bar scrolled {tabs, offset}`: scrolling runs on every frame of the gesture |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/tabs` (production, in place) | `TabBar::track_scroll` and the scrolling row. Stamped again. |
| `riso_shell/shell` dev version | The tabs container owns a scroll handle and scrolls the selected tab into view |
| `riso/startup` | Dev copy that uses the dev shell. While `tab-overflow` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **UI kit, GPUI tests:** a tab bar with `track_scroll` and more tabs than fit has a scrollable row (`max_offset` positive); a horizontal scroll moves it; a vertical wheel over it moves it; a bar without `track_scroll` lays out as today.
- **Shell, GPUI tests, with the dev shell:**
  - twenty tabs in a narrow window: selecting the last one, by click after scrolling and by `riso::SelectNextTab` from the first, scrolls it fully into view;
  - selecting a tab already in view does not scroll;
  - closing the selected last tab scrolls the newly selected one into view;
  - the scroll position is not recorded.
- **Logging**, alone in its file: `tab scrolled into view` once when the row moves, and not when it does not.

## Test extension: `examples/layout_tour`

A new case, `tab-overflow`. The layout is the controls above a tabs container with twenty cards, `t01` to `t20`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-overflow LAYOUT_TOUR=tab-overflow cargo run dev` | The bar shows the first tabs; the rest are past its end. |
| Scroll over the bar with the trackpad or the wheel | The tabs slide; `t20` comes into view. |
| Click `select t20` | The bar scrolls so that `t20` is whole at its end; `tab scrolled into view {node=t20}`. |
| Press `alt-cmd-right` (`riso::SelectNextTab`) | `t01` is selected and the bar scrolls back to the start. |
| `user layer: 0` all along | Nothing was recorded. |

## Under review

- `riso_ui/tabs`, in place
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Zed's navigation buttons at the bar's start, pinned tabs, and the two-row tab bar.
- A menu of the tabs that do not fit.
