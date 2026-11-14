# Keymap feature 7: keymap-reload

## What it adds

`keymap.json` is checked once a second while the application runs, with the check the settings engine uses. When it changes, the user's keys are bound again from the new content, at once: a key added works, a key removed stops working, and the default keys are as they were.

## How it works

- **The check.** `riso_paths`' `FileCheck`, once a second, as the settings file is checked. A file created, changed, or deleted is read again; a check that finds nothing new does nothing.
- **Bound again.** GPUI cannot remove some bindings and keep others, so the keymap is rebuilt: every binding is cleared (`App::clear_key_bindings`), the default bindings are bound again as they were before the user's file was first read, and then the user's file is bound as `user-keymap` binds it. The default bindings are the ones bound when the file was first read at startup: the shell's, the text input's, and every extension's from its `init`.
- **A broken edit** follows the rules of `user-keymap`: a file that is not valid JSON binds none of the user's keys, so the defaults alone are bound; a bad entry is skipped with its `WARN` and the others are bound.
- **A deleted file:** the defaults alone.
- **What does not follow.** The menus keep the keys they showed when they were set; they show the new keys at the next start. A key bound by code after startup, outside an extension's `init`, is not among the defaults and is lost at the first reload: nothing in riso does that today.

## What code can do

```rust
// riso_keymap
/// Checks `path` once a second and binds the user's keys again when it changes.
pub fn watch_user_keymap(cx: &mut App, path: PathBuf);
```

This is the only new public item. The startup calls it after `load_user_keymap`.

## Logging

| Situation | Log |
|---|---|
| The watching starts | A group `keymap-watch{path}` that lasts while the application runs, as `settings-watch` does; `debug`: `started` |
| The file changed | Inside it, a group `keymap-reload{path}`, with `debug`: `defaults bound` with `count`, then the lines of `user-keymap` for the file (`bound`, each `WARN`, `user keys bound`, or `no keymap file`) |
| A check finds nothing new | Nothing |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_keymap/user_keymap` (production) | Nothing, unless keeping the default bindings needs it to record them when it first loads: then it records them in a global of its own, and that change is stamped again |
| `riso_keymap/keymap_reload` (new, dev) | `watch_user_keymap` |
| `riso/startup` | Dev version beside production: `riso_keymap::watch_user_keymap(cx, paths.keymap_file())` after `load_user_keymap` |

## Tests

- GPUI tests, with test actions, default bindings, and a file in a temporary directory, the check driven by GPUI's test clock:
  - after the file adds a binding and a second passes, the new keystroke runs its action;
  - after the file removes it, the keystroke runs nothing, and the default keys still work;
  - an edit to invalid JSON: the defaults alone work; a later good edit binds the user's keys again;
  - deleting the file: the defaults alone;
  - no change: nothing is bound again (the bindings count does not change, and no `keymap-reload` group opens).
- A logging test alone in its file: every row of the table.
- **Existing tests:** none changes.

## Test extension

As `user-keymap`: `LAYOUT_TOUR=zoom` with a fresh home, `RISO_HOME=/tmp/riso-keys`.

| Command and action | What to look for |
|---|---|
| Start with no `keymap.json`; click card `b`; write `[{"context": "Shell", "bindings": {"cmd-shift-z": "riso::ToggleZoom"}}]` to `config/keymap.json`; press `cmd-shift-z` | `b` zooms, without restarting. `keymap-reload` with `defaults bound` and one `bound`. |
| Remove that binding from the file; press `cmd-shift-z` | Nothing. `shift-escape` still zooms. |
| Break the file (delete a bracket) | The `WARN`; the default keys still work. |

## Under review

- `riso_keymap/keymap_reload`, new
- `riso/startup`, in its dev version
- `riso_keymap/user_keymap`, if it records the defaults

## Not in this feature

- Menus that show the new keys at once: when the menus can be set again, by need.
- Keys bound by code after startup.
