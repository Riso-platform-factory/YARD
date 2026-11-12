# Theme engine roadmap

## Goal

The theme engine decides how everything looks: the colors, the fonts and their sizes, and the density. A component never writes a color, a font family, or a size of text in pixels. It asks the theme by name. The user chooses the theme and the fonts in the settings, and a change applies at once, in every window.

The theme engine and the layout engine do not know each other. The layout decides what is on screen and where. The theme decides how it looks. A change of theme redraws the windows and leaves the tree as it is.

## What is built

In production, in `riso_ui/theming`, with fixed values, and the theme chosen in the settings by `riso_ui/theme_setting`:

- IBM Plex Sans is bundled, and is the UI font at 16px.
- Zed's One Dark and One Light are loaded into Zed's theme registry. One Dark is active.
- The palette gives four colors by name: `background`, `text`, `border`, and `drop_target`.
- Zed's `ui` components read the fonts and the default density through a settings provider.

## Decisions already made

- The section `theme` of the settings holds the theme, the fonts, and the density. `riso_ui` declares it.
- A theme name that is not registered keeps the active theme, and is logged as a `WARN`.
- A color is added to the palette when something draws with it. So the palette grows with the other roadmaps, not with this one.
- Theme and font files come from Zed unchanged, each with its license and a `NOTICE`.

## How this roadmap is followed

- Features are built in the order of the table, one at a time. Each one follows the workflow in `AGENTS.md` from foundation to production.
- A feature starts with a short contract in this folder, `docs/theme-engine/<slug>.md`. The user approves the contract before any code.
- One test extension, `examples/theme_tour`, grows with the features. Its card shows every color of the palette as a swatch with its name, and a line of text in each font. A change of theme, font, or density can then be seen at a glance.

## Reusing Zed

| From Zed | License | How it is reused |
|---|---|---|
| `theme`: the registry, the color tables, `SystemAppearance`, `UiDensity`, the settings provider | GPL-3.0-or-later | A dependency of `riso_ui`, as today |
| `theme_settings` | GPL-3.0-or-later | It cannot be a dependency: it needs Zed's `settings`. It is the reference for the shape of the section (`mode`, `light`, `dark`, the fonts) and for `setup_ui_font` and `load_user_theme` |
| `theme_selector` | GPL-3.0-or-later | It cannot be a dependency: it needs `picker`, which needs `workspace`. A selector is an extension, written with the UI kit |
| `assets/themes`: One, Ayu, Gruvbox. `assets/fonts/lilex` | Each family has its own license file | Copied unchanged, with the license |
| `gpui`: `window_appearance`, `observe_window_appearance`, `refresh_windows` | Apache-2.0 | Used directly |

## Features

Status is one of: planned, dev, production.

### Chosen in the settings

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 1 | `theme-setting` | The section `theme` names a dark theme and a light theme, and its `mode` says which one is used: `dark` or `light`. It is applied at startup | With `"mode": "light"` in `settings.json`, the window is One Light. A theme name that is not registered logs a `WARN`, and the default stays | `theme`'s registry | Settings engine: `user-file` | production |
| 2 | `theme-switch` | A change of the section while the application runs switches the theme, and every window redraws | Editing `settings.json` turns the running window from dark to light and back | `gpui`: `refresh_windows` | 1. Settings engine: `live-reload` | production |
| 3 | `system-appearance` | `mode` can be `system`, and that becomes the default, as in Zed. The theme then follows the light or dark appearance of the OS, also when the OS changes while the application runs | Switching macOS between light and dark switches the window | `SystemAppearance`. `gpui`: `observe_window_appearance` | 2 | dev |

### Fonts and density

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 4 | `ui-font` | The section holds the UI font's family and size. Text follows, and so does every size given in rems. A size outside sane limits is clamped, and a family that is not installed keeps the default with a `WARN` | A larger size in `settings.json` scales the card and the tab bar | `setup_ui_font`, as reference | 2 | planned |
| 5 | `density` | The section holds the density: compact, default, or comfortable. It changes the spacing of the components that come from Zed | The tab bar gets lower and higher | `UiDensity` | 2 | planned |

### More themes

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 6 | `user-themes` | Theme files in Zed's format, in the folder `themes` of the config directory, are registered at startup. A file that cannot be read is a `WARN` that names it, and the others still load. `riso_paths` names the folder | Zed's Ayu file dropped in the folder and named in `settings.json` | `riso_ui`'s loader of Zed's theme files, which reads One today. `load_user_theme`, as reference | 1 | planned |
| 7 | `theme-list` | The engine lists the registered themes, each with its name and whether it is light or dark. With the settings engine's `write`, a component can offer the choice | The tour lists the themes. A click on one writes the setting, and the window changes | `theme`'s registry | Settings engine: `write`. UI kit: `list` | planned |

### By need

| Slug | What it adds | Waits for |
|---|---|---|
| `code-font` | Lilex is bundled as the code font, with its family and size in the section, and given to Zed's `ui` components. It removes `TODO(code-font)` | The first component that shows code: the UI kit's `code-text` |

## Later

- Overriding single colors of a theme in the settings, as Zed's `theme_overrides`.
- Reading a user's theme file again when it changes.
- More bundled theme families, such as Zed's Ayu and Gruvbox.
- Syntax colors by name, with the first component that highlights code.
- Icon themes for file types.
