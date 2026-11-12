# Theme feature 3: system-appearance

## What it adds

The section's `mode` can be `system`, and that becomes the default, as in Zed. The theme then follows the OS: the light theme when macOS is light, the dark theme when it is dark. It follows when the OS changes while the application runs.

## The section

| Value | JSON | Default |
|---|---|---|
| Which theme is used | `mode`: `"system"`, `"dark"`, or `"light"` | `"system"` |
| The dark and the light theme | `dark`, `light` | `"One Dark"`, `"One Light"`, as before |

So a user without a section now gets One Light on a light Mac, where before the default was One Dark.

## How

- **At startup,** `system` chooses by the OS's appearance as GPUI gives it, `window_appearance`. Zed's `theme` crate keeps it in its `SystemAppearance`, which `riso_ui::init` already sets up.
- **While running,** GPUI tells a window when its appearance changes. The theme engine does not open windows, so it offers a function that the code that opens a window calls once for it:

  ```rust
  pub fn follow_appearance(window: &mut Window, cx: &mut App);
  ```

  The startup calls it for the main window, before the shell's root view is created. When told, the engine records the new appearance. If `mode` is `system`, it chooses the theme again by the rules of `theme-switch`, and every window draws again.
- The OS's appearance is seen as light or dark. GPUI's vibrant variants count as their plain ones.

## Rules

| Situation | What happens | Log |
|---|---|---|
| At startup, `mode` is `system` | The theme of the OS's appearance is applied | `theme-apply{mode=system, theme}`, `debug`: `applied` |
| The OS's appearance changes and `mode` is `system` | The other theme becomes active; every window draws again | `appearance-change{appearance}`, then inside it `theme-apply` with `applied` |
| The OS's appearance changes and `mode` is `dark` or `light` | The appearance is recorded; the theme stays | `appearance-change{appearance}`, `debug`: `mode is not system` |
| The section changes to `system` while running | The theme of the current appearance, as `theme-switch` does | `theme-apply` with `applied` or `unchanged` |
| The same appearance is reported twice | Nothing | `appearance-change`, `debug`: `unchanged` |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/theme_setting` | Dev version beside production. `system` in `Mode`, the new default, and `follow_appearance` |
| `riso/startup` | Dev copy. It calls `riso_ui::dev::follow_appearance(window, cx)` in the main window's builder, before `riso_shell::new_shell` |

## Tests

- GPUI tests. GPUI keeps a test window's `simulate_appearance_change` private, and its test windows always report light. So a test records an appearance in `SystemAppearance` first, then opens a window with `follow_appearance`: GPUI then reports the window's light appearance through the same observer an OS change goes through. The tests cover:
  - `system` at startup, with a light and with a dark recorded appearance;
  - a reported change while the mode is `system` switches the theme;
  - with `mode: dark`, a reported change does not switch;
  - a section edit to `system` picks the recorded appearance;
  - the same appearance reported twice does nothing.
- A logging test alone in its file: every row, with its level and its group.

## Test extension: `examples/theme_tour`

Use a fresh home and no file: `RISO_HOME=/tmp/riso-theme THEME_TOUR=card cargo run dev`.

| Action | What to look for |
|---|---|
| Start with macOS in light mode | `One Light`. `theme-apply{mode=system, theme=One Light}`. |
| Switch macOS to dark, in System Settings, Appearance | Within a moment, `One Dark`. `appearance-change{appearance=dark}` with `applied`. |
| Write `{"theme": {"mode": "light"}}`, then switch macOS back and forth | The card stays `One Light`. `mode is not system`. |

## Under review

- `riso_ui/theme_setting` and `riso/startup`, in their dev versions

## Not in this feature

- Choosing light and dark themes per OS appearance in a picker: the theme engine's `theme-list`.
