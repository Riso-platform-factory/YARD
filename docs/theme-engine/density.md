# Theme feature 5: density

## What it adds

The section `theme` chooses the density of Zed's components: compact, default, or comfortable. It changes their spacing, such as the height of the tab bar and the padding of a tab. A change while the application runs applies at once.

## The section

One value joins the section `theme`. Its JSON name and its values are Zed's:

| Value | JSON | Default |
|---|---|---|
| The density | `ui_density`: `"compact"`, `"default"`, or `"comfortable"` | `"default"` |

```json
{ "theme": { "ui_density": "compact" } }
```

## How

- Zed's components ask the settings provider for the density, and size their spacing by it through Zed's `DynamicSpacing`. YARD's provider returns the default density today.
- The density joins the global of `yard_ui` that holds the UI font, and the provider reads it from there.
- YARD's own code has no spacing that depends on the density. It is not changed.

## Rules

Applying the density runs in a group, `density-apply{density}`, at startup inside `app-init`, and on a change of the section beside `theme-apply` and `ui-font-apply`:

| Situation | What happens | Log |
|---|---|---|
| No value | The default density | `debug`: `applied` |
| `compact` or `comfortable` | That density | `debug`: `applied` |
| A value that is none of the three | The section cannot be read: the settings engine's `WARN`, and the section's last good values stay | the settings engine's `WARN` |
| A change while running that leaves the density as it was | Nothing | `debug`: `unchanged` |
| A change of the density while running | Applied; every window draws again | `debug`: `applied` |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/theming` | Dev version beside production. The global holds the density, and the provider returns it |
| `yard_ui/theme_setting` | Dev version beside production. The value, its group, and its rules |
| `yard_shell/shell` | Dev version beside production, with only its paths changed: the shell reads the UI font from `yard_ui`'s global, whose type the dev version of `theming` holds |
| `yard/startup` | Dev copy, with the dev functions and the dev shell |

## Tests

- GPUI tests:
  - the default density;
  - `compact` and `comfortable`;
  - a value that is none of the three keeps the last good density;
  - a change while running that applies, and one that changes nothing;
  - the provider's density equals the chosen one.
- A logging test alone in its file: every row, with its level and its group.

## Test extension: `examples/theme_tour`

The card's tab bar shows the density. Use a fresh home: `YARD_HOME=/tmp/yard-theme THEME_TOUR=card cargo run dev`, then edit `config/settings.json` while it runs:

| Edit | What to look for |
|---|---|
| Write `{"theme": {"ui_density": "compact"}}` | The tab bar gets lower. `density-apply{density=compact}` with `applied`. |
| Write `{"theme": {"ui_density": "comfortable"}}` | The tab bar gets higher than by default. |
| Write `{"theme": {"ui_density": "huge"}}` | Nothing changes. The settings engine's `WARN` that the section `theme` cannot be read. |

## Under review

- `yard_ui/theming`, `yard_ui/theme_setting`, `yard_shell/shell`, and `yard/startup`, in their dev versions

## Not in this feature

- A density for YARD's own components: each component adopts it when it is wrapped in the UI kit.
