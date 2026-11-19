# Theme feature 2: theme-switch

## What it adds

A change of the section `theme` while the application runs switches the theme at once, and every window draws again. The user edits `settings.json` and sees the window turn from dark to light.

## How

- `apply_theme_setting` also observes the section `theme`, with the settings engine's `observe`. The settings engine tells it only when the section's content changed.
- When told, it chooses the theme again with the same rules as at startup, and makes it active. It then asks GPUI to draw every window again, with `refresh_windows`.
- The layout is not touched: the same views stay, with their state. They only draw again.

## Rules

Each switch runs in the same group as at startup, `theme-apply{mode, theme}`. GPUI tells observers when the update that changed the section ends, so the group comes right after the `settings-reload` that changed the section, inside `settings-watch`.

| Situation | What happens | Log |
|---|---|---|
| The chosen theme changed and is registered | It becomes active; every window draws again | `debug`: `applied` |
| The section changed, but the chosen theme is the one already active, for example the light theme's name changed while the mode is dark | Nothing is drawn again | `debug`: `unchanged` |
| The chosen name is not registered | The active theme stays | `WARN`: `theme <name> is not registered` |
| The section cannot be read | The settings engine keeps its last good value and does not tell anyone | the settings engine's `WARN` |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/theme_setting` | Dev version beside production. It observes the section, switches, and refreshes the windows. The observation lasts as long as the application |
| `yard/startup` | Dev copy, calling the dev `apply_theme_setting` |

## Tests

- GPUI tests with the settings engine's watch and the test clock:
  - an edit from dark to light switches `active_theme`;
  - a name change of the theme that is not chosen gives `unchanged`;
  - an unregistered name keeps the theme;
  - a broken edit keeps the theme.
- A logging test alone in its file: every row, with `theme-apply` inside `settings-watch`, after `settings-reload`.

## Test extension: `examples/theme_tour`

Start `THEME_TOUR=card cargo run dev` with a fresh home and no file, then edit `config/settings.json` while it runs:

| Edit | What to look for |
|---|---|
| Write `{"theme": {"mode": "light"}}` | Within a second, the card turns light and says `One Light`. `theme-apply` with `applied`, inside `settings-watch`. |
| Change it to `{"theme": {"mode": "light", "dark": "One Light"}}` | Nothing changes. `unchanged`. |
| Write `{"theme": {"mode": "light", "light": "Solarized"}}` | The card stays as it was. The `WARN` `theme Solarized is not registered`. |
| Delete the file | The card turns dark again: `One Dark`. |

## Under review

- `yard_ui/theme_setting` and `yard/startup`, in their dev versions

## Not in this feature

- Following the OS's appearance: `system-appearance`.
