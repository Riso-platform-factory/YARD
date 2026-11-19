# Theme feature 4: ui-font

## What it adds

The section `theme` chooses the UI font: its family and its size. Text follows them, and so does everything sized in rems, and the tab bar, whose height Zed's components take from the font size. A change while the application runs applies at once, as a change of theme does.

## The section

Two values join the section `theme`. Their JSON names are Zed's:

| Value | JSON | Default |
|---|---|---|
| The UI font's family | `ui_font_family` | `"IBM Plex Sans"`, the bundled font |
| The UI font's size, in pixels | `ui_font_size` | `16` |

```json
{ "theme": { "ui_font_size": 20 } }
```

## What code can do

`ui_font` and `ui_font_size` now take the application, because their values come from the settings:

```rust
pub fn ui_font(cx: &App) -> Font;
pub fn ui_font_size(cx: &App) -> Pixels;
```

- Their values are kept in a global of `yard_ui`, which the theme setting updates.
- The settings provider that Zed's `ui` components ask reads the same global. So Zed's components, the tab bar among them, follow too.
- The shell's root view already sets each window's rem size from `ui_font_size`, and its font from `ui_font`, every time it draws. It now reads them from the global.

## Rules

Applying the font runs in a group, `ui-font-apply{family, size}`, at startup inside `app-init`, and on a change of the section beside `theme-apply`:

| Situation | What happens | Log |
|---|---|---|
| No value in the section | IBM Plex Sans at 16 | `debug`: `applied` |
| A size between 6 and 100 | That size | `debug`: `applied` |
| A size below 6 or above 100 | Clamped to 6 or 100, as Zed does | `WARN`: `ui_font_size <n> is clamped to <m>`, then `applied` |
| A family that is installed or bundled | That family | `debug`: `applied` |
| A family that is not installed | The default family; the size still applies | `WARN`: `font <family> is not installed` |
| A change while running that leaves family and size as they were | Nothing | `debug`: `unchanged` |
| A change of family or size while running | Applied; every window draws again | `debug`: `applied` |

- Whether a family is installed is asked from GPUI's text system: the names of all the fonts it knows, bundled ones included.
- The size is in pixels, a number with or without decimals.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/theming` | Dev version beside production, with its folder. `ui_font(cx)` and `ui_font_size(cx)` read the global, and so does the settings provider |
| `yard_ui/theme_setting` | Dev version beside production. The two values, their rules, and the global |
| `yard_shell/shell` | Dev version beside production. It reads `ui_font(cx)` and `ui_font_size(cx)` |
| `yard/startup` | Dev copy, with the dev shell and the dev theme setting |

`examples/theme_tour` builds against the dev versions. `examples/settings_tour` and `layout_tour` change their calls to `ui_font(cx)` and `ui_font_size(cx)` when this feature is promoted.

## Tests

- GPUI tests:
  - the default font and size;
  - a size in the file;
  - a size below 6 and above 100;
  - an installed family and one that is not installed;
  - a change while running that applies, and one that changes nothing;
  - the settings provider gives the same font and size as `ui_font` and `ui_font_size`.
- A logging test alone in its file: every row, with its level and its group.

## Test extension: `examples/theme_tour`

The card gains a tab bar at its top, `yard_ui`'s `TabBar` with two tabs, `a` and `b`, so that its height can be seen. The card's last line states the UI font and its size, read with `ui_font(cx)` and `ui_font_size(cx)`. Use a fresh home: `YARD_HOME=/tmp/yard-theme THEME_TOUR=card cargo run dev`, then edit `config/settings.json` while it runs:

| Edit | What to look for |
|---|---|
| Write `{"theme": {"ui_font_size": 24}}` | The card's text and the tab bar grow, and the line says `IBM Plex Sans 24`. `ui-font-apply{size=24}` with `applied`. |
| Write `{"theme": {"ui_font_size": 200}}` | The text at 100. The `WARN` `ui_font_size 200 is clamped to 100`. |
| Write `{"theme": {"ui_font_family": "Menlo"}}` | The card in Menlo, at 16. |
| Write `{"theme": {"ui_font_family": "Nope Sans"}}` | IBM Plex Sans, and the `WARN` `font Nope Sans is not installed`. |


## Under review

- `yard_ui/theming`, `yard_ui/theme_setting`, `yard_shell/shell`, and `yard/startup`, in their dev versions

## Not in this feature

- The buffer font and the code font: `code-font`, by need.
- Font weight, features, and fallbacks.
- Changing the size with keys, as Zed's `zoom in` and `zoom out`: the keymap engine.
