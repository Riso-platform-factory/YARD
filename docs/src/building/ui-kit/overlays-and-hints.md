# Overlays, Key Hints, and Tooltips

## Modals

`Modal::new(id)` builds a dialog surface on `elevation_3` with a default width of `30rem` (overridable via `.width(Rems)`):

- **Header (`.header(ModalHeader)`):** `ModalHeader::new()` accepts `.headline(...)`, `.description(...)`, and `.on_dismiss(|window, cx| ...)` (renders a close `IconButton` wired to the callback).
- **Sections (`.section(Section)`):** Repeatable content groups; `Section::new()` accepts an optional `.header(SectionHeader::new(label))` and children.
- **Footer (`.footer(ModalFooter)`):** Bordered footer bar with `.start_slot(...)` and `.end_slot(...)`.
- **Unpadded surface:** `ModalSurface::new()` provides a vertical `elevation_3` container with no internal padding, used for custom overlays such as the command palette.
- **Button elevation:** Pass `.layer(ElevationIndex::ModalSurface)` (or `Background`, `Surface`, `ElevatedSurface`) to buttons placed inside dialogs or elevated containers so their fills match their background.

To present a modal in a window, implement `ModalView` and call `Extension::show_modal` (see [Extensions](../extensions.md#overlays-toasts-modals-and-prompts) and [Windows](../../windows.md#overlays-modals-toasts-and-prompts)).

## Key Hints

`KeyHint` draws the keystrokes currently bound to a GPUI `Action`:

- `KeyHint::for_action(action, cx)` resolves the binding for the current focus context.
- `KeyHint::for_action_in(action, focus, cx)` resolves the binding for an explicit `FocusHandle`.
- `.style(KeyHintStyle)` selects `Default` (keycap chips) or `Label` (plain text); `.disabled(bool)` dims the hint.
- When the action has no active binding in the resolved context, `KeyHint` renders zero space.
- `yard_ui::key_hint_text(action, window, cx) -> Option<String>` returns the platform-formatted keystroke text when you need a plain string.

## Tooltips

`Tooltip` provides four constructors returning `impl Fn(&mut Window, &mut App) -> AnyView`, matching GPUI's `.tooltip(...)` and `Button::tooltip` / `IconButton::tooltip`:

- `Tooltip::text(title)`
- `Tooltip::for_action(title, action)`
- `Tooltip::for_action_in(title, action, focus)`
- `Tooltip::with_meta(title, action: Option<&dyn Action>, meta)`

Tooltips appear after GPUI's `500ms` hover delay on an elevated surface capped at `18rem` max width, and also show when hovering disabled `Button` and `IconButton` controls. Constructing a tooltip view opens `tracing::info_span!("tooltip-show", title = %title)`.

## Toasts

`Toast` and `ToastKind` (`Info`, `Success`, `Warning`, `Error`) define timed window notifications presented via `Extension::show_toast`. See [Extensions](../extensions.md#overlays-toasts-modals-and-prompts) for the API and [Windows](../../windows.md#overlays-modals-toasts-and-prompts) for display and dismissal behavior.

## Reference

### Defaults

| Component | Default |
|---|---|
| `Modal` width | `30rem` |
| `KeyHintStyle` | `Default` |
| `Tooltip` hover delay | `500ms` (GPUI default) |
| `Tooltip` max width | `18rem` |

### Lifecycle Trace Groups

| Group | Level | Fields | Emitted when |
|---|---|---|---|
| `tooltip-show` | `INFO` | `title` | A `Tooltip` builder constructs a tooltip view on hover |
