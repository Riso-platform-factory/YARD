# settings-ui: a settings editor and a schema for settings.json

In production. The sample application starts the editor with the command palette; `settings_editor` is a platform extension in `extensions/`.

An application made with YARD keeps its settings in one `settings.json`, with one section per extension (`docs/src/settings.md`). A user changes them today by editing that file, without help.

This slice adds two things:
- **A JSON schema of the declared sections,** written next to the file, so an editor such as VS Code or Zed completes and checks `settings.json`.
- **A settings editor inside the application,** which shows each section's fields as switches, choices, numbers, and text, and writes the change back to `settings.json`.

## Design

The design follows VS Code's settings editor, which generates both its UI and its schema from one description of each setting. It also draws on Zed's `settings_ui` at rev `ddb90e5`: a page per section, one control per kind of field, a "modified" mark, and reset to default.
- Zed's settings crates are GPL-3.0, so YARD uses none of them.
- Zed hand-writes every page. YARD instead derives the description from the section's type with `schemars` (MIT, already in `Cargo.lock` at 1.2.3), as VS Code derives it from `contributes.configuration`.

Electron, Tauri, and Flutter have no generated settings UI. The sources are in the proposal.

## What an extension writes

```rust
#[derive(Default, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(default)]
/// My extension
struct MySettings {
    /// Show the preview pane.
    preview: bool,
    /// How the list is sorted.
    sort: SortOrder, // an enum of unit variants
    /// Items per page.
    page_size: u32,
    /// The greeting.
    greeting: String,
}
impl yard_settings::Settings for MySettings { const KEY: &'static str = "my_extension"; }

ext.register_editable_settings::<MySettings>(cx);
```

- **`Extension::register_editable_settings::<T>(cx)`** is `register_settings` plus a description for the schema and the editor. `T` must also be `serde::Serialize` and `schemars::JsonSchema`. The section's `KEY` must equal the extension's name, as for `register_settings`.
- **A section registered with plain `register_settings` keeps working** as today. It is neither in the schema nor in the editor.
- **Titles and help come from the type.** The section's title is its doc comment, else its key. Each field's help is its doc comment, which `schemars` puts in the schema's `description`.

## The schema (`yard_settings`)

