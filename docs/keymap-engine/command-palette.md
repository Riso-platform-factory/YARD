# Keymap feature 8: command-palette

## What it adds

The first extension that comes with the platform, in the new folder `extensions/`: Zed's command palette. `cmd-shift-p` opens it; it lists every action available where the focus was, by a readable name and with its keys; typing narrows the list; enter runs the chosen action where the focus was.

## How it works

As Zed's `crates/command_palette` and its picker (GPL, so read and rebuilt from riso's own parts, not copied):

- **An extension.** The crate `extensions/command_palette` is an extension like any other: it binds its key and handles its action through its handle (`bind_keys`, `on_action`), opens its view with `toggle_modal`, and is listed by the composition root. riso's own composition root lists it, so riso has a command palette.
- **Opening.** `command_palette::Toggle`, bound to `cmd-shift-p` on macOS and `ctrl-shift-p` elsewhere in the context `Shell`, as Zed's. Pressed again while the palette is open, it closes it, as `toggle_modal` does.
- **The commands.** The actions available from the focused element at the moment the palette opens (`Window::available_actions`), each once, without GPUI's `NoAction` and `Unbind` and without the palette's own `Toggle`. Each is shown by Zed's readable form of its name (`humanize_action_name`: `riso::ToggleZoom` reads `riso: toggle zoom`), with its keys at the right (the UI kit's `KeyHint` from the focus the palette was opened from).
- **Narrowing.** The query is matched against the readable names, case-insensitively, as a subsequence of characters, as Zed's fuzzy matcher accepts. Matches are ranked: a match at the start of a word before one inside a word, fewer gaps before more, then by name. With an empty query, every command, by name. Matched characters are not highlighted in this feature.
- **Moving and running.** `up` and `down` move the selection, wrapping at the ends; the list scrolls to keep it in view. `enter` runs the selected command: the palette closes, the focus goes back to where it was, and the action is dispatched from there. A click on a row runs it too. `escape` and a click outside close the palette without running anything, as the modal layer does.
- **Its look,** as Zed's command palette: a modal 38 rems wide, near the top of the window, the UI kit's text input with the placeholder `Execute a command...` at the top, a divider, then the list, at most about twelve rows tall, scrolling beyond. An empty match shows `No matches`.

## What code can do

```rust
// extensions/command_palette
gpui::actions!(command_palette, [Toggle]);
/// The extension's init, listed by a composition root.
pub fn init(cx: &mut App, ext: &riso_shell::Extension);
pub fn humanize_action_name(name: &str) -> String;
```

These are the only public items. For tests, the rows carry the debug selectors `PALETTE-ROW-<index>`, from 0 at the top.

## Logging

| Situation | Log |
|---|---|
| The palette opens | Inside the modal layer's `modal-show`, `debug`: `commands listed` with `count` |
| The query changes | `trace`: `commands matched` with `query_length` and `count`: it follows the keystrokes |
| A command runs | A group `command-run{action}` around the dispatch, so the action's own lines are inside it |
| The key is bound | The extension's `keys-bind` group, as every extension's keys are logged |

## Where the code changes

| Part | What changes |
|---|---|
| `command_palette/palette` (new, dev) | The extension: its action and key, its view, the matching, the running |
| `AGENTS.md` | The rule of `extensions/`: the extensions that come with the platform are crates in `extensions/`, with stages and tests like the engines; they reach the platform only through `riso_shell`'s extension handle and the UI kit; a composition root lists the ones it wants; `cargo test -p riso` checks them as it checks the engines |
| `app/riso/tests/stages.rs`, `gpl_boundary.rs` | They read `extensions/` as they read `crates/`: the same stage rules, and no GPL crate of Zed in an extension. These tests change; their change is listed when it is made |
| `riso/startup` or the composition root | riso's composition root lists `("command_palette", command_palette::init)` among its extensions; at review, `review.rs` does |

## Tests

- Plain tests of `humanize_action_name` on Zed's own examples, and of the matching and ranking: a subsequence matches, a non-subsequence does not, word starts rank first, fewer gaps rank first, ties by name.
- GPUI tests, with the shell, a focused card, and test actions bound in its context:
  - `cmd-shift-p` opens the palette with the input focused; the list holds the card's and the shell's actions, by readable name, without `NoAction`, `Unbind`, or `Toggle`; `cmd-shift-p` again closes it;
  - typing narrows the list as the ranking says; an empty match shows `No matches`;
  - `down`, `up`, wrapping; `enter` runs the selected action once on the card (its handler sees the card focused) and closes the palette; the focus is back on the card;
  - a click on a row runs it; `escape` runs nothing.
- A logging test alone in its file: every row of the table.

## Test extension

The palette is shown in riso itself: `LAYOUT_TOUR=zoom cargo run dev` gives it cards and the shell's actions. Use `RISO_HOME=/tmp/riso-palette`.

| Command and action | What to look for |
|---|---|
| Click card `b`, press `cmd-shift-p` | The palette near the top, as Zed's: the input with `Execute a command...`, the list of commands, each with its keys at the right. `commands listed {count=…}`. |
| Type `zoom`, press enter | `riso: toggle zoom` is first; enter closes the palette and `b` zooms. `command-run{action=riso::ToggleZoom}` with `layout-action` inside it. |
| Open it, type `fcs`, move with `down` and `up` | `riso: focus …` commands, ranked; the selection moves and wraps. |
| Open it, press escape | It closes; nothing runs. |
| Beside Zed's command palette | The width, the input row, the rows, and the keys match. |

## Under review

- `command_palette/palette`, new
- The `AGENTS.md` rule of `extensions/`

## Not in this feature

- Recently used commands first, as Zed's history: by need.
- Highlighting the matched characters: with a highlighted label in the UI kit, by need.
- Hiding commands by namespace, as Zed's `CommandPaletteFilter`: by need.
