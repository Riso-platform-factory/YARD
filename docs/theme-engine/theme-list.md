# Theme feature 7: theme-list

## What it adds

The engine lists the registered themes, each with its name and whether it is light or dark, in the order Zed's theme selector shows them. A component can then offer the choice: choosing a theme writes the theme setting through the settings engine, as Zed's selector does, and the window changes.

## How it works

As Zed's `theme_selector` and `theme_settings::set_theme`:

- **The list.** Every theme of the registry, bundled and the user's, as entries of a name and an appearance. Dark themes come first, then light ones; each group by name.
- **Choosing.** `choose_theme(cx, name)` writes the theme section of `settings.json` with `yard_settings::write`, as Zed's `set_theme` changes Zed's setting:
  - a dark theme goes in `dark`, a light one in `light`;
  - `mode` changes to the theme's appearance, unless `mode` is `system` and the theme's appearance is the system's: then the system keeps choosing, and the new theme shows because it is the one for that appearance.
  The write reaches the window as an edit of the file does: the theme setting applies it at once.
- **A name that is not registered** writes nothing and is an `ERROR`: the caller asked for something the list never offered.
- **The test extension** offers the list on a card, with the UI kit's list. A searchable selector in a modal, as Zed's, comes with the command palette's picker.

## What code can do

```rust
// yard_ui
pub enum ThemeAppearance { Light, Dark }
pub struct ThemeEntry { pub name: SharedString, pub appearance: ThemeAppearance }

/// Every registered theme: dark first, then light, each group by name.
pub fn theme_list(cx: &App) -> Vec<ThemeEntry>;
/// Writes the theme setting so that `name` shows, as Zed's selector does.
pub fn choose_theme(cx: &mut App, name: &str);
```

These are the only new public items.

## Logging

| Situation | Log |
|---|---|
| A theme is chosen | A group `theme-choose{name}`, around the setting's write, so `settings-write` and its reload are inside it. Inside it, `debug`: `theme chosen` with `field` (`dark` or `light`) and `mode`, the mode written |
| An unknown name | Inside it, `ERROR`: `theme <name> is not registered`. Nothing is written |
| The list is read | Nothing: a component may read it on every frame |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/theme_list` (new, dev) | `ThemeAppearance`, `ThemeEntry`, `theme_list`, `choose_theme`. `ThemeSettings` of `yard_ui/theme_setting` gains `Serialize`, which `yard_settings::write` needs: a change to that production part, stamped again; nothing else in it changes |

## Tests

- GPUI tests, after the theme setting is applied, with a settings file in a temporary directory:
  - the list holds One Dark and One Light, with their appearances, dark first; with a user theme loaded, it is in its place by name;
  - with `mode: dark`, choosing `One Light` writes `light: One Light` and `mode: light`, and One Light is active;
  - with `mode: system` and the system dark, choosing a dark theme writes `dark` and keeps `mode: system`; choosing a light theme writes `light` and `mode: light`;
  - choosing an unknown name writes nothing and leaves the active theme.
- A logging test alone in its file: every row of the table, with exact fields.
- **Existing tests:** none changes.

## Test extension: `examples/settings_tour`

A new case, `SETTINGS_TOUR=themes`: the tour's card shows the theme list with the UI kit's `List`: a header `Themes`, one `ListItem` per theme, its name, and its appearance as a muted label at the end, the active theme selected. A click on an item chooses it. With the Ayu fixture in the themes folder, the list holds the five themes. Use a fresh home: `YARD_HOME=/tmp/yard-settings`.

| Command and action | What to look for |
|---|---|
| `SETTINGS_TOUR=themes cargo run dev` | The list, dark themes first: One Dark selected. |
| Click `One Light` | The window turns light at once. `settings.json` holds `"light": "One Light"` and `"mode": "light"`. `theme-choose{name=One Light}` with `settings-write` inside it. |
| Copy the Ayu fixture to `config/themes/`, start again, click `Ayu Mirage` | The Ayu themes are in the list; the window turns Ayu Mirage. |
| Set `"mode": "system"` by hand, then click a theme of the system's appearance | `mode` stays `system`. |

## Under review

- `yard_ui/theme_list`, new
- `yard_ui/theme_setting`, changed by one derive

## Not in this feature

- A selector in a modal with search, and previewing a theme while moving through the list, as Zed's: with the command palette's picker.
- Icon themes.
