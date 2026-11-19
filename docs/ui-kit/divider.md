# UI kit feature 4: divider

## What it adds

A thin line between groups, horizontal or vertical, wrapped from Zed's `ui::Divider`. The status bar and the modal are its first users. The gallery gets a page `divider`.

## What code can do

```rust
pub struct Divider;   // Divider::horizontal(), Divider::vertical(), Divider::horizontal_dashed(), Divider::vertical_dashed()
                      // .inset() .color(DividerColor)
pub enum DividerColor { Border, BorderFaded, BorderVariant }   // default BorderVariant, as Zed's
```

- These are YARD's own types in `yard_ui`, mapped privately to Zed's.
- **A divider fills its parent's width when horizontal, or its height when vertical,** with a line one pixel thick, as Zed's.
- **`inset`** keeps a small margin at both ends.
- **The colors come from the theme:**
  - `Border` is its border color;
  - `BorderFaded` is the same at 60%;
  - `BorderVariant` is its variant border color.
- **Dashed dividers** draw the same line in dashes, as Zed's.
- Drawing a divider logs one line at `trace`, with its direction, style, and color, as labels do.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/divider` (new, dev) | `Divider` and `DividerColor` |

The startup does not change. `review.rs` loads the gallery.

## Tests

- GPUI tests:
  - a horizontal divider in a column fills its width and is one pixel high;
  - a vertical one in a row fills its height and is one pixel wide;
  - `inset` leaves a margin;
  - each color gives the theme's color.
- A logging test alone in its file: one `trace` line per draw.

## Test extension: `examples/ui_gallery`

A fourth page, `divider`, after `button`, in groups titled as the other pages are:

- `Horizontal`: labels separated by a solid, a dashed, and an inset divider, in each color;
- `Vertical`: a row of labels separated by vertical dividers, solid and dashed.

| Command | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, the tab `divider` | Every divider, as in Zed. `gallery-build{pages=4}`. |
| The same, in One Light | The lines follow the theme. |

## Under review

- `yard_ui/divider`

## Not in this feature

- A divider with a label in it.
