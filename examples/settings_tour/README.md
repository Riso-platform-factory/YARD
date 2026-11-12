# settings_tour

A test extension for the settings engine, `riso_settings`. When `app/riso/src/dev/review.rs` lists it as `("settings_tour", settings_tour::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app.

Without a case it does nothing, so a plain `cargo run dev` shows only what the app really does. With a case, it declares a section of its own and places one card that shows the section's values, read from the engine while the card draws.

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

## Removing it

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/riso/Cargo.toml`, and the app runs as it should without it.
