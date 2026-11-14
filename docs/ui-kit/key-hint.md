# UI kit feature 9: key-hint

## What it adds

The keys bound to an action, drawn as Zed draws them beside a command in its command palette and its menus: on macOS with the platform's symbols (`⌘`, `⇧`, `⌥`, `⌃`) in small rounded key caps, elsewhere with `Ctrl` and `+`. It is wrapped from Zed's `KeyBinding` component.

## How it works

As Zed's `crates/ui/src/components/keybinding.rs`:

- **Which binding.** For an action, the hint shows the binding that would run it now: the one of highest precedence for the action from a given focus (`KeyHint::for_action_in`), as Zed's command palette asks from the focus that was there before the palette opened, or from the window's focus (`KeyHint::for_action`). A binding of several keystrokes (`cmd-k cmd-left`) is drawn as its keystrokes in order, with a space between them.
- **No binding.** An action with no binding draws nothing and takes no room.
- **Its look.** Zed's: each key in a key cap with the theme's colors, at Zed's default size, which follows the UI font size; `KeyHintStyle::Outlined` draws the caps with a border, as Zed's outlined style. A disabled hint is drawn in the disabled color.
- **Text.** `key_hint_text(action, window, cx)` gives the same keys as text, as Zed's `text_for_action`, for a tooltip or a log line.

## What code can do

```rust
// riso_ui
pub enum KeyHintStyle { Default, Outlined }

pub struct KeyHint;
impl KeyHint {
    pub fn for_action(action: &dyn gpui::Action, cx: &App) -> Self;
    pub fn for_action_in(action: &dyn gpui::Action, focus: &gpui::FocusHandle, cx: &App) -> Self;
    pub fn style(self, style: KeyHintStyle) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
}   // RenderOnce

pub fn key_hint_text(action: &dyn gpui::Action, window: &Window, cx: &App) -> Option<String>;
```

These are riso's own types; they use GPUI's `Action` and `FocusHandle`, and no type of Zed.

## Logging

| Situation | Log |
|---|---|
| A hint is drawn | Nothing: hints are drawn on every frame |

A hint reads the keymap and changes nothing, so it has no group and no line. What it shows can be checked in the gallery.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/key_hint` (new, dev) | `KeyHint`, `KeyHintStyle`, `key_hint_text`, wrapping Zed's, with Zed's notice |

## Tests

- GPUI tests, with the shell's default keys bound (`riso_shell::bind_default_keys`) in a test that depends on `riso_shell` as a dev-dependency of `riso_ui` only if that does not make a cycle; otherwise with test bindings of the same shapes bound in the test:
  - `key_hint_text` for an action bound to `shift-escape` gives Zed's text for it on the OS the test runs on (on macOS `⇧⎋`, as Zed's `text_for_keystroke` gives);
  - for an action bound to two keystrokes, the text holds both, in order;
  - for an action with no binding, `None`, and a hint drawn for it takes no room;
  - with two bindings for an action, one in a context deeper than the other, `for_action_in` from a focus inside that context shows the deeper one;
  - a hint for a bound action takes room, and its width grows with the number of keys.
- **Logging:** none to test, as the table says.

## Test extension: `examples/ui_gallery`

A new page, `key-hint`. It lists every action of the shell with a default key, each as a row: the action's name in a label, then its hint at the right, as Zed's command palette shows a command. A second group shows the same hints in the outlined style, and one disabled.

| Command and action | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, open the `key-hint` tab | `riso::ToggleZoom` with `⇧⎋`, `riso::SelectNextTab` with `⌘}`, `riso::FocusLeft` with `⌘K ⌘←`, `riso::Quit` with `⌘Q`, and the others. |
| Beside Zed's command palette (`cmd-shift-p`) | The key caps match in size, spacing, rounding, and colors. |
| `{"theme": {"mode": "light"}}` | The caps follow the theme. |

## Under review

- `riso_ui/key_hint`, new

## Not in this feature

- Vim-style notation and Zed's setting that hides key bindings: when a feature first needs them.
- A hint that updates when the user's keymap changes: it does already, since it reads the keymap when drawn; its test comes with `keymap-reload`.
