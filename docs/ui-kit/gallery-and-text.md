# UI kit feature 1: gallery-and-text

## What it adds

The first base components, and the gallery that shows them.

- **Text:** a label and a headline, in Zed's sizes and colors, wrapped from Zed's `ui` crate.
- **Layout helpers:** a row and a column.
- **The gallery:** a test extension, `examples/ui_gallery`, with one page per base component as tabs. Its first page is `text`.

## What code can do

All of these are YARD's own types in `yard_ui`. No type of Zed appears in them.

```rust
pub struct Label;      // Label::new(text).size(LabelSize).color(TextColor)
pub enum LabelSize { Large, Default, Small, XSmall }
pub struct Headline;   // Headline::new(text).size(HeadlineSize)
pub enum HeadlineSize { XSmall, Small, Medium, Large, XLarge }
pub enum TextColor { Default, Muted, Accent, Disabled, Placeholder, Error, Warning, Success }
pub fn h_flex() -> Div;   // a row, its children centered across it
pub fn v_flex() -> Div;   // a column
```

- `Label` and `Headline` can be children of any element, as Zed's are. Their defaults are `LabelSize::Default` and `TextColor::Default`, and `HeadlineSize::Medium`.
- **The sizes are Zed's,** in rems, so they follow the UI font's size:

  | Size | Rems | At 16px |
  |---|---|---|
  | `LabelSize::Large` | 1 | 16px |
  | `LabelSize::Default` | 0.875 | 14px |
  | `LabelSize::Small` | 0.75 | 12px |
  | `LabelSize::XSmall` | 0.625 | 10px |
  | `HeadlineSize::XSmall` to `XLarge` | 0.88, 1, 1.125, 1.27, 1.43 | |

- **The colors are named,** and come from the active theme:
  - `Default`, `Muted`, `Accent`, `Disabled`, and `Placeholder` are the theme's text colors;
  - `Error`, `Warning`, and `Success` are its status colors.
- A headline is drawn in the theme's text color, as in Zed: Zed's headline has no other color.
- `h_flex` and `v_flex` are the same plain GPUI helpers as Zed's. YARD writes them itself; nothing of Zed is needed for them.
- Labels and headlines draw as Zed's do. Drawing happens every frame, so each draw logs one line at `trace`, a label's with its size and color and a headline's with its size, and nothing above `trace`. The gallery logs what it builds.

## The gallery

`examples/ui_gallery` is a test extension:

- With `UI_GALLERY=all`, it places a `yard.tabs` container as the root. It has one tab per page, each page a component of the gallery's own. A page's tab is its node ID.
- Its first page, `text`, shows, from top to bottom:
  - a headline per `HeadlineSize`, each saying its size's name;
  - a label per `LabelSize`, each saying its size's name;
  - a label per `TextColor`, each saying its color's name;
  - a row of three labels made with `h_flex`, and a column of three made with `v_flex`.
- Each group has a title above it, as Zed's component preview draws a group's title (`component_layout.rs`): the group's name in upper case, a label at `LabelSize::XSmall` and `TextColor::Muted`, beside a hairline in the palette's `border` that fills the rest of the row.
- A page draws only with `yard_ui`: its components, the palette, and rems.
- It runs in a group, `gallery-build{pages}`, and logs each page it places at `debug`.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/text` (new, dev) | `Label`, `LabelSize`, `Headline`, `HeadlineSize`, `TextColor` |
| `yard_ui/stack` (new, dev) | `h_flex` and `v_flex` |

The startup does not change. `review.rs` loads the gallery, and the gallery builds against `yard_ui::dev`.

## Tests

- Unit tests:
  - each `LabelSize` and `HeadlineSize` gives its rems;
  - each `TextColor` gives the theme's color for it under One Dark and One Light.
- GPUI tests: a label and a headline of every size and color draw in a test window.
- A logging test alone in its file: drawing a label and a headline logs one `trace` line each, with its size and color.
- The gallery's logging: `gallery-build{pages}` with a `debug` line per page.

## Test extension: `examples/ui_gallery`

Use a fresh home: `YARD_HOME=/tmp/yard-gallery`.

| Command | What to look for |
|---|---|
| `cargo run dev` | Nothing from the gallery. |
| `UI_GALLERY=all cargo run dev` | A tab `text`. Its page shows the headlines, the labels by size and by color, a row, and a column. `gallery-build{pages=1}`. |
| The same, with `{"theme": {"mode": "light"}}` in `settings.json` | The same page in One Light: every color follows. |
| The same, with `{"theme": {"ui_font_size": 20}}` | Every text larger, in the same proportions. |

The page is checked beside Zed's component preview of `Label` and `Headline`: the sizes and colors must match.

## Under review

- `yard_ui/text` and `yard_ui/stack`

## Not in this feature

- A label's weight, italic, strikethrough, truncation, and single line: each comes when a component first needs it.
- Code text in the buffer font: the UI kit's `code-text`, by need.
