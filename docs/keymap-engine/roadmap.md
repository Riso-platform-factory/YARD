# Keymap engine roadmap

## Goal

The keymap engine makes an application usable from the keyboard. A command is a GPUI action. The engine knows which node has the focus, sends an action there, binds keys to actions, lets the user change the keys in a file, and lets every action be found by its name.

GPUI already has actions, key contexts, key bindings, and the dispatch of an action along the focus path. The engine adds only what GPUI does not have.

## Decisions already made

- Commands are GPUI actions. There is no second command system.
- Default keys are bound in code: by the shell for its own actions, and by each extension in its `init`, inside the key context that its views set.
- The user's keys are in a file that is loaded after the defaults, so it wins. Its entries are turned into bindings with GPUI's `build_action`.
- The command palette is an extension. It lists the actions that GPUI knows.
- The shell's own actions work on the focused node: zoom it, hide it, go to the next or previous tab of its container, and move the focus to the node on the left, on the right, above, or below, by the nodes' places on screen.

## How this roadmap is followed

- Features are built in the order of the table, one at a time. Each one follows the workflow in `AGENTS.md` from foundation to production.
- A feature starts with a short contract in this folder, `docs/keymap-engine/<slug>.md`. The user approves the contract before any code.
- The focus is shown by `examples/layout_tour`: its card says whether it has the focus. A new test extension, `examples/keys_tour`, declares an action of its own and its key.

## Reusing Zed

| From Zed | License | How it is reused |
|---|---|---|
| `gpui`: actions, focus handles, `KeyContext`, `KeyBinding`, `bind_keys`, `clear_key_bindings`, `build_action`, `available_actions`, `bindings_for_action` | Apache-2.0 | Used directly |
| `settings/src/keymap_file.rs`: the format of a keymap file and its loading, 2,811 lines | GPL-3.0-or-later | Not UI, so not copied. riso's loader reads the same format, so a user can bring bindings over from Zed |
| `assets/keymaps/default-macos.json` and the files for Linux and Windows | Part of Zed's repository | Reference for the keys Zed uses: `shift-escape` to zoom, `cmd-}` for the next tab, `cmd-k cmd-left` to focus the pane on the left |
| `workspace`: `find_pane_in_direction` in `pane_group.rs` | GPL-3.0-or-later | Reference for moving the focus by direction |
| `command_palette` (1,847 lines), `picker` (2,064 lines) | GPL-3.0-or-later | They cannot be dependencies: they need `workspace`, the database, and the project. The palette is written with the UI kit, with Zed's as the reference for its behavior. The matching of typed text is riso's own |

Focus and the shell's actions are in `riso_shell`. The user's file is read by a new crate, `riso_keymap`.

## Features

Status is one of: planned, dev, production.

### Focus and the shell's actions

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 1 | `focused-node` | Every shown node can take the keyboard focus, and a click inside a node gives it the focus. The shell knows the focused node, and a component can ask whether it is the one | The focused card says `focused`. A click on another card moves the mark | `gpui`: focus handles | none | production |
| 2 | `layout-actions` | The shell declares actions on the focused node: zoom it or zoom out, hide it, go to the next or the previous tab of its container. An action that cannot apply is refused and logged, like any operation | Buttons in the tour send each action, and the focused card reacts | `gpui`: actions | 1 | production |
| 3 | `default-keys` | The shell binds default keys to its actions at startup: the keys Zed uses on each OS | `shift-escape` zooms the focused card, and `cmd-}` shows the next tab | `gpui`: `bind_keys`. Zed's default keymaps, as reference | 2 | production |
| 4 | `focus-by-direction` | Four actions move the focus to the node on the left, on the right, above, or below, by the nodes' places on screen | In a row with a column inside, the keys walk through the cards | `find_pane_in_direction`, as reference | 3 | dev |

### Extensions

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 5 | `extension-actions` | An extension declares actions and binds their default keys. It handles an action in one of its components, where it runs only while that component has the focus, or for the whole window through its handle | The keys tour's action runs from its key only while its card has the focus. Its window-wide action runs from anywhere | `gpui`: `on_action`, key contexts | 3 | planned |

### The user's keys

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 6 | `user-keymap` | `keymap.json` in the config directory, in Zed's format, is loaded after the defaults. It adds bindings, replaces them, and removes them. An entry that cannot be read is a `WARN` that names it, and the others still load. `riso_paths` names the file | A file that moves zoom to another key: the new key zooms, the old one does not | `gpui`: `KeyBinding::load`, `build_action` | 3 | planned |
| 7 | `keymap-reload` | The file is read again when it changes while the application runs, with the check the settings engine uses | Editing the file changes the key at once | `gpui`: `clear_key_bindings` | 6. Settings engine: `live-reload` | planned |

### Commands by name

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 8 | `command-palette` | An extension that comes with the platform. It lists every action available where the focus is, by a readable name and with its keys, narrows the list as the user types, and runs the chosen action | Its key opens the palette. Typing `zoom` and pressing enter zooms the focused card | `gpui`: `available_actions`, `bindings_for_action`. Zed's palette, as reference | UI kit: `text-input`, `list`, `scroll`, `key-hint`. Window engine: `modal-layer` | planned |

## Open decisions

- **A folder for the platform's extensions.** The command palette is a feature, so it is an extension, not part of an engine. It is the first extension that comes with the platform. Proposed: a new folder, `extensions/`, beside `crates/` and `examples/`. Its crates have stages like the engines, and an application's composition root lists the ones it wants. `AGENTS.md` gets this rule when feature 8 is built. The other choice is to keep the palette in `examples/`, where it has no stage and never reaches production.

## Later

- The keys of other editors as a base, and modes as in Vim.
- An editor for the keymap.
- A popup that shows the keys that can follow a first key.
- Keys for keyboards that are not US keyboards.
- Resizing a split and moving the focused node with the keyboard.
