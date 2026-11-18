# State engine roadmap

## Goal

The state engine is how what the user was looking at survives a restart. The layout engine already saves where things are: the user layer. The state engine saves what is in them: what each component was showing, which tab was selected, and the small values an extension wants to keep.

## Decisions already made

- A component gives its state as JSON, and only what is needed to build its view again. Larger data, such as a document, is stored by the extension itself, and the state keeps a reference to it.
- A state is saved under its node's ID, together with the ID of the node's component. It is given back the first time the node's view is created, if the node still has the same component.
- State is kept in JSON files with a `version`, written in one step. There is no database: the data is small and written as a whole.
- A file that is missing, cannot be read, or has another `version` means a start without state. It is logged, and it is not an error.
- State is saved a short delay after it changes, and once more when the application quits.

## How this roadmap is followed

- Features are built in the order of the table, one at a time. Each one follows the workflow in `AGENTS.md` from foundation to production.
- A feature starts with a short contract in this folder, `docs/state-engine/<slug>.md`. The user approves the contract before any code.
- The test extension is `examples/layout_tour`. Its card already counts clicks, and that count is the state to keep.
- Every run uses a fresh `RISO_HOME`, so the user's real files are never touched.

## Reusing Zed

| From Zed | License | How it is reused |
|---|---|---|
| `gpui`: `on_app_quit`, timers, the OS's store of credentials | Apache-2.0 | Used directly |
| `db`, `sqlez`, `workspace/src/persistence.rs`, `session` | GPL-3.0-or-later | Not UI, so not copied. They are the reference for what Zed remembers between runs: per item, per pane, per window |

The file's format is riso's own code, in a new crate without GPUI, `riso_state`. The shell asks the views for their state and gives it back.

## Features

Status is one of: planned, dev, production.

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 1 | `node-state` | A component can give a small state for its node. The state is saved when the application quits, and the component receives it when the node's view is created at the next start. `riso_paths` names the file | Count a card to 3, quit, start: the card shows 3 | `gpui`: `on_app_quit` | Foundation 6: atomic file write. Layout engine: `save-restore` | production |
| 2 | `save-on-change` | A component says when its state changed. The state is then saved after a short delay, so that a burst of changes writes once, and a crash loses little | Ten fast clicks write the file once. Ending the process without quitting still keeps the count | `gpui`: timers | 1 | production |
| 3 | `view-state` | The shell's own containers keep their state the same way: the selected tab of a tabs container, and the zoomed node | Select the tab `c` and zoom it, quit, start: `c` is selected and zoomed | none | 1 | planned |
| 4 | `extension-store` | An extension keeps named values of its own between runs, through its handle. Another extension cannot read them. `riso_paths` names the folder | The tour stores a value, and shows it again after a restart | none | 1 | planned |

### By need

| Slug | What it adds | Waits for |
|---|---|---|
| `secrets` | An extension keeps a secret in the OS's store of credentials, through its handle | The first extension that needs a password or a token |
| `item-state` | State for each copy that an item slot makes | Layout engine: `item-slots` |

## Later

- Reading a state file written by an older version.
- A database, when an application needs to query its data. Zed's own are GPL, so it would be a permissively licensed crate.
