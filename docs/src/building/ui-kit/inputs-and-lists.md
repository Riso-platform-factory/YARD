# Inputs, Lists, and Scrolling

## Text Input

`TextInput::new(window, cx)` is a borderless single-line editor entity with selection, clipboard actions, IME composition, placeholder text (`.placeholder(text)`), and undo/redo.

- **Key bindings:** `yard_ui::bind_text_input_keys(cx)` registers the `TextInput` key-context actions (called automatically when `yard_shell` starts; see [Key Bindings](../../key-bindings.md#text-input)).
- **Events (`TextInputEvent`):**
  - `TextInputEvent::Changed`: emitted whenever the text content changes.
  - `TextInputEvent::Confirmed`: emitted when the user presses `Enter` (ignored while IME marked text is active). Pressing `Enter` does not insert a newline and does not clear the input.
- **Single-line normalization:** Pasted line breaks (`\r\n`, `\r`, `\n`, `\u{0085}`, `\u{2028}`, `\u{2029}`) are converted to spaces.
- **Reading and replacing text:** `text(&self) -> &str` returns the current text. `set_text(&mut self, text, cx)` converts line breaks to spaces, moves the cursor to the end of the text, clears the undo/redo history, resets horizontal scroll, and emits `TextInputEvent::Changed` if the text changed.

## Lists

`List::new()` arranges `ListItem` rows vertically with an optional `ListHeader` (`.header(...)`) and `ListSeparator` dividers.

- **Empty state:** When a `List` has no children, it shows `"No items"` by default. Override this with `.empty_message(message)`.
- **Row options (`ListItem::new(id)`):** `.toggle_state(bool)`, `.disabled(bool)`, `.selectable(bool)`, `.spacing(ListItemSpacing)`, `.inset(bool)`, `.indent_level(usize)`, `.start_slot(element)`, `.end_slot(element)`, and `.on_click(|window, cx| ...)`.
- **Spacing (`ListItemSpacing`):** `Dense` (default), `ExtraDense`, or `Sparse`.
- **Clicks:** Clicking an enabled `ListItem` runs its handler inside `tracing::debug_span!("list-item-click", id = %id)`. Disabled items ignore clicks.

## Scrolling

`yard_ui` wraps Zed's vertical scrollbar in two containers:

- **`ScrollArea::new(id, &handle)`:** Takes a `gpui::ScrollHandle` and arbitrary vertical children via `ParentElement`. Use this for variable-height content such as forms or settings panels.
- **`ScrollList::new(id, count, &handle, rows)`:** Takes a row count, a `gpui::UniformListScrollHandle`, and a closure `Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static` backed by GPUI's `uniform_list`. Use this for long lists of equal-height rows; only the visible range is constructed each frame.

## Reference

### Defaults

| Type / Widget | Default |
|---|---|
| `List` empty message | `"No items"` |
| `ListItemSpacing` | `Dense` |

### Lifecycle Trace Groups

| Group | Level | Fields | Emitted when |
|---|---|---|---|
| `text-input-keys` | `INFO` | `count` | `bind_text_input_keys` registers `TextInput` bindings |
| `list-item-click` | `DEBUG` | `id` | An enabled `ListItem` click handler runs |
