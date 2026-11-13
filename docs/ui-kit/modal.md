# UI kit feature 5: modal

## What it adds

The frame of a modal, adapted from Zed's `ui::Modal`: a header with a headline, a description, and a close button; sections, each with an optional header; and a footer with buttons at its start and its end. The frame draws a modal where its owner puts it. Showing it above the window, with the focus, escape, and a click outside, is the window engine's `modal-layer`. The gallery gets a page `modal`.

## What code can do

All are riso's own types in `riso_ui`:

```rust
pub struct Modal;          // Modal::new(id).width(Rems).header(ModalHeader).section(Section).footer(ModalFooter)
pub struct ModalHeader;    // ModalHeader::new().headline(text).description(text).on_dismiss(impl Fn(&mut Window, &mut App) + 'static)
pub struct Section;        // Section::new().header(SectionHeader).child(impl IntoElement)
pub struct SectionHeader;  // SectionHeader::new(label)
pub struct ModalFooter;    // ModalFooter::new().start_slot(impl IntoElement).end_slot(impl IntoElement)
pub enum ElevationIndex { Background, Surface, ElevatedSurface, ModalSurface }   // Button::layer, IconButton::layer
```

- **The layout, the spacing, and the colors are Zed's,** as `ui::Modal` and its parts draw them, and they follow the density:
  - **the modal** is a column: its header, then its sections with 8px between them, then its footer. It draws Zed's modal surface around them: the elevated surface background, rounded large corners, a one-pixel border in the variant border color, and the modal shadow. Its width is the one given, as each Zed modal gives its own, and 30 rems when none is given;
  - **the header** puts the headline in a small muted headline, and the description below it in a muted label;
  - **a section** is a column with its header above its children, padded as Zed's uncontained section; a section header is a small label;
  - **the footer** has a top border in the variant border color, its start slot on the left, and its end slot on the right.
- **The close button** is an `IconButton` with the icon `Close`, at the right of the header. It is shown only when `on_dismiss` is given, and a click on it calls `on_dismiss`. The frame never closes itself: the owner decides what closing means, as with a button's click. Zed's close button sends `menu::Cancel` to the focused element; riso's calls the owner instead, so that a modal drawn anywhere, with or without the focus, can be closed.
- **A button knows the surface it is drawn on.** `Button` and `IconButton` gain `.layer(ElevationIndex)`, as Zed's buttons have. A filled or outlined button paints the background of its layer, as Zed's does: on `ModalSurface` the window's background color. Without it, a filled button paints the element background, which on a modal's surface hardly shows. So a modal's filled buttons take `.layer(ElevationIndex::ModalSurface)`, as every Zed modal's do. A button without a layer is drawn as before.
- **Every part is optional.** A modal with no header draws no header row, and one with no footer no footer row.
- **An id names each modal,** as in Zed. The close button's id comes from it, and each section is named by its place in the modal, so two sections whose headers have the same label do not share an element.
- **Logging:**
  - drawing a modal logs one line at `trace`, with its id, its headline, and which parts it has: description, close button, the number of sections, footer. Its sections, section headers, and footer log nothing of their own;
  - a button's draw line also names its layer, when it has one;
  - a click on the close button logs `button-click{id}` at `debug`, as every button does, with the owner's callback inside it.
- Zed's types stay inside `riso_ui`. `Modal` and `ModalHeader` are riso's compositions of riso's and Zed's pieces, because Zed's header hardcodes its close button's action. `Section`, `SectionHeader`, and `ModalFooter` wrap Zed's.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/modal` (new, dev) | `Modal`, `ModalHeader`, `Section`, `SectionHeader`, `ModalFooter` |
| `riso_ui/button` | Dev version beside production. `ElevationIndex`, and `layer` on both buttons |

The startup does not change. `review.rs` loads the gallery.

## Tests

- GPUI tests:
  - a modal with every part draws, and so does one with none;
  - the close button is there only when `on_dismiss` is given, and a click on it calls `on_dismiss` once;
  - buttons in the footer's slots are drawn, the start one left of the end one, and their clicks reach their owner;
  - with no width, the modal is 30 rems wide; with a width, that width;
  - the footer is below the sections, and the sections below the header;
  - two sections with the same header label in one modal draw, and so do two modals on one page;
  - a filled button with the layer `ModalSurface` paints the theme's background color, and one without a layer the element background.
- A logging test alone in its file: exactly one `trace` line per modal draw from the modal part, with its id, its headline, and the value of each part's field, for a full modal and for a bare one; and `button-click{id}` around the owner's line when the close button is clicked.

## Test extension: `examples/ui_gallery`

A fifth page, `modal`, after `divider`, in groups titled as the other pages are. Each modal is drawn in place, on the page, the modals of a group side by side and aligned at their tops:

- `Header`: a modal with a headline only; one with a headline and a description; one with a close button as well;
- `Sections`: a modal with a section without a header and two sections with headers, each with a line of text;
- `Footer`: a modal with buttons at the end only, `Cancel` and a filled `Confirm` on the layer `ModalSurface`, and one with a start slot as well, a muted label;
- `Dismiss`: a full modal whose close button and `Cancel` count, `dismissed 0 times`.

Use a fresh home: `RISO_HOME=/tmp/riso-gallery`.

| Command and action | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, the tab `modal` | Every modal, as Zed draws its modals. Each filled `Confirm` stands out from the surface. `gallery-build{pages=5}`. |
| The tab `button` | Every button as before. |
| Click the close button of the `Dismiss` modal, then `Cancel` | `dismissed 2 times`. Two `button-click{id=…}` groups, each with the page's own line. |
| The same, in One Light, and with `{"theme": {"ui_density": "compact"}}` | The surface follows the theme, and the spacing the density. |

## Under review

- `riso_ui/modal`
- `riso_ui/button`, in its dev version, which `modal` is the first to use

## Not in this feature

- Showing a modal above the window, the focus, escape, a click outside, and the dimmed background: the window engine's `modal-layer`.
- Scrolling a modal's sections: the UI kit's `scroll`.
- Zed's back button, the header's icon, contained sections, a section's meta text, and `ModalRow`: when a modal first needs them.
- Custom children in the header: when a modal first needs them.
