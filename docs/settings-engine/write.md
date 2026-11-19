# Settings feature 5: write

## What it adds

Code changes one value of a section, and the engine writes it to `settings.json`. The file is written in one step, so it is never half written. Every other section, every other value, and every key that nothing declared are kept, in their order. The new value then reaches the section's users as an edit of the file does, at once.

## How it works

- **One value.** `yard_settings::write::<T>(cx, field, value)` sets `field` of the section `T` to `value`. The section must be registered. `field` must be one of the section's: a key of the section's defaults, serialized. So a section that can be written also implements `serde::Serialize`.
- **The file as it is now.** The engine reads the file again right before it writes, so an edit the user made in the last second is kept. A missing file is created, with only this section and this value. A section missing from the file is added at its end.
- **Nothing is written** when:
  - the file cannot be read or is not a JSON object: the user's file is left as it is, so a broken edit is not lost;
  - the section in the file is not a JSON object;
  - `field` is not one of the section's;
  - the section, with the new value, cannot be read as `T`: the value has the wrong type.
- **The value is the same** as the file's: nothing is written.
- **The form of the file.** The engine writes the whole file as JSON with two-space indentation and a final newline, keeping the order of every key. A file the user wrote in another form is rewritten in this one. Comments are not supported: the file is plain JSON, as the roadmap decided.
- **At once.** After the write, the engine reads the new content as the file check does, publishes the sections whose content changed, and tells their users. The next check of the file finds nothing new.
- The write uses `yard_paths::write_atomic`.

## What code can do

```rust
// yard_settings
/// Sets `field` of the section `T` to `value` in the settings file.
pub fn write<T: Settings + serde::Serialize>(cx: &mut App, field: &str, value: serde_json::Value);
```

This is the only new public item. The workspace's `serde_json` gets its `preserve_order` feature, so that the keys of the file keep their order.

## Logging

| Situation | Log |
|---|---|
| A write | A group `settings-write{section, field}` around the whole write |
| The file is written | Inside it, `debug`: `written` with `path`; then the group `settings-reload` of the new content, with its `changed` lines |
| The value is the same | Inside it, `debug`: `unchanged`. Nothing is written |
| The file cannot be read, or the section in it is not an object | Inside it, `WARN`: `the settings file is not written: <reason>` |
| `field` is not one of the section's | Inside it, `ERROR`: `section <key> has no field <field>` |
| The new value cannot be read as the section | Inside it, `ERROR`: `section <key> cannot take <field>: <reason>`. A wrong type is the code's mistake |
| The section is not registered | Inside it, `ERROR`: `section <key> is not registered` |
| The file cannot be written | Inside it, `ERROR`: `the settings file cannot be written: <reason>` |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_settings/write` (new, dev) | `write` |
| `yard_settings/sections` (production, changed and stamped again) | Three changes that `write` needs, with no change of behavior or of logging: the store keeps the file's path after `watch` starts; the part of the file check's reload that applies a new content, publishes the changed sections, and tells their users becomes a function of its own, which the reload and `write` both call; and two crate-private functions give the file's path and whether a key is registered. Every existing test of `sections` passes unchanged |

`sections` cannot have a dev version beside it, as the shell has: every crate, `yard_ui` included, reads the one store of `sections`, so a copy would hold another store that nobody reads.

## Tests

- Plain tests of the new content, without a window: a value replaced in a section; a section added at the end of the file; other sections, other values, and undeclared keys kept, in their order; the form of the output, two-space indentation and a final newline.
- GPUI tests, with a file in a temporary directory:
  - `write` changes the file, and `get` gives the new value at once, before any file check; an observer of the section is told once, and an observer of another section is not;
  - a missing file is created with only that value;
  - a file that is not valid JSON, and a section that is not an object, are left byte for byte, and the value does not change;
  - a value of the wrong type, and an unknown field, write nothing;
  - the same value writes nothing: the file's modification time does not change;
  - an edit made to the file just before the write is kept.
- A logging test alone in its file: every row of the table, with exact messages.
- **Existing tests:** none changes. If a test of another crate compares JSON text whose key order changes with `preserve_order`, each change is listed when it is made.

## Test extension: `examples/settings_tour`

A new case, `SETTINGS_TOUR=write`: the card as in `card`, with two buttons under the values. `size +1` writes `size` as the shown value plus one. `wrong type` writes `size` as the text `"big"`. Use a fresh home: `YARD_HOME=/tmp/yard-settings`.

| Command and action | What to look for |
|---|---|
| `SETTINGS_TOUR=write cargo run dev`, click `size +1` | `size: 15` at once. `config/settings.json` is created with `{"settings_tour": {"size": 15}}`. `settings-write{section=settings_tour, field=size}`, `written`, then `settings-reload` with `changed`. |
| Add `"theme": {"mode": "light"}` and a key `"x": 1` to the file by hand, then click `size +1` | `size: 16`. The file keeps `theme` and `x`, in their places. |
| Click `wrong type` | Nothing changes. The `ERROR` `section settings_tour cannot take size: …`. |
| Break the file by hand (delete a brace), then click `size +1` | The file is left as broken. The `WARN` `the settings file is not written: …`. The card keeps the last good value. |

## Under review

- `yard_settings/write`, new

## Not in this feature

- Comments and trailing commas in the file: in the roadmap's "Later".
- Writing a value deeper than one field, such as a key of a map inside a section: when a section first needs it.
- A settings editor: an extension.
