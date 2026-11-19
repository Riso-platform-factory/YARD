# settings_tour

A test extension for the settings engine, `yard_settings`. When `app/yard/src/dev/review.rs` lists it as `("settings_tour", settings_tour::init)` among the extensions it passes to the startup, and `app/yard/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. Every feature it covers is in production, so it is not loaded now; add both back to check the settings engine again, or when the next settings or theme feature is under review. Its second extension, `settings_guest`, is listed as `("settings_guest", settings_tour::guest_init)`.

Without a case it does nothing, so a plain `cargo run dev` shows only what the app really does. With a case, it declares a section of its own through its handle and places one card that shows the section's values, read from the engine while the card draws.

Its section:

| Key | Value | Default |
|---|---|---|
| `settings_tour` | `greeting` | `hello` |
| | `size` | `14` |
| | `enabled` | `true` |

Use a fresh home, so that the user's real files are never touched: `YARD_HOME=/tmp/yard-settings`.

## Feature 1: sections

| Command | What to look for |
|---|---|
| `cargo run dev` | Nothing from the tour. `settings-init` inside `app-init`, with `store created`. |
| `SETTINGS_TOUR=card cargo run dev` | One card: `settings_tour`, then `greeting: hello`, `size: 14`, `enabled: true`. In the trace, `settings-register{section=settings_tour}` inside `extension-init{extension=settings_tour}`, with `registered`. |
| `SETTINGS_TOUR=twice cargo run dev` | The second `settings-register{section=settings_tour}` fails (✗) with `section settings_tour is already registered`. The card still shows the defaults. |
| `SETTINGS_TOUR=other cargo run dev` | A `WARN` that the case is unknown, and an empty window. |

## Feature 2: user-file

The tour writes no file: write `config/settings.json` under the fresh home yourself, then start the tour.

| File `config/settings.json` | `SETTINGS_TOUR=card cargo run dev` shows |
|---|---|
| none | The defaults. `no settings file` in `settings-load{path}`, and `registered {from=defaults}`. |
| `{"settings_tour": {"greeting": "hi", "size": 20}}` | `greeting: hi`, `size: 20`, `enabled: true`. `loaded {sections=1}` and `registered {from=file}`. |
| `{"settings_tour": {"size": "big"}}` | The defaults. A `WARN` `section settings_tour cannot be read: …` inside `settings-register{section=settings_tour}`. |
| `not json` | The defaults. A `WARN` `the settings file cannot be read: …` inside `settings-load`. |
| `{"other": 1, "settings_tour": {"enabled": false}}` | `enabled: false`, and nothing about `other`. |

For example: `mkdir -p /tmp/yard-settings/config && echo '{"settings_tour": {"greeting": "hi", "size": 20}}' > /tmp/yard-settings/config/settings.json`.

## Feature 3: live-reload

The card observes its section, and draws again when the engine says it changed. Start `SETTINGS_TOUR=card cargo run dev` with no file, then edit `config/settings.json` under the fresh home while it runs:

| Edit | What to look for |
|---|---|
| Write `{"settings_tour": {"greeting": "hi"}}` | Within a second, `greeting: hi`. `changed {section=settings_tour}` in `settings-reload`, inside `settings-watch`. |
| Change it to `{"settings_tour": {"greeting": "hi"}, "other": 1}` | Nothing changes on the card. `reloaded {changed=0}`. |
| Write `not json` | The card keeps `hi`. One `WARN` that the file cannot be read. |
| Write `{"settings_tour": {"greeting": "hi", "size": "big"}}`, then change `other` in it twice | The card keeps its values. The `WARN` that the section cannot be read appears once. |
| Delete the file | The defaults. `changed {section=settings_tour}`. |

The trace gets no line for the checks themselves; they are at `trace`.

## Removing it

Remove it from the list of extensions in `app/yard/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/yard/Cargo.toml`, and the app runs as it should without it.

## Feature 4: extension-sections

The tour declares its section with `ext.register_settings`, whose key must be the extension's name. A second extension of the tour, `settings_guest`, declares the same section type through its own handle when `SETTINGS_TOUR=guest`.

| Command | What to look for |
|---|---|
| `SETTINGS_TOUR=card cargo run dev` | The card with the three defaults, as before. `settings-register{section=settings_tour}` inside `extension-init{extension=settings_tour}`. |
| `SETTINGS_TOUR=guest cargo run dev` | The card, as with `card`. Inside `extension-init{extension=settings_guest}`, the `ERROR` `extension settings_guest cannot declare the section settings_tour`, and no second `settings-register`. |
| `SETTINGS_TOUR=twice cargo run dev` | As before: the second `settings-register{section=settings_tour}` fails with `section settings_tour is already registered`. |

## Feature 5: write

`SETTINGS_TOUR=write` shows the card as in `card`, with two buttons under the values: `size +1` writes `size` as the shown value plus one, and `wrong type` writes `size` as the text `"big"`.

| Command and action | What to look for |
|---|---|
| `SETTINGS_TOUR=write cargo run dev`, click `size +1` | `size: 15` at once. `config/settings.json` is created with only `{"settings_tour": {"size": 15}}`. `settings-write{section=settings_tour, field=size}`, `written`, then `settings-reload` with `changed`. |
| Add `"theme": {"mode": "light"}` and a key `"x": 1` to the file by hand, then click `size +1` | `size: 16`. The file keeps `theme` and `x`, in their places. |
| Click `wrong type` | Nothing changes. The `ERROR` `section settings_tour cannot take size: …`. |
| Break the file by hand (delete a brace), then click `size +1` | The file is left as broken. The `WARN` `the settings file is not written: …`. The card keeps the last good value. |

## Theme feature 6: user-themes

The tour carries Zed's Ayu theme file in `fixtures/ayu.json`, with its license and `NOTICE`, for the user to copy into the themes folder.

| Command and action | What to look for |
|---|---|
| Copy `fixtures/ayu.json` to `$YARD_HOME/config/themes/`, write `{"theme": {"mode": "dark", "dark": "Ayu Dark"}}`, `SETTINGS_TOUR=card cargo run dev` | The window in Ayu Dark from the first frame. `themes-load` with `theme file loaded {file=ayu.json, themes=3}`, `user themes loaded {count=3}`. |
| Change the setting to `{"theme": {"mode": "light", "light": "Ayu Light"}}` while it runs | The window switches. |
| Put a broken copy beside it, `broken.json`, and start again | The `WARN` names `broken.json`; Ayu still loads. |
| Remove the folder and start | `no themes folder`; One Dark, with the `WARN` that the named theme is not registered. |

## Theme feature 7: theme-list

`SETTINGS_TOUR=themes` shows the registered themes as a list: a header `Themes`, one item per theme with its appearance at the end, dark themes first, the active theme selected. A click chooses the theme, as Zed's theme selector does.

| Command and action | What to look for |
|---|---|
| `SETTINGS_TOUR=themes cargo run dev` | The list, dark themes first: One Dark selected. |
| Click `One Light` | The window turns light at once, and `One Light` is selected. `settings.json` holds `"light": "One Light"` and `"mode": "light"`. `theme-choose{name=One Light}` with `settings-write` inside it. |
| Copy `fixtures/ayu.json` to `config/themes/`, start again, click `Ayu Mirage` | The Ayu themes are in the list; the window turns Ayu Mirage; the file holds `"dark": "Ayu Mirage"` and `"mode": "dark"`. |
| Set `"mode": "system"` by hand, then click a theme of the system's appearance | `mode` stays `system`. |
