# Settings feature 3: live-reload

## What it adds

While the application runs, the engine checks `settings.json` once a second. When the file changed, it is read again, and each section whose content changed gets its new value. The code that uses a section can ask to be told when the section changes. A broken edit keeps the last good values. So the user edits the file and sees the change at once, without restarting.

## Checking the file

- Every second, the engine reads the file's bytes and compares them with what it read the second before. The file is small, so this costs nothing. A file's modification time is not used: two quick writes of the same length can keep the same time on some file systems, and an editor can restore it, so a time would miss a change.
- The check is plain code without GPUI, in `riso_paths`, so that the keymap file can use it later with a timer of its own:

  ```rust
  pub struct FileCheck { .. }
  impl FileCheck {
      pub fn new(path: PathBuf) -> Self;             // nothing read yet
      pub fn check(&mut self) -> Option<FileState>;  // the state, only when it differs from the last check
  }
  pub enum FileState { Missing, Bytes(Vec<u8>), Unreadable(String) }
  ```

  The first check always gives the state. An unreadable file with the same reason as before is not a change, so a file that stays broken is reported once.

## What code can do

The functions of `user-file` stay as they are, and two are added:

```rust
pub fn watch(cx: &mut App);
pub fn observe<T: Settings>(cx: &mut App, on_change: impl FnMut(&mut App) + 'static) -> gpui::Subscription;
```

- The startup calls `watch` once, in `app-init`, after `load`. `watch` takes the file's state at once, as the state `load` read, and then checks every second. A second call does nothing, with `debug`: `already watching`. Without a `load` before it, there is no file to check: `watch` does nothing, with `debug`: `no settings file to watch`.
- `observe::<T>` calls `on_change` each time the section `T` gets a new value, until the subscription is dropped.

## Rules

`watch` runs in `settings-watch` and logs `started` at `debug`. Each check is logged at `trace`, without a group of its own, so that the trace does not get a line every second. A read runs in a group, `settings-reload{path}`, which belongs to the watch: the task that checks is spawned inside `settings-watch` with `.instrument`.

| Situation | What happens | Log |
|---|---|---|
| The file did not change | Nothing | `trace` |
| The file cannot be read, is not valid JSON, or is not an object | Every section keeps its value. The same broken content is not reported again | `WARN`: `the settings file cannot be read: <reason>` |
| The file was deleted | Every section that did not have its defaults gets them | `debug`: `changed {section}` for each |
| A section's content in the file is the same as before | Nothing; its users are not told | — |
| A section's content changed and can be read | It gets the new value, with defaults where the file says nothing; its users are told | `debug`: `changed {section}` |
| A section's content changed and cannot be read | It keeps its value. The broken content counts as seen, so it is not reported again while it stays the same | `WARN`: `section <key> cannot be read: <reason>`, once |
| A section's key was removed from the file | It gets its defaults; its users are told | `debug`: `changed {section}` |

Each read ends with `debug`: `reloaded {changed}`, the number of sections that changed.

"Content" is the section's JSON value in the file. Two files that differ only in spaces or in the order of keys have the same content.

## Plain code

Comparing two contents and deciding, for each section, whether it is unchanged, changed, removed, or broken is plain code, tested without a window:

```rust
fn parse_content(bytes: &[u8]) -> Result<Map<String, Value>, String>;
fn changed_sections(applied: &[(&'static str, Option<Value>)], content: &Map<String, Value>) -> Vec<(&'static str, Option<Value>)>;
```

- `parse_content` gives the JSON object, or the reason it is not one. `load` uses it too.
- `changed_sections` takes, for each registered key, the content that was last applied, and gives the keys whose content in the new file differs, with their new content. A removed key gives nothing as its new content. Contents are compared as JSON values.
- A section that cannot be read keeps its value, but its new content counts as seen: it is decoded again only when its content changes again, so its `WARN` is not repeated when another section changes. The GPUI part only checks the file on a timer, calls it, and publishes the result. GPUI tells the observers of a global every time it is set, even to an equal value, so a section that did not change is not set again.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_paths/file_check` (new, dev) | `FileCheck` and `FileState` |
| `riso_settings/sections` | Dev version beside production. It adds `watch` and `observe`, and reading again |
| `riso/startup` | Dev copy. It calls the dev `init`, `load`, then `watch`, in `app-init` |

## Tests

- Unit tests of the plain code: every row of the table, and contents that differ only in spaces or key order.
- GPUI tests with the test clock:
  - an edit reaches `get` and calls `observe`;
  - an unchanged section does not call `observe`;
  - a broken edit keeps the values;
  - a deleted file gives the defaults;
  - `watch` twice starts one check.
- A logging test alone in its file: every row, its level and its group, with `settings-reload` inside `settings-watch`.
- Unit tests of `FileCheck`: the first check, no change, a change of the same length, a file that appears and disappears, and an unreadable path reported once.

## Test extension: `examples/settings_tour`

The card observes its section and draws again when told. Use a fresh home: `RISO_HOME=/tmp/riso-settings`. Start with `SETTINGS_TOUR=card cargo run dev`, then, while it runs:

| Edit of `config/settings.json` | What to look for |
|---|---|
| Write `{"settings_tour": {"greeting": "hi"}}` | Within a second, `greeting: hi`. `changed {section=settings_tour}` in `settings-reload`. |
| Change it to `{"settings_tour": {"greeting": "hi"}, "other": 1}` | Nothing changes on the card, and no `changed`. `reloaded {changed=0}`. |
| Write `not json` | The card keeps `hi`. A `WARN` that the file cannot be read, once. |
| Write `{"settings_tour": {"size": "big"}}` | The card keeps `hi`. A `WARN` that the section cannot be read. |
| Delete the file | The defaults. `changed {section=settings_tour}`. |

## Under review

- `riso_paths/file_check`
- `riso_settings/sections` and `riso/startup`, in their dev versions

## Not in this feature

- Watching with the operating system's file events instead of a timer.
- The themes folder, which holds many files and will need a check of its own.
- The keymap file and the themes folder.
