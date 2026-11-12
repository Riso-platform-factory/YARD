# Settings engine roadmap

## Goal

The settings engine is how everything that can be configured is declared, read, and changed. An engine or an extension declares a section: a key, a type, and a default for every value. The user overrides values in one file, `settings.json`. The engine gives each section its value, and tells the section's users when the value changes.

The engine does not know what a setting means. The theme engine owns the section `theme`, and an extension owns its own section.

## Decisions already made

- `settings.json` is one JSON object with one key per section. It is in the config directory, and `riso_paths` names it.
- Every value of a section has a default in code. A value in the file replaces the default. A value that is missing keeps it.
- A section that cannot be read keeps the value it had, and is logged as a `WARN`. The other sections are not affected.
- A file that is not valid JSON changes nothing. A file that is deleted gives every section its defaults. A key that nothing declared is left alone.
- The engine checks the file once a second, and reads it again when it changed. The users of a section are told only when the content of that section changed.
- Decoding and merging are plain code, tested without a window. Publishing the values and telling the users is done with GPUI.

## How this roadmap is followed

- Features are built in the order of the table, one at a time. Each one follows the workflow in `AGENTS.md` from foundation to production.
- A feature starts with a short contract in this folder, `docs/settings-engine/<slug>.md`. The user approves the contract before any code.
- One test extension, `examples/settings_tour`, grows with the features. Its card shows the values of its own section as the engine gives them, so every change of the file can be seen.
- Every run of the tour uses a fresh `RISO_HOME`, so the user's real `settings.json` is never touched.

## Reusing Zed

| From Zed | License | How it is reused |
|---|---|---|
| `gpui`: globals and observers | Apache-2.0 | Used directly |
| `settings`, `settings_content`, `settings_json`, `settings_macros`, `migrator` | GPL-3.0-or-later | Not used and not copied. They are not UI, so they have no place in `riso_ui`, and `settings` is the store of Zed's own settings. They are read as reference: `settings_store.rs` for sections, `settings_file.rs` for watching the file, `settings_json.rs` for writing a value back |

The engine is riso's own code, in a new crate, `riso_settings`. The earlier project's `crew_settings` (230 lines, with its tests) already does features 1 to 3, and is ported.

## Features

Status is one of: planned, dev, production.

### Reading

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 1 | `sections` | A crate declares a section: a key, a type, and its defaults. Any code reads the section's value. Without a file, every section has its defaults | A card shows the values of the tour's section: the defaults | `gpui` globals | none | dev |
| 2 | `user-file` | `settings.json` is read at startup, and its values replace the defaults. A broken section, a broken file, and a missing file behave as decided above | With a file, the card shows the file's values. A section with a value of the wrong type keeps its defaults, and the log says why | none | 1 | planned |
| 3 | `live-reload` | The file is checked once a second while the application runs. A changed file is read again, and the users of each changed section are told. The check is written so that the keymap file and the themes folder can use it later | Editing the file changes the card at once. A broken edit keeps the last good values | none | 2 | planned |

### Declared by extensions

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 4 | `extension-sections` | An extension declares its section through its handle. The section's key is the extension's name, so an extension cannot take the section of an engine or of another extension | The tour's section is `settings_tour`. The guest extension asks for another key and is refused with an `ERROR` | none | 2 | planned |

### Writing

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 5 | `write` | Code changes one value of a section. The engine writes the file in one step, and keeps the other sections and the keys that nothing declared. The change then arrives like any edit of the file | A button on the card changes a value. The file changes, and the card follows | none | 3. Foundation 6: atomic file write | planned |

## Later

- **Comments in the file.** Zed's `settings.json` may hold comments and trailing commas. Reading them is easy with `serde_json_lenient`, a crate Zed publishes under MIT or Apache-2.0. Keeping them when the engine writes a value is the hard part: Zed edits the text with a JSON parser, in `settings_json.rs`, 2,766 lines of GPL code that is not UI. Until that is written, the file is plain JSON.
- **A schema.** A JSON schema of every declared section, so that an editor can complete and check `settings.json`.
- **A file per project.** A second file in a project's folder that overrides the user's file. It needs a project, which is not part of the platform.
- **Renamed keys.** Keeping the user's value when a key gets another name in a newer version, as Zed's `migrator` does.
- **A settings editor.** A user interface for the settings is an extension, not part of the engine.
