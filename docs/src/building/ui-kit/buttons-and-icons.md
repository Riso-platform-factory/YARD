# Buttons, Icons, and Dividers

## Buttons

`yard_ui` provides labelled `Button::new(id, label)` and compact `IconButton::new(id, icon)` controls.

Both share the same style, size, surface layer, state, and interaction builders:

- **Style (`.style(ButtonStyle)`):** `Filled`, `Tinted(TintColor)`, `Outlined`, `Subtle` (default), or `Transparent`. `TintColor` defaults to `Accent` and also offers `Error`, `Warning`, and `Success`.
- **Size (`.size(ButtonSize)`):** `Large` (`2rem`), `Medium` (`1.75rem`), `Default` (`1.375rem`), `Compact` (`1.125rem`), or `None` (`1rem`).
- **Elevation layer (`.layer(ElevationIndex)`):** `Background`, `Surface`, `ElevatedSurface`, or `ModalSurface` to match the surface behind the button.
- **States:** `.disabled(bool)`, `.toggle_state(bool)`, and `.selected_style(ButtonStyle)`.
- **Interaction:** `.on_click(|window, cx| ...)` and `.tooltip(...)` (tooltips also display when a button is disabled).

`Button` adds `.start_icon(IconName)` and `.end_icon(IconName)`. `IconButton` adds `.icon_size(IconSize)`, `.icon_color(TextColor)`, and `.shape(IconButtonShape)` (`Wide` by default, or `Square`).

Clicking a `Button` or `IconButton` wraps its callback in `tracing::debug_span!("button-click", id = %id)`.

## Icons

`Icon::new(name)` renders one of 12 bundled SVG icons using `.size(IconSize)` (default `IconSize::Medium`, `1rem`) and `.color(TextColor)` (default `TextColor::Default`).

- **`IconName` variants:** `Check`, `ChevronDown`, `ChevronRight`, `Close`, `GenericClose`, `GenericMaximize`, `GenericMinimize`, `GenericRestore`, `Info`, `Plus`, `Warning`, `XCircle`.
- **Enumeration and paths:** `IconName::ALL` lists all 12 variants; `IconName::path(self) -> &'static str` returns the bundled asset path (for example `"icons/check.svg"`).
- **`IconSize` values:** `Indicator` (`0.625rem`), `XSmall` (`0.75rem`), `Small` (`0.875rem`), `Medium` (`1rem`, default), `XLarge` (`3rem`).
- **Assets:** Pass `yard_ui::Assets` to GPUI's `Application::with_assets` at startup so bundled icons resolve.

## Dividers

`Divider` draws a thin hairline separator between sections:

- **Solid:** `Divider::horizontal()`, `Divider::vertical()`
- **Dashed:** `Divider::horizontal_dashed()`, `Divider::vertical_dashed()`
- **Modifiers:** `.inset()` pads the ends along the divider's axis; `.color(DividerColor)` picks `Border`, `BorderFaded`, or `BorderVariant` (default).

## Reference

### Defaults

| Type | Default |
|---|---|
| `ButtonStyle` | `Subtle` |
| `TintColor` | `Accent` |
| `ButtonSize` | `Default` (`1.375rem`) |
| `IconButtonShape` | `Wide` |
| `IconSize` | `Medium` (`1rem`) |
| `DividerColor` | `BorderVariant` |

### Lifecycle Trace Groups

| Group | Level | Fields | Emitted when |
|---|---|---|---|
| `button-click` | `DEBUG` | `id` | A `Button` or `IconButton` click handler runs |