- **`yard_settings::describe::<T: Settings + Serialize + JsonSchema>(cx)`** records the section's schema, which `register_editable_settings` calls.
- **`yard_settings::json_schema(cx) -> serde_json::Value`** builds one JSON Schema (Draft 07, which VS Code's JSON server supports in full) for the whole file:
  - one property per described section;
  - keys sorted at every level, including inside default values, so the same sections give the same bytes on every run;
  - `"$schema"` allowed as a string;
  - other keys allowed, because sections that are not described must not be marked as errors.
- **`yard_settings::write_schema_file(cx, path)`** writes it with `yard_paths::write_atomic`, and skips the write when the bytes have not changed.
  - The file is `<config dir>/settings.schema.json`, beside `settings.json`.
  - `yard_app/startup` writes it once the extensions have registered, and also describes the built-in `theme` and `network` sections. A failed write logs a warning and does not stop the start.
- **A user points their editor at it** by adding `"$schema": "./settings.schema.json"` to `settings.json`. YARD ignores that key when it reads, and keeps it when it writes. YARD never adds it to the user's file itself.

## Changing a field

- **`yard_settings::write::<T>(cx, field, value)`** is unchanged, and the editor uses it.
- **`yard_settings::reset::<T>(cx, field)`** is new.
- **The editor's forms, `write_value` and `reset_value`, return a `WriteOutcome`:** `Written`, `Unchanged`, or `Refused(reason)`. The reason is a fixed category, never the value: unknown section, unknown field, type mismatch, the value does not fit the field, an unreadable `settings.json`, a failed write. The editor shows a short message for a refusal.
  - It removes the field from the section in `settings.json`, so the default applies.
  - It removes the section when it is left empty.
  - It writes atomically, and keeps every other key and the order.
  - A missing field is success, and does not write.
- **Both refuse an unregistered section, or a field the section does not have,** as `write` does today. `describe` and `reset` also refuse a type that is not the one registered under that key.
- **A `settings.json` that is not valid JSON is never overwritten** by a write or a reset. The change is refused, with one `log::warn!`; so is a section in the file that is not a JSON object. A type mismatch in the code is a quiet `info!` refusal.
- **A write that finds the value already on disk** returns `Unchanged`, and still updates the section from what it read, so the editor shows the file's state at once.
- **A change made on disk while YARD writes is not lost.** YARD reads the file, changes it, and before replacing it reads it again. If it changed meanwhile, YARD starts over from the new content, up to three times, then refuses. `write` gets the same rule. This narrows the window to the instant of the rename. It is not a lock: an editor saving at that instant can still be overwritten. The check runs through a new `yard_paths::write_atomic_if(path, bytes, check)`, which writes the temporary file and renames it only when `check` passes just before the rename. `write_atomic` itself is unchanged.
- **No log of `yard_settings` carries a value.** A value that does not decode is logged by its section, field, and a fixed reason, never by the decoder's message, which can quote the value. This applies to `write`, to reloading the file, and to the new functions.

## The editor (`extensions/settings_editor`)

- **A platform extension, as `command_palette` is.** It reaches YARD only through `yard_shell`'s handle, `yard_ui`, and `yard_settings`.
- **It opens as a modal** with the action `settings_editor::Open`, bound to cmd-, on macOS and ctrl-, elsewhere. The action also appears in the command palette.
- **A list of described sections on the left;** the selected section's fields on the right.
- **Each field shows its title** (the field name, made readable: `page_size` becomes "Page size"), its help, and a control by its schema:

  | Schema | Control | Writes |
  |---|---|---|
  | `boolean` | a switch (`yard_ui::Switch`, new) | on toggle |
  | `string` with `enum` | a dropdown (`yard_ui::Dropdown`, new) | on choice |
  | `integer` or `number`, with its `minimum` and `maximum` | a `TextInput` | on Enter or leaving it; refused when it does not parse or is out of range |
  | `string` | a `TextInput` | on Enter or leaving it |
  | anything else (objects, arrays, unions) | "Edit in settings.json", which opens the file with `ext.open_path` | — |

  More rules for numbers and schemas:
  - An integer is parsed exactly as an integer, never through a float, and checked against the schema's bounds and the bounds its `format` implies (`uint32` is 0 to 4294967295, and so on). A decimal point or an exponent is refused for an integer.
  - A `float` number must be finite and within `f32`'s range.
  - Before choosing a control, the editor resolves local `$ref`s (`#/definitions/…`, with a guard against cycles) and an `allOf` of one schema. A `$ref` fragment is percent-decoded before its JSON Pointer escapes (`~1`, `~0`) are read. The bounds on an `allOf` wrapper and on its target both apply.
  - A nullable field (`Option<T>`: a type list with `null`, or `anyOf` with a `null` branch) gets the control of `T`. Clearing a nullable text or number field resets it, so the default (`null`) applies. A nullable choice offers **Use default**, which resets it. Its name always differs from the choice's values: if a value is itself named "Use default", the reset option is named "Use default setting". A nullable text field set to an empty string can still be cleared: submitting it empty resets it.

  A refused value shows a short message under the field and writes nothing. So does a write that `write_value` refuses, for any control: the editor never shows a value that was not saved.
- **A field whose value differs from its default** shows a "modified" mark and a **Reset** button, which calls `reset`.
- **The editor follows the file.** When `settings.json` changes on disk, the shown values update.
- **"Edit in settings.json" works on a fresh configuration.** When the file does not exist, the editor creates it as `{}` with an exclusive create, never replacing a file made meanwhile, then opens it.
- **Every control has its accessibility role and name**, as in `accessibility`:
  - the switch: `Switch`, toggled;
  - the dropdown: `ComboBox`, with its value;
  - each field named by its title.

## The UI kit (`yard_ui`)

- **`yard_ui::Switch`** wraps Zed's `ui::Switch`, with its role, name, and toggled state.
- **`yard_ui::Dropdown`** uses Zed's own popup and menu, as Zed's `DropdownMenu` does, so placement, scrolling, width, hit-testing, and dismissal are Zed's:
  - its trigger is YARD's own button with a bundled chevron icon, the `ComboBox` role, its name, its value, and whether its menu is open as `expanded`, never as `toggled`, since on macOS AccessKit reports a toggled state in place of the value;
  - its menu is Zed's `PopoverMenu` with a `ContextMenu` built anew for each opening, so no dismiss handler outlives its opening. Each option is a named `toggleable_entry`, checked when chosen;
  - YARD binds Zed's menu keys (`menu::Cancel` on Escape, `menu::SelectNext` and `SelectPrevious` on Down and Up, `menu::Confirm` on Enter) in the `menu` key context, so Escape closes the menu and leaves the settings modal open;
  - focus returns where it was when the menu opened, as Zed's popup does.
  - The menu is at most 20 rem wide, and never wider than the window when it opens.
  - Known limits, both Zed's at `ddb90e5`:
    - a dismissal within the same frame the menu opens can leave focus on the closed menu;
    - in a menu longer than its height, keyboard selection does not scroll the chosen row into view. Settings choices are short lists.
    - an open menu keeps its place and width until it is reopened: it does not follow its trigger when the layout moves it, or shrink when the window does.
- **Both are production parts** (promoted with the rest of the slice).
- **The built-in sections become editable.** These are production parts, changed and stamped again:
  - `ThemeSettings` (`yard_ui/theme_setting`) derives `Serialize` and `JsonSchema`. Theme names are plain strings in this slice;
  - `NetworkSettings` (`yard_http`) derives the same.

## Logging

- **`settings-describe{section}`:** `debug!("described")`.
- **`settings-schema-write{sections}`:** `debug!("written")`, or `debug!("unchanged")`. A failed write is a `warn!` plus one `log::warn!`.
- **`settings-reset{section, field}`:** `debug!("reset")`; `info!("refused")` with the reason for an unknown section or field.
- **`settings-editor-open`:** `debug!("opened")` with the number of sections.
- **`settings-editor-change{section, field}`** around a change from the editor, around `write` or `reset`. A refused value is `info!(reason, "refused")`. Values are never logged: a setting can hold something private.

## Tests

Written first by a worker that does not write the code.

- **`yard_settings`:**
  - the schema of a described test section: properties, kinds, enum values, the doc comments as titles and descriptions, `$schema` allowed, other keys allowed;
  - `write_schema_file` writes, then reports unchanged;
  - `reset` removes a field, removes an empty section, keeps other keys and the order, refuses an unknown field, and does not write for a missing one;
  - the logging, in its own file, run 20 times.
- **The editor:**
  - the field-kind mapping from a schema;
  - the readable title;
  - number parsing with range checks;
  - with `TestAppContext`, opening the editor on a test section, toggling a switch, choosing a value, entering a number, and resetting. Each changes `settings.json` as expected;
  - the logging.
- **`yard_shell`:** `register_editable_settings` refuses a key that is not the extension's name, as `register_settings` does.
- **Test extension `examples/settings_editor_tour`** (`SETTINGS_EDITOR_TOUR=1`):
  - registers a section with one field of each kind, and a nested object;
  - its README gives each case;
  - the lead opens the editor, changes each field, checks `settings.json` and the schema file, and edits the file by hand to see the editor follow;
  - it also checks that VS Code completes `settings.json` against the schema.

## Not in this slice

- Comments and trailing commas in `settings.json` (JSONC). The file stays plain JSON, as `docs/src/settings.md` says. `jsonc-parser` (MIT) is the candidate later.
- Theme names and fonts as choices in the schema. They are known only at run time.
- The `extensions` section, which turns extensions on and off. Its keys are dynamic.
- Search across sections; editing objects and arrays in the editor; a separate settings window.
- Adding `$schema` to the user's file automatically.
