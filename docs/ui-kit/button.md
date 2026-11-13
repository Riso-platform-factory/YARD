# UI kit feature 3: button

## What it adds

A button with a label, an icon, or both, wrapped from Zed's `ui` crate, in Zed's styles and sizes. It can be disabled or toggled, and it tells its owner when it is clicked. The gallery gets a page `button`.

## What code can do

All are riso's own types in `riso_ui`:

```rust
pub struct Button;      // Button::new(id, label)
pub struct IconButton;  // IconButton::new(id, IconName)
pub enum ButtonStyle { Filled, Tinted(TintColor), Outlined, Subtle, Transparent }  // default Subtle
pub enum TintColor { Accent, Error, Warning, Success }                             // default Accent
pub enum ButtonSize { Large, Medium, Default, Compact, None }                       // default Default
pub enum IconButtonShape { Square, Wide }                                          // default Wide
```

Both buttons take:

- `.style(ButtonStyle)` and `.size(ButtonSize)`;
- `.disabled(bool)`;
- `.toggle_state(bool)`, and `.selected_style(ButtonStyle)` for how a toggled button looks;
- `.on_click(impl Fn(&mut Window, &mut App) + 'static)`, the same form as a tab's click.

`Button` also takes `.start_icon(IconName)` and `.end_icon(IconName)`.

`IconButton` also takes `.icon_size(IconSize)`, `.icon_color(TextColor)`, and `.shape(IconButtonShape)`.

- **The styles, the sizes, and the colors of every state are Zed's.** Hovered, pressed, disabled, and toggled all come from the active theme and the density, as Zed draws them. The sizes are 32, 28, 22, 18, and 16px at 16px, in rems.
- **A disabled button is never clicked.** Its label and icon use the disabled color.
- **A toggled button** uses its selected style, and its colors are Zed's: a `Button`'s label turns accent and its icons keep their colors, and an `IconButton`'s icon takes the color of its selected style.
- **The owner keeps the state.** A button holds no state of its own: a view that owns a toggle sets `toggle_state` from its own field when it draws, changes the field in `on_click`, and asks to be drawn again.
- **An id names each button,** as in Zed. Two buttons in the same view need different ids.
- **Logging:**
  - drawing a button logs one line at `trace`, with its id, style, size, and whether it is disabled or toggled;
  - a click logs `button-click{id}` at `debug`, around the owner's callback, so whatever the owner logs while it handles the click is inside it.
- Zed's types stay inside `riso_ui`. The button maps riso's `IconName`, `IconSize`, and `TextColor` to Zed's privately, in its own part, so the production parts `text` and `icon` do not change.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/button` (new, dev) | `Button`, `IconButton`, and their enums |

The startup does not change. `review.rs` loads the gallery.

## Tests

- Unit tests: the enums' defaults, and each `ButtonSize`'s rems.
- GPUI tests:
  - every style, size, and state of both buttons draws;
  - a click on a button calls its `on_click` once;
  - a click on a disabled button calls nothing;
  - a view that toggles a button sees its own state change at each click.
- A logging test alone in its file: the draw at `trace`, and `button-click{id}` at `debug` with the owner's line inside it.

## Test extension: `examples/ui_gallery`

A third page, `button`, after `icon`, in groups titled as the other pages are:

- `Style`: a `Button` in each style, each named by its style, `Tinted` once per tint color;
- `Size`: a `Button` in each size;
- `Icon`: a button with a start icon, one with an end icon, one with both, and an `IconButton` in each shape;
- `State`: a disabled `Button` and `IconButton`, and a toggled one of each;
- `Click`: a `Button` that counts its clicks, `clicked 0 times`, and an `IconButton` that toggles itself on each click. Both log their click.

Use a fresh home: `RISO_HOME=/tmp/riso-gallery`.

| Command and action | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, the tab `button` | Every style, size, icon, and state, as in Zed. `gallery-build{pages=3}`. |
| Hover and press a button | Its hover and pressed colors. |
| Click `clicked 0 times` twice | `clicked 2 times`. Two `button-click{id=…}` groups, each with the page's own line. |
| Click the toggling `IconButton` | It turns accent, then back. |
| Click the disabled buttons | Nothing, and no `button-click`. |
| `{"theme": {"ui_density": "compact"}}` | The buttons get smaller, as Zed's do. |

## Under review

- `riso_ui/button`

## Not in this feature

- A tooltip on a button: the UI kit's `tooltip`.
- The keys that run a button's action: the UI kit's `key-hint`.
- The click's position and modifiers: when a feature first needs them.
- Outlined styles beyond `Outlined`, and Zed's split and toggle button groups.
