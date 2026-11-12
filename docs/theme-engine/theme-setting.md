# Theme feature 1: theme-setting

## What it adds

The settings choose the theme. The section `theme` names a dark theme and a light theme, and its `mode` says which one is used. It is applied once, at startup, before the first window opens. Without a section, the application looks as it does today: One Dark.

## The section

`riso_ui` declares the section `theme`:

| Value | JSON | Default |
|---|---|---|
| Which theme is used | `mode`: `"dark"` or `"light"` | `"dark"` |
| The dark theme | `dark`: a theme's name | `"One Dark"` |
| The light theme | `light`: a theme's name | `"One Light"` |

```json
{ "theme": { "mode": "light" } }
```

- The names are those of the registered themes. Today these are the bundled ones, `One Dark` and `One Light`.
- The JSON names are Zed's. Zed also accepts a plain name, `"theme": "One Dark"`. riso does not: a section is an object.
- `system` as a mode comes with the feature `system-appearance`, and becomes the default then.

## What code can do

```rust
pub fn apply_theme_setting(cx: &mut App);
pub fn active_theme(cx: &App) -> String;
```

- The startup calls `apply_theme_setting` once, after `riso_ui::init`. It registers the section and applies it.
- `active_theme` gives the name of the active theme, such as `One Dark`. It is riso's own function: no type of Zed leaves `riso_ui`.

## Rules

Applying the section runs in a group, `theme-apply{mode, theme}`, inside `app-init`, right after `ui-init`. `theme` is the chosen name.

| Situation | What happens | Log |
|---|---|---|
| No section in the file, or no file | One Dark, as today | `debug`: `applied` |
| `mode` chooses a registered theme | That theme is active | `debug`: `applied` |
| The chosen name is not registered | The active theme stays: One Dark | `WARN`: `theme <name> is not registered` |
| The section cannot be read, such as a `mode` that is neither `dark` nor `light` | The settings engine logs its `WARN` and gives the defaults: One Dark | the settings engine's `WARN`, then `debug`: `applied` |

- The palette and Zed's `ui` components follow the active theme without any other change, because both read it from the same global.
- The theme is applied before the first window opens, so nothing has to redraw.

## Startup

The order of `app-init` changes, so that the file is read before the theme is chosen:

1. `riso_log::init_app`
2. the settings store and `settings.json`: `settings-init`, `settings-load`
3. `riso_ui::init`: the fonts, the theme registry, the bundled themes, One Dark active, as today
4. the theme setting: the section is registered, and the chosen theme applied

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/theme_setting` (new, dev) | The section type, `ThemeSettings`, `apply_theme_setting(cx)`, and `active_theme(cx)`. `riso_ui` depends on `riso_settings`, which holds no code of Zed, so the GPL boundary does not move |
| `riso/startup` | Dev copy, with the new order. It calls `riso_ui::dev::apply_theme_setting(cx)` after `riso_ui::init` |

Production `riso_ui/theming` does not change.

## Tests

- GPUI tests:
  - no section: One Dark is active;
  - `mode: light`: One Light;
  - `mode: dark` with `dark: "One Light"`: One Light;
  - an unregistered name keeps One Dark;
  - a `mode` of another value keeps One Dark;
  - the palette's `background` differs between the two themes.
- A logging test alone in its file: every row, with its level and its group.

## Test extension: `examples/theme_tour`

A new test extension. With `THEME_TOUR=card`, it places one card as the root. The card's first line is the active theme's name, from `active_theme`. Then it shows, for each color of the palette, a swatch of that color with its name: `background`, `text`, `border`, `drop_target`. Under them is a line of text in the UI font, with its family and size: `IBM Plex Sans 16`. The colors and the font are read from `riso_ui` while the card draws.

Use a fresh home: `RISO_HOME=/tmp/riso-theme`.

| File `config/settings.json` | `THEME_TOUR=card cargo run dev` shows |
|---|---|
| none | One Dark: a dark card. `theme-apply{mode=dark, theme=One Dark}` with `applied`. |
| `{"theme": {"mode": "light"}}` | One Light: a light card and light text. `theme=One Light`. |
| `{"theme": {"mode": "light", "light": "Solarized"}}` | One Dark, and the `WARN` `theme Solarized is not registered`. |
| `{"theme": {"mode": "dim"}}` | One Dark, and the settings engine's `WARN` that the section `theme` cannot be read. |

## Under review

- `riso_ui/theme_setting`
- `riso/startup`, in its dev version

## Not in this feature

- Switching the theme while the application runs: `theme-switch`.
- Following the OS: `system-appearance`.
- Fonts and density in the section: `ui-font` and `density`.
- Themes other than the bundled ones: `user-themes`.
