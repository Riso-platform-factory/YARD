# Keymap feature 6: user-keymap

## What it adds

The user's keys, in `keymap.json` in the config directory, in Zed's format. The file is loaded after the default keys of the shell and of the extensions, so it wins: it adds bindings, replaces them, and removes them. An entry that cannot be read is a `WARN` that names it; the others still load.

## How it works

As Zed's keymap file, read with GPUI's own pieces (`KeyBinding::load`, `App::build_action`, `NoAction`, `Unbind`); Zed's keymap loader is GPL and not UI, so it is read as reference only:

- **The file.** A JSON array of sections. Each section has an optional `context`, a key-context predicate as GPUI reads it (`"Shell"`, `"Shell && !ModalLayer"`), and `bindings`, an object from keystrokes to an action:
  ```json
  [
    { "context": "Shell", "bindings": { "cmd-shift-z": "riso::ToggleZoom", "shift-escape": null } },
    { "bindings": { "cmd-alt-q": ["zed::Unbind", "riso::Quit"] } }
  ]
  ```
  An action is its name, `"riso::ToggleZoom"`, or a name with its input, `["name", {…}]`, as Zed writes it.
- **Adding and replacing.** A binding is added after the defaults. For the same keystrokes in the same context, the later binding wins, so the user's replaces the default.
- **Removing.** `null` as the action disables those keystrokes in that context: GPUI's `NoAction`. `["zed::Unbind", "<action>"]` removes the default binding of that action for those keystrokes, as Zed's `Unbind`.
- **What cannot be read,** each a `WARN` that names it, and the rest still loads:
  - a file that is not valid JSON, or not an array: nothing from the file is bound;
  - a section that is not an object, or whose `context` cannot be parsed: that section is skipped;
  - a binding whose keystrokes cannot be parsed, or whose action GPUI does not know or cannot build from its input: that binding is skipped.
- **No file:** nothing is bound and nothing is wrong.
- **When.** Once, at startup, after every extension's `init`, so that every default key is bound before the user's, and before the menus are set, so that the menus are built over the user's keys. Reading the file again when it changes is the next feature, `keymap-reload`.
- `riso_paths` names the file: `config/keymap.json`.

## What code can do

```rust
// riso_paths
impl Paths {
    /// The user's keymap file.
    pub fn keymap_file(&self) -> PathBuf;
}

// riso_keymap
/// Reads `path` and binds its keys after the defaults. Returns how many bindings were bound.
pub fn load_user_keymap(cx: &mut App, path: &Path) -> usize;
```

These are the only new public items. `riso_keymap` is the keymap engine's crate; if it does not exist yet, this feature creates it, with the stages of every crate, depending only on `gpui`, `riso_paths`, `serde_json`, `tracing`, and `log`.

## Logging

| Situation | Log |
|---|---|
| The startup loads the file | A group `keymap-load{path}` |
| No file | Inside it, `debug`: `no keymap file`, and nothing more |
| A binding is bound | Inside it, `debug`: `bound` with `keys`, `action`, and `context` (empty when none), as the default keys are logged |
| Something cannot be read | Inside it, `WARN`: `keymap.json: <what>: <reason>`, `<what>` naming the section by its index and the binding by its keystrokes |
| The end, with a file | Inside it, `debug`: `user keys bound` with `count` and `skipped` |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_paths/keymap_file` (new, dev) | `Paths::keymap_file` |
| `riso_keymap/user_keymap` (new, dev) | `load_user_keymap` |
| `riso/startup` | Dev version beside production: `riso_keymap::load_user_keymap(cx, &paths.keymap_file())` after `extensions-load`, before `set_menus` |

## Tests

- A plain test: `keymap_file` is `config/keymap.json` under the home.
- GPUI tests, with test actions and a file in a temporary directory:
  - a file that binds `cmd-shift-z` to an action: dispatching that keystroke in its context runs the action; the default key still runs it too;
  - a file that binds the default keystrokes of one action to another: the keystroke runs the user's action;
  - `null` for a default keystroke: the keystroke runs nothing in that context;
  - `["zed::Unbind", "<action>"]`: the default binding of that action for those keys is gone; `bindings_for_action` no longer lists it;
  - an action with input, `["name", {…}]`, is built with its input;
  - each kind of bad entry is skipped and the others load: invalid keys, an unknown action, a bad context, a section that is not an object; a file that is not JSON binds nothing; no file binds nothing and returns 0.
- A logging test alone in its file: every row of the table, with exact fields and messages.
- **Existing tests:** none changes.

## Test extension: `examples/keys_tour`

The tour's existing cases show the default keys; with a `keymap.json` in the fresh home, they show the user's. Use `RISO_HOME=/tmp/riso-keys`.

| Command and action | What to look for |
|---|---|
| Write `[{"context": "Shell", "bindings": {"cmd-shift-z": "riso::ToggleZoom", "shift-escape": null}}]` to `config/keymap.json`; `LAYOUT_TOUR=zoom cargo run dev`; click card `b`; press `cmd-shift-z` | `b` zooms. `keymap-load` with two `bound` lines and `user keys bound {count=2, skipped=0}`. |
| Press `shift-escape` | Nothing: the default key is removed. |
| Add an entry with an unknown action and one with keys `cmd-+-x` | Each a `WARN` naming it; the good entries still work. |

## Under review

- `riso_paths/keymap_file`, new
- `riso_keymap/user_keymap`, new
- `riso/startup`, in its dev version

## Not in this feature

- Reading the file again when it changes: `keymap-reload`.
- Zed's other keymap options (`use_key_equivalents`, `unbind` sections, base keymaps such as VS Code's): when a user first needs them.
- Comments and trailing commas in the file: plain JSON, as the settings file.
