# State feature 4: extension-store

## What it adds

An extension keeps named values of its own between runs, through its handle, as Zed's extensions keep their small settings in the key-value store: a value that belongs to no node, such as how many times the tour was opened. Another extension cannot read them. The tour counts its starts in its own store and shows the count.

## How it works

- **Through the handle.** An extension reads and writes its values with its `Extension` handle, by a key: a non-empty string. A value is JSON. The handle reaches only its own extension's values, so that another extension can neither read nor change them: there is no call that takes another extension's name.
- **Where.** Each extension's values are in their own file, `<data>/store/<extension>.json`. `yard_paths` names the folder (`Paths::store_dir`). The format: a `version` (1) and the `values`, a map from key to value. `yard_state` owns it, as it owns `state.json`'s.
- **Reading.** An extension's file is read once, the first time the extension reads or writes a value, and kept in memory for the rest of the run.
  - A missing file means no values: `debug`.
  - A broken file, or one of another `version`, also means no values, with a `WARN`, never an `ERROR`. The next save writes over it, as the state file is written over.
- **Saving.** A write changes the value in memory at once, so the next read gives it back. Storing a value equal to the one there, or removing a key that is not there, is not a change. The file is saved as feature 2 saves the states: 300 ms after the extension's last change, at most 2 s after the first of a burst, on the background executor. At quit, the waits are cancelled, and each extension with a change not yet written is written once more, with `reason=quit`. Only the extensions whose values changed are written, each to its own file.
- **The folder** is given once by the startup, with `set_store_dir`, before any extension starts. A second call, or a call after a store was read, is refused with an `ERROR`, since it is a bug: the folder stays as it was.
- **No folder given.** When the startup gave the shell no store folder (as in tests that do not set one), values are kept in memory for the run only, with a `debug` `no store folder` once.
- **Removing.** Removing a key that is not there does nothing. Removing the last key writes a file with no values.

## What code can do

```rust
// yard_paths (dev part `store_dir`)
impl Paths {
    /// The folder of the extensions' stores.
    pub fn store_dir(&self) -> PathBuf;
}

// yard_shell::dev
impl Extension {
    pub fn stored(&self, cx: &mut App, key: &str) -> Option<serde_json::Value>;
    pub fn store(&self, cx: &mut App, key: &str, value: serde_json::Value);
    pub fn unstore(&self, cx: &mut App, key: &str);
}
pub fn set_store_dir(cx: &mut App, dir: PathBuf);
```

An empty key is refused with an `INFO` `refused {reason}`: `stored` gives `None`, and `store` and `unstore` do nothing.

`yard_state` (dev part `store`): the format's type and `encode_store`/`decode_store`, with their own logging.

## Logging

| Situation | Log |
|---|---|
| The first read or write of an extension | A group `store-read{extension, file}`, ending with `debug` `read {keys}`, or `no saved values`, or the `WARN` |
| `stored`, `store`, `unstore` | `trace` `stored {key}`, `store {key}`, `unstore {key}`: they can run on every keystroke |
| A save | A group `store-save{extension, keys, reason}` holding `file-write{path}`; `reason` is `change` or `quit` |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_state/store` (new, dev) | The store's format |
| `yard_paths/store_dir` (new, dev) | `Paths::store_dir`: `store` in the data directory |
| `yard_shell/shell` dev version | `stored`, `store`, `unstore`, `set_store_dir`, reading, and saving with feature 2's wait and at quit |
| `yard/startup` | Dev copy that gives the shell its store folder. While `extension-store` is under review, `cargo run dev` loads `review` and `layout_tour` on it |
| `examples/layout_tour` (no stage) | In the case `user-layer`, the controls show `starts: N`: each start adds one to the tour's stored `starts` |

## Tests

- **`yard_state`, unit tests:** encode and decode give the same values; a broken file, another `version`, and unknown fields are handled as above.
- **Paths, unit test:** `store_dir()` is `store` in the data directory.
- **Shell, GPUI tests, with the dev shell:**
  - a value stored, quit, a new run with the folder: `stored` gives it back;
  - two extensions store the same key: each reads its own value, each file holds only its own;
  - ten writes 10 ms apart save the file once, 300 ms after the last; quitting with a save waiting writes once;
  - a value stored, the process ended without quitting after the wait: the file holds it;
  - a missing, broken, or other-version file: no values, with the `debug` or the `WARN`, and the next save writes a good file;
  - with no folder: values work for the run, nothing is written;
  - an empty key is refused; removing a key, and the last key.
- **Logging**, alone in its file: `store-read{extension, file}` with `read {keys}`; `store-save{extension, keys, reason=change}` and `reason=quit`, each holding `file-write`.

## Test extension: `examples/layout_tour`

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-store LAYOUT_TOUR=user-layer cargo run dev` | The controls show `starts: 1`; `store-read{extension=layout_tour}` with `no saved values`, then `store-save{extension=layout_tour, keys=1, reason=change}`. |
| Quit, start again | `starts: 2`; `read {keys=1}`; `store/layout_tour.json` holds `"starts": 2`. |
| End the process with `kill -9` after a second, start again | `starts: 3`. |

## Under review

- `yard_state/store`, `yard_paths/store_dir`, and `yard_shell/shell`, `yard/startup` in their dev versions

## Not in this feature

- Secrets in the OS's store of credentials: `secrets`, by need.
- Values shared between extensions: an extension that wants to share offers a function of its own.
