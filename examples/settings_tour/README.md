# settings_tour

A test extension for the settings engine, `riso_settings`. When `app/riso/src/dev/review.rs` lists it as `("settings_tour", settings_tour::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. It is loaded now, for `extension-sections`, which is under review, with its second extension `settings_guest`, listed as `("settings_guest", settings_tour::guest_init)`.

Without a case it does nothing, so a plain `cargo run dev` shows only what the app really does. With a case, it declares a section of its own through its handle and places one card that shows the section's values, read from the engine while the card draws.

Its section:

| Key | Value | Default |
|---|---|---|
| `settings_tour` | `greeting` | `hello` |
| | `size` | `14` |
| | `enabled` | `true` |

Use a fresh home, so that the user's real files are never touched: `RISO_HOME=/tmp/riso-settings`.

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

For example: `mkdir -p /tmp/riso-settings/config && echo '{"settings_tour": {"greeting": "hi", "size": 20}}' > /tmp/riso-settings/config/settings.json`.

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

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/riso/Cargo.toml`, and the app runs as it should without it.

## Feature 4: extension-sections

The tour declares its section with `ext.register_settings`, whose key must be the extension's name. A second extension of the tour, `settings_guest`, declares the same section type through its own handle when `SETTINGS_TOUR=guest`.

| Command | What to look for |
|---|---|
| `SETTINGS_TOUR=card cargo run dev` | The card with the three defaults, as before. `settings-register{section=settings_tour}` inside `extension-init{extension=settings_tour}`. |
| `SETTINGS_TOUR=guest cargo run dev` | The card, as with `card`. Inside `extension-init{extension=settings_guest}`, the `ERROR` `extension settings_guest cannot declare the section settings_tour`, and no second `settings-register`. |
| `SETTINGS_TOUR=twice cargo run dev` | As before: the second `settings-register{section=settings_tour}` fails with `section settings_tour is already registered`. |
