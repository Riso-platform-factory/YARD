# UI kit feature 10: tooltip

## What it adds

A short text shown when the pointer rests on an element, as Zed shows one over its buttons and tabs: a title, optionally the keys of an action at its right, and optionally a muted line of detail below. It is wrapped from Zed's `Tooltip` component, and a button or an icon button takes one.

## How it works

As Zed's `crates/ui/src/components/tooltip.rs` and GPUI's tooltips:

- **When it shows.** GPUI shows the tooltip of an element after the pointer has rested on it for 500 ms, and hides it when the pointer leaves or a mouse button is pressed. The kit keeps GPUI's delay and does not draw its own. Any element with an ID takes a tooltip through GPUI's `tooltip`; `Button` and `IconButton` take one through their own `tooltip`, which passes it to Zed's button, as Zed's `ButtonLike` does. A disabled button shows its tooltip too, as in Zed.
- **What it shows.** Zed's layout: the title, at most 18 rems wide (`max_w_72`), with the keys at its right (`justify_between`, `gap_4`) drawn by the kit's `KeyHint`; below, the detail in a small muted label, at most 18 rems wide. It sits on Zed's elevated surface (`elevation_2`: the elevated surface color, a border, rounding, a shadow), with Zed's padding (`py_1`, `px_2`) and the UI font and text color, offset from the pointer as Zed offsets it (`pl_2`, `pt_2p5`).
- **Keys.** A tooltip for an action shows the keys that would run it, as `KeyHint::for_action` gives them, or from a given focus as `KeyHint::for_action_in`. An action with no binding shows the title alone.
- **Builders.** As Zed's, a tooltip is given as a builder that GPUI calls when it shows: `Tooltip::text(title)`, `Tooltip::for_action(title, action)`, `Tooltip::for_action_in(title, action, focus)`, `Tooltip::with_meta(title, action, meta)`. Each gives `impl Fn(&mut Window, &mut App) -> AnyView`, GPUI's own type.

## What code can do

```rust
// yard_ui, new
pub struct Tooltip;   // a view
impl Tooltip {
    pub fn text(title: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView;
    pub fn for_action(title: impl Into<SharedString>, action: &dyn gpui::Action)
        -> impl Fn(&mut Window, &mut App) -> AnyView;
    pub fn for_action_in(title: impl Into<SharedString>, action: &dyn gpui::Action, focus: &gpui::FocusHandle)
        -> impl Fn(&mut Window, &mut App) -> AnyView;
    pub fn with_meta(title: impl Into<SharedString>, action: Option<&dyn gpui::Action>, meta: impl Into<SharedString>)
        -> impl Fn(&mut Window, &mut App) -> AnyView;
}

// yard_ui, Button and IconButton, added
pub fn tooltip(self, tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self;
```

These are YARD's own types and GPUI's; no type of Zed.

## Logging

| Situation | Log |
|---|---|
| A tooltip is shown | A group `tooltip-show{title}`, around the building of its view, with `tracing::debug!` `tooltip built` and fields `keys` (the keys' text, or none) and `meta` (whether it has a detail line) |
| The pointer moves over an element with a tooltip | Nothing: GPUI's hover handling runs on every move |

A tooltip is built once each time it shows, not on every frame, so the group shows in the trace each time the user sees a tooltip.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/tooltip` (new, dev) | `Tooltip` and its builders, wrapping Zed's, with Zed's notice |
| `yard_ui/button` (production, changed and stamped again) | `Button::tooltip` and `IconButton::tooltip`, each passing the builder to Zed's button. Nothing else changes. They take GPUI's type, so production code does not use the dev part |

## Tests

- GPUI tests, with test actions bound in the test (the shell depends on `yard_ui`, so its actions cannot be used):
  - a button with `Tooltip::text("Save")`: after the pointer moves onto it and the 500 ms pass, the window shows a tooltip whose text is `Save`; before the delay passes, none; after the pointer leaves, none;
  - a pointer press on the button hides its tooltip;
  - an icon button and a plain element with an ID take a tooltip the same way;
  - a disabled button shows its tooltip;
  - `Tooltip::for_action` for an action bound to a key: the tooltip is wider than the same title with `Tooltip::text`, as the keys take room at its right; for an action with no binding, the same width as `Tooltip::text`;
  - `Tooltip::with_meta`: the tooltip is taller than without the detail;
  - a long title wraps within 18 rems: its width is at most 18 rems plus the padding.
- **Logging**, alone in its file: showing a tooltip opens one `tooltip-show` group with the title in its field and `tooltip built` inside it, with the keys of a bound action; moving the pointer over the element before the delay logs nothing at `DEBUG` or above from `yard_ui`.

## Test extension: `examples/ui_gallery`

A new page, `tooltip`, with every kind of tooltip on buttons:

| Command and action | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, open the `tooltip` tab, rest the pointer on `Text` | After half a second, `Save the file` in a small raised box below the pointer. `tooltip-show{title="Save the file"}` with `tooltip built`. |
| Rest on `Keys` | `Quit the gallery` with `⌘Q` at its right, as the shell binds it, and `keys=Command-Q` in the log. |
| Rest on `Detail` | The title, and below it a muted line. |
| Rest on the icon button, and on the disabled button | Each shows its tooltip. |
| Rest on `Long` | A long title wrapped to several lines. |
| Move away, or press the button | The tooltip goes. |
| Beside Zed, resting on a button of its title bar or its status bar | The box matches in padding, rounding, border, shadow, text size, and the keys' look. |
| `{"theme": {"mode": "light"}}` | The box follows the theme. |

## Under review

- `yard_ui/tooltip`, new
- `yard_ui/button`, changed

## Not in this feature

- Tooltips that stay while the pointer moves into them (Zed's `hoverable_tooltip`), and tooltips with an element as title: when a feature first needs them.
- Tooltips on tabs: with the layout engine's `close-tab`, which first needs them.
