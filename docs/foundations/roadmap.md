# Foundations roadmap

The foundations are what the layout engine stands on. They are built before the layout engine, in the order of this table. Each one follows the workflow in `AGENTS.md`, one at a time.

A foundation that nothing uses yet cannot be seen in the application. So the ones marked "with its first user" are built right before the layout feature that needs them, not earlier.

| # | Foundation | What it gives | Who needs it | Status |
|---|---|---|---|---|
| 1 | Product build config | The application's name and version, read from `product.toml` when it is built | every engine | production |
| 2 | Log engine, `riso_log` | The lifecycle trace and the diagnostic log | every engine | production |
| 3 | Startup, `riso/startup` | The log first, then the log files, the GPUI app, the main window, the extensions | the application | production |
| 4 | Directories, `riso_paths` | The config, data, and logs directories of the application, from the platform or from `<NAME>_HOME` | the log files now; saved layouts later | production |
| 5 | UI boundary and theme, `riso_ui` | The one crate that may use Zed's GPL crates. First content: the theme's colors and fonts, with fixed defaults | the first layout feature, which draws with the theme | production |
| 6 | Atomic file write, in `riso_paths` | Writing a file in one step, so a crash never leaves half a file | with its first user: saving the user's layout | planned |
| 7 | Tab widgets, in `riso_ui` | `Tab` and `TabBar`, wrapped from Zed's `ui` | with its first user: the tabs container | production |

## What each planned foundation covers

### 4. Directories

- Three directories: config, data, logs. With `<NAME>_HOME` set, they are `config/`, `data/`, and `logs/` under it; otherwise the platform's directories for the product's name.
- The startup takes the logs directory from it. This removes the temporary logs folder and its printed path (`TODO(paths-engine)`).
- No other crate builds a path to the application's files.

### 5. UI boundary and theme

- `riso_ui` is the only crate that depends on Zed's GPL crates (`ui`, `theme`) or holds code copied from them. It exposes riso's own types and functions, and re-exports nothing of Zed. Every other crate draws through it. A test fails when another crate depends on a GPL crate of Zed.
- This keeps the license changeable later: replacing the inside of `riso_ui` frees everything outside it.
- First content: colors by name, the UI font and the code font with their sizes, and Zed's default themes. The values are fixed defaults until a settings engine exists.

### 6. Atomic file write

- Write to a temporary file beside the target, sync it, rename it over the target, sync the folder.

### 7. Tab widgets

- `Tab` and `TabBar` as riso types that wrap Zed's.

## Not in this roadmap

A settings engine. The layout engine does not depend on it. It gets its own roadmap when something must be configurable.
