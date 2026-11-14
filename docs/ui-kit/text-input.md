# UI kit feature 7: text-input

## What it adds

An input of one line, as Zed's single-line editor in its pickers and search fields: typing, moving and selecting with the keyboard and the mouse, the clipboard, undo, input methods for languages such as Vietnamese or Japanese, and a placeholder. It tells its owner when the text changes and when it is confirmed with enter.

## How it works

Adapted from GPUI's `crates/gpui/examples/input.rs` (Apache-2.0), as the earlier project's `crew_ui` text input did, kept to one line:

- **A view.** `TextInput` is a GPUI view that the owner creates and holds, and draws as a child. It has a focus handle; it takes the keys only while it has the focus. A click on it focuses it.
- **Its look.** No border and no background of its own: the owner puts it in a frame, as Zed's pickers put their editor. The text is in the UI font at the size of Zed's single-line editor, with the editor's line height. While the text is empty, the placeholder is drawn in the theme's placeholder color. The selection uses the theme's selection color. The cursor is a thin bar in the theme's cursor color that blinks every half second while the input has the focus, and stays on while the user types or moves it.
- **One line.** Enter does not insert a line: it confirms. Text pasted with line breaks has them replaced by spaces. Text wider than the input scrolls sideways, so the cursor is always visible.
- **Keys,** in the key context `TextInput`, as Zed binds them for an editor on each OS:

  | What | macOS | Linux and Windows |
  |---|---|---|
  | Delete before, after | `backspace`, `delete` | `backspace`, `delete` |
  | Delete the word before | `alt-backspace` | `ctrl-backspace` |
  | Delete to the line's start | `cmd-backspace` | none |
  | Move by a character, a word | `left`, `right`; `alt-left`, `alt-right` | `left`, `right`; `ctrl-left`, `ctrl-right` |
  | Move to the start, the end | `cmd-left`, `cmd-right`, `home`, `end`, `ctrl-a`, `ctrl-e` | `home`, `end` |
  | Select while moving | the same keys with `shift` | the same keys with `shift` |
  | Select all | `cmd-a` | `ctrl-a` |
  | Copy, cut, paste | `cmd-c`, `cmd-x`, `cmd-v` | `ctrl-c`, `ctrl-x`, `ctrl-v` |
  | Undo, redo | `cmd-z`, `cmd-shift-z` | `ctrl-z`, `ctrl-shift-z`, `ctrl-y` |
  | Confirm | `enter` | `enter` |

  Escape is not bound: it reaches the input's ancestors, so a modal around the input still closes with it.
- **The mouse.** A click places the cursor; a drag selects; a double click selects a word; shift-click extends the selection.
- **Input methods.** The input is GPUI's input handler for its window while it has the focus: composed text is shown underlined while it is being composed, and replaced by the final text.
- **Undo** groups the typing of one word, and every paste or cut alone, as the earlier project's input does.

## What code can do

```rust
// riso_ui
pub enum TextInputEvent { Changed, Confirmed }

/// A GPUI view: `Render`, `Focusable`, `EventEmitter<TextInputEvent>`.
pub struct TextInput;
impl TextInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self;
    pub fn placeholder(self, text: impl Into<SharedString>) -> Self;
    pub fn text(&self) -> &str;
    /// Replaces the text, puts the cursor at its end, and forgets the undo history.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>);
}

/// Binds the input's keys. The startup calls it in `app-init`, as it binds the shell's.
pub fn bind_text_input_keys(cx: &mut App);
```

`Changed` is emitted once for every change of the text, by typing, deleting, pasting, cutting, undoing, composing, or `set_text`; never for a move or a selection. `Confirmed` is emitted on enter; the text is not cleared.

## Logging

| Situation | Log |
|---|---|
| The keys are bound | A group `text-input-keys{count}` inside `app-init`, with one `debug` line per binding: keys and action |
| The text changes | `trace`: `text changed` with `length`, once per change: it follows the keystrokes |
| Enter confirms | `debug`: `text confirmed` with `length` |
| Copy, cut, paste | `debug`: `copied`, `cut`, `pasted` with `length` |
| The text cannot be shaped or painted | `ERROR` with the reason, once per input until the text changes |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/text_input` (new, dev) | `TextInput`, `TextInputEvent`, `bind_text_input_keys`, with GPUI's notice for the example it adapts. The earlier project's `crew_ui/src/dev/text_input.rs` and its `editing` module are ported, with its multi-line parts removed |
| `riso/startup` | Dev version beside production: `riso_ui::bind_text_input_keys(cx)` in `app-init`, after `riso_shell::bind_default_keys` |

## Tests

- GPUI tests, with an input focused in a window of a known size:
  - typing `héllo` gives `héllo`, with one `Changed` per character; `backspace` removes one; `alt-backspace` (macOS) removes the word;
  - moving and selecting with the keys of the table, and typing over a selection replaces it; `cmd-a` then typing replaces everything;
  - copy, cut, and paste through GPUI's test clipboard; pasted line breaks become spaces;
  - undo and redo restore the text as the grouping says;
  - enter emits `Confirmed` once and keeps the text; escape is not handled by the input and reaches an ancestor's handler;
  - composing through the input handler: marked text is shown, then replaced by the final text, with `Changed` for each step;
  - text wider than the input keeps the cursor inside the input's bounds;
  - a click places the cursor, a drag selects, a double click selects a word;
  - `set_text` emits `Changed`, puts the cursor at the end, and clears the undo history;
  - with the input not focused, keys do nothing to it.
- A logging test alone in its file: every row of the table.

## Test extension: `examples/ui_gallery`

A new page, `text-input`. It shows two inputs in frames drawn as Zed's picker draws its editor: the first empty with the placeholder `Search…`, the second with the text `hello world`. Under each, a muted label shows the last event: `changed: <text>` or `confirmed: <text>`.

| Command and action | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, open the `text-input` tab, click the first input | The cursor blinks at its start; the placeholder stays until the first character. |
| Type `xin chào` with the macOS Vietnamese input method (Telex), then press enter | The composed letters appear underlined, then final: `xin chào`. The label: `confirmed: xin chào`. |
| In the second input, `alt-left`, `shift-cmd-left`, `cmd-c`, click the first input, `cmd-a`, `cmd-v` | The first input reads `hello `, as copied. |
| Type until the text is wider than the input | The text scrolls; the cursor stays visible. |
| `cmd-z` several times | The edits come back word by word. |
| `{"theme": {"mode": "light"}}` | The text, the placeholder, the selection, and the cursor follow the theme. |

## Under review

- `riso_ui/text_input`, new
- `riso/startup`, in its dev version

## Not in this feature

- Several lines, wrapping, and vertical scrolling: `text-area`, by need.
- A frame, an icon, or a clear button around the input: its owner draws them.
- Right-click menus on the input: with `context-menu`, by need.
