# Layout feature 12: save-restore, with foundation 6: atomic file write

## What it adds

The user layer is saved to disk every time it changes. At the next start, the default layout is built as always, and then the saved user layer is applied to it. An operation that no longer fits the default layout is skipped and logged, and the rest is applied. This happens, for example, when its node is gone because an extension changed or was removed.

## Foundation 6: atomic file write, in `yard_paths`

- `write_atomic(path, bytes)` writes the file in one step. A reader sees the old content or the new content, never half a file.
  - It writes to a temporary file beside the target, syncs it, and renames it over the target.
  - It then syncs the folder, where the platform allows it.
  - It creates the parent folders.
  - It runs in a group, `file-write{path}`.
  - A failure is a `WARN` in the diagnostic log, with the path and the error. The caller decides what the failure means.
- **The file of the saved layout is `layout.json` in the data directory.** `yard_paths` names it, through `Paths::layout_file()`, because no other crate builds a path to the application's files.
- This is built here, with its first user, as the foundations roadmap says.

## What is saved

- **Only the user layer:** the list of operations recorded since the default layout was fixed, in order.
- **Not saved:** the shares of splits, the selected tabs, and the zoom. They are view state. Saving them needs a stable way to name a split's children across starts, which belongs to a later feature.
- **The file format is JSON:**
  - a `version` number, starting at 1;
  - a list of operations;
  - each operation names its kind and its fields by their IDs: `add` with node, component, parent, and slot; `remove` with node; `move` with node, parent, slot, and index; `hide` and `show` with node;
  - a `move` to the end of a slot has no `index`, so that the file never holds a number that depends on the platform; a missing `index` means last;
  - the extension that asked for each operation is kept, so that the namespace rule of `add` can be checked again when the operation is restored.
- **The model owns the format.** A new part, `yard_layout/saved`, encodes and decodes it without GPUI. Decoding checks every ID with the slug rules.

## When it is saved

- After every operation that is recorded.
- After every reset. A reset saves an empty user layer.
- A save that fails is a `WARN`. The application keeps running with its layout in memory, and the next save tries again.

## Restoring at startup

The order of startup is unchanged:
1. the extensions build the default layout;
2. the window opens and fixes the default;
3. the saved user layer is applied;
4. the views are created.

In step 3, each saved operation is checked by the same rules as a live one, the shell's and the model's:

| Situation | What happens | Log |
|---|---|---|
| No file | The default layout is used | `debug`: `no saved layout` |
| The file cannot be read, is not valid JSON, or has another `version` | The default layout is used. The file is left as it is until the next save replaces it. | `WARN`, with the reason |
| An operation is refused by the rules (its node or parent is gone, its component is not registered, its slot is not declared, it makes a cycle) | That operation is skipped; the next ones are still tried | `INFO`: `skipped`, with the reason |
| An operation is applied | It is recorded in the user layer again | `debug` |

- Restoring runs in a group, `layout-restore{file}`, inside `window-open`. It ends with `restored {applied, skipped}` at `debug`.
- Skipped operations are not recorded. The next save writes only the operations that were applied, so a skipped operation is dropped from the file.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_paths/write_atomic` (new, dev) | Foundation 6 |
| `yard_paths/layout_file` (new, dev) | `Paths::layout_file()` |
| `yard_layout/saved` (new, dev) | Encoding and decoding the user layer, with its version |
| `yard_shell/shell` | Dev version beside production. It sets the layout file, saves after every recorded operation and every reset, and restores in `new_shell` after the default is fixed and before the views are created. |
| `yard/startup` | Dev copy. It gives the shell `paths.layout_file()` in `app-init`. |

## Test extension: `examples/layout_tour`

Use a fresh home, so that the user's real files are never touched: `YARD_HOME=/tmp/yard-layout`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=user-layer cargo run dev`, drag tab `a` onto `right`'s bar, `hide d`, then quit | `layout-save` after each operation. `data/layout.json` holds the two operations. |
| The same command again | `layout-restore` applies 2 and skips 0. `right` has `c` and `a` as tabs, and `d` is hidden. `user layer: 2`. |
| `reset layout`, quit, and start again | The default layout. The file holds no operation. |
| Rearrange as above, quit, then `LAYOUT_TOUR=tabs cargo run dev` | The `tabs` layout has no `right` and no `d`. Both operations are skipped with `INFO`, and the default `tabs` layout is shown. |
| Write `not json` into `data/layout.json`, then start `user-layer` | A `WARN` that the saved layout cannot be read, and the default layout. |

## Under review

- `yard_paths/write_atomic`, `yard_paths/layout_file`, and `yard_layout/saved`
- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- Saving shares, tab selections, and the zoom.
- Several saved layouts, or one per project.
- Migrating a file of another version.
