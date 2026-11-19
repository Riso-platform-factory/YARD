# Settings feature 2: user-file

## What it adds

At startup, the engine reads the user's `settings.json` once. When a section is registered, its values from the file replace its defaults. A missing file, a broken file, and a broken section each fall back to defaults and are logged, as the roadmap decided. The file is never written.

## The file

- `settings.json` in the config directory. `yard_paths` names it, through `Paths::settings_file()`.
- It is one JSON object, with one key per section:

  ```json
  { "settings_tour": { "greeting": "hi", "size": 20 } }
  ```

- A section type marks itself `#[serde(default)]`. Then a value missing from the file keeps its default, and the file can set only what it changes.

## What code can do

The functions of `sections` stay as they are, and one is added:

```rust
pub fn load(cx: &mut App, path: PathBuf);
```

- The startup calls `load` once, in `app-init`, right after `init` and before any extension runs. So every section is registered after the file is read.
- `register::<T>` publishes the section's value from the file, or its defaults.

## Rules

`load` runs in a group, `settings-load{path}`:

| Situation | What happens | Log |
|---|---|---|
| No file | Every section has its defaults | `debug`: `no settings file` |
| The file cannot be read, is not valid JSON, or is not a JSON object | Every section has its defaults. The file is left as it is | `WARN`: `the settings file cannot be read: <reason>` |
| The file is read | Its content is kept for the sections | `debug`: `loaded {sections}`, the number of top-level keys |
| `load` a second time | Panic: `yard_settings::load runs once` | — |

`register::<T>` runs in `settings-register{section}`, as before:

| Situation | What happens | Log |
|---|---|---|
| The file has no key for the section | The section has its defaults | `debug`: `registered {from=defaults}` |
| The section's value in the file is read | The section has the file's values, and its defaults where the file says nothing | `debug`: `registered {from=file}` |
| The section's value cannot be read: a value of the wrong type, or not an object | The section has its defaults | `WARN`: `section <key> cannot be read: <reason>`, then `registered {from=defaults}` |
| A key that no section declares | Left alone. Nothing is logged | — |

A broken section does not affect the other sections.

## Plain code

Reading the file and decoding a section are plain functions, without GPUI, in `yard_settings/sections`. They are tested without a window:

- reading gives the JSON object, nothing, or the reason it cannot be read;
- decoding a section from the object gives its value, nothing, or the reason it cannot be read.

`load` and `register` only call them and publish the result.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_paths/settings_file` (new, dev) | `Paths::settings_file()`: `settings.json` in the config directory |
| `yard_settings/sections` | Dev version beside production, `src/dev/sections.rs`. It adds `load`, and `register` decodes from the file |
| `yard/startup` | Dev copy. It calls the dev `init`, then `load(cx, paths.settings_file())`, in `app-init` |

The dev version of `sections` has globals of its own. The dev startup and the tour use only the dev functions.

## Tests

- Unit tests of the plain functions:
  - a missing file;
  - unreadable content, invalid JSON, and JSON that is not an object;
  - a section that is present, partial, absent, of the wrong type, and not an object;
  - an unknown key.
- GPUI tests:
  - the file's values reach `get`;
  - a broken section keeps its defaults while another section still gets its values;
  - `load` twice panics.
- A logging test alone in its file: every row of both tables, with its level and its group.
- A unit test of `Paths::settings_file()`.

## Test extension: `examples/settings_tour`

The tour's section gets `#[serde(default)]`. It uses the dev functions. The tour writes no file: the user writes it. Use a fresh home: `YARD_HOME=/tmp/yard-settings`.

| File `config/settings.json` | `SETTINGS_TOUR=card cargo run dev` shows |
|---|---|
| none | The defaults. `no settings file` in `settings-load`. |
| `{"settings_tour": {"greeting": "hi", "size": 20}}` | `greeting: hi`, `size: 20`, `enabled: true`. `registered {from=file}`. |
| `{"settings_tour": {"size": "big"}}` | The defaults, and the `WARN` `section settings_tour cannot be read`. |
| `not json` | The defaults, and the `WARN` `the settings file cannot be read`. |
| `{"other": 1, "settings_tour": {"enabled": false}}` | `enabled: false`, and nothing about `other`. |

## Under review

- `yard_paths/settings_file`
- `yard_settings/sections` and `yard/startup`, in their dev versions

## Not in this feature

- Reading the file again while the application runs: `live-reload`.
- Comments and trailing commas in the file.
- Writing the file: `write`.
