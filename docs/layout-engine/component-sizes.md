# Layout feature 21: component-sizes

## What it adds

A component declares a minimum size, and optionally a preferred size, in rems, so that both follow the size of the UI font. A split never makes a child smaller than its minimum, and a child with a preferred size along the split starts fixed at that size, as a Zed panel starts at its default width. The minimum of a container comes from its children. This replaces the fixed minimums of the axis, 80 px across and 100 px down.

## How it works

- **Declaring.** `Extension::size_hints(cx, component, SizeHints { min, preferred })` declares the sizes of a component of the extension. `min` is a width and a height in rems; `preferred` is an optional width and an optional height in rems. It is refused with an `ERROR`, a bug of the extension, when the component ID is invalid, is not in the extension's namespace, or already has hints, or when a size is not positive or a preferred size is below the minimum. It can be called before or after the component is registered.
- **The default minimum** of a component without hints is 5 rems across and 6.25 rems down, which is the 80 px and 100 px of today at the default UI font of 16 px.
- **Rems to pixels.** Sizes are turned into pixels with the window's rem size each time the split lays out, so a larger `ui_font_size` in `settings.json` moves every limit on the next frame.
- **The minimum of a container:**
  - a split: along its axis, the sum of its shown children's minimums; across it, the largest of them;
  - a tabs container: the largest minimum of its children, plus the height of its tab bar;
  - a component's own `min` hint, if larger, wins.
  A container with no shown child has the default minimum.
- **Laying out.** Feature 20's rules hold, with each child's own minimum instead of the fixed ones: flexible children shrink to their minimums first, then fixed children shrink, never below theirs.
- **Preferred sizes.** A child of a split whose component has a preferred size along the split's axis, and which has no entry in the split's `sizes` or `shares` prop, is fixed at its preferred size. An entry in the props, from the default layout or from the user, wins. A reset gives back the preferred size.

## In the UI kit

`riso_ui`'s axis takes a minimum for each child: `AxisChild { size: AxisSize, min: Pixels }` instead of a bare `AxisSize`. It no longer has minimums of its own.

## What code can do

```rust
// riso_ui (production, in place)
pub struct AxisChild { pub size: AxisSize, pub min: Pixels }
pub fn axis(id, direction, children: Rc<RefCell<Vec<AxisChild>>>, elements) -> Axis;   // was Vec<AxisSize>

// riso_shell::dev
pub struct SizeHints { pub min: Size<Rems>, pub preferred: Size<Option<Rems>> }
impl Extension {
    pub fn size_hints(&self, cx: &mut App, component: &str, hints: SizeHints);
}
```

`on_resized` and `Resized` are as feature 20 made them. The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| A component's sizes are declared | A group `size-hints{component}`, with `debug` `declared {min, preferred}`; an `ERROR` for a refusal |
| A split lays out | `trace` `child minimums {split, mins}`, in pixels: layout runs on every frame |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/axis` (production, in place) | `AxisChild` with a minimum per child; the constants 80 and 100 go. Stamped again. |
| `riso_shell/shell` (production, in place) | Only what the changed axis signature needs: production splits pass today's minimums, 80 px across and 100 px down, per child. Stamped again. |
| `riso_shell/shell` dev version | `SizeHints`, `size_hints`, minimums of components and containers in rems, and preferred sizes |
| `riso/startup` | Dev copy that uses the dev shell. While `component-sizes` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **UI kit, GPUI tests:** a drag stops at each child's own minimum; a child with a larger minimum stops it earlier; shrinking the axis respects each minimum.
- **Shell, GPUI tests, with the dev shell:**
  - a component with `min` 20 rems across stops a drag at 20 rems, and the default minimum is 5 rems;
  - with a larger `ui_font_size`, the same minimum is more pixels;
  - a split's minimum along its axis is the sum of its children's, so a nested split stops a drag in its parent at that sum;
  - a child with a preferred width starts fixed at it; a `sizes` or `shares` entry wins; a reset gives the preferred width back;
  - the refusals of `size_hints` each log an `ERROR`.
- **Logging**, alone in its file: `size-hints{component}` with `declared`.

## Test extension: `examples/layout_tour`

A new case, `component-sizes`. Its component `layout_tour.wide` declares a minimum of 20 rems across, and `layout_tour.panel` a preferred width of 15 rems. The layout is the controls above a row with `panel` (a `layout_tour.panel`), `wide` (a `layout_tour.wide`), and `plain`, a card with no hints.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-sizes2 LAYOUT_TOUR=component-sizes cargo run dev` | `panel` 15 rems wide (240 px at the default font), `wide` and `plain` sharing the rest. `size-hints` for both components. |
| Drag the line between `wide` and `plain` to the left | It stops when `wide` is 20 rems (320 px). |
| Drag the line between `wide` and `plain` to the right | `plain` stops at 5 rems (80 px). |
| Set `"theme": {"ui_font_size": 20}` in `settings.json`, then drag again | The limits move: `wide` stops at 400 px, `panel` is 300 px. |
| `reset layout` | `panel` is back at its preferred width. |

## Under review

- `riso_ui/axis` and production `riso_shell/shell`, in place
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Hiding a child by dragging it below its minimum: feature 22.
- A maximum size.
