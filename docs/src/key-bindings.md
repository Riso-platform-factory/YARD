# Key Bindings

Every visible layout node can take keyboard focus, and actions run against the focused node or the active window. YARD binds Zed-style defaults at startup and lets users override, add, or remove bindings in `keymap.json` with live reload.

## Default Key Bindings

### Shell and Command Palette

| Action | macOS | Linux and Windows | Context | Description |
|---|---|---|---|---|
| `yard::ToggleZoom` | `shift-escape` | `shift-escape` | `Shell` | Zoom the focused node to fill the body, or zoom out |
| `yard::Hide` | *(unbound)* | *(unbound)* | `Shell` | Hide the focused node |
| `yard::SelectNextTab` | `cmd-}`, `alt-cmd-right` | `ctrl-pagedown` | `Shell` | Select the next tab in the focused node's tab container |
| `yard::SelectPreviousTab` | `cmd-{`, `alt-cmd-left` | `ctrl-pageup` | `Shell` | Select the previous tab in the focused node's tab container |
| `yard::FocusLeft` | `cmd-k cmd-left` | `ctrl-k ctrl-left` | `Shell` | Move focus to the visible leaf node to the left |
| `yard::FocusRight` | `cmd-k cmd-right` | `ctrl-k ctrl-right` | `Shell` | Move focus to the visible leaf node to the right |
| `yard::FocusUp` | `cmd-k cmd-up` | `ctrl-k ctrl-up` | `Shell` | Move focus to the visible leaf node above |
| `yard::FocusDown` | `cmd-k cmd-down` | `ctrl-k ctrl-down` | `Shell` | Move focus to the visible leaf node below |
| `yard::NewWindow` | `cmd-shift-n` | `ctrl-shift-n` | `Shell` | Open a new window |
| `yard::Dismiss` | `escape` | `escape` | `ModalLayer` | Close the open modal |
| `yard::Quit` | `cmd-q` | `ctrl-q` | *(global)* | Quit the application |
| `command_palette::Toggle` | `cmd-shift-p` | `ctrl-shift-p` | `Shell` | Open or close the [Command Palette](./command-palette.md) |
| `command_palette::SelectPrevious` | `up` | `up` | `CommandPalette` | Move selection up in the command palette |
| `command_palette::SelectNext` | `down` | `down` | `CommandPalette` | Move selection down in the command palette |

Directional focus chords (`cmd-k cmd-left`, etc.) wait up to one second for the second keystroke. Focus moves by `4px` probes from the midpoint of the focused node's edge, crossing container chrome such as tab bars, and stops without wrapping when no leaf node lies in that direction.

`yard::SelectNextTab` and `yard::SelectPreviousTab` target the nearest ancestor `yard.tabs` container of the focused node, wrap around past the first or last tab, exit zoom if a zoomed node lives inside that container, and move keyboard focus to that tabs container.

### Text Input

Single-line text inputs (`TextInput` context) bind standard editing, selection, clipboard, and undo keys for each operating system:

- **All platforms:** `backspace`, `delete`, `left`, `right`, `home`, `end`, `shift-left`, `shift-right`, `shift-home`, `shift-end`, `enter`.
- **macOS:** `alt-backspace` (delete word), `cmd-backspace` (delete to line start), `alt-left`/`alt-right` (word), `cmd-left`/`cmd-right` and `ctrl-a`/`ctrl-e` (line start/end), `alt-shift-left`/`alt-shift-right` (select word), `cmd-shift-left`/`cmd-shift-right` and `ctrl-shift-a`/`ctrl-shift-e` (select to line start/end), `cmd-a` (select all), `cmd-c`/`cmd-x`/`cmd-v` (copy/cut/paste), `cmd-z`/`cmd-shift-z` (undo/redo).
- **Linux and Windows:** `ctrl-backspace` (delete word), `ctrl-left`/`ctrl-right` (word), `ctrl-shift-left`/`ctrl-shift-right` (select word), `ctrl-a` (select all), `ctrl-c`/`ctrl-x`/`ctrl-v` (copy/cut/paste), `ctrl-z`/`ctrl-y`/`ctrl-shift-z` (undo/redo).

## Focus Tracking and Layout Action Refusals

When keyboard focus sits inside nested components, YARD resolves the **innermost** shown non-frame node containing focus. Extensions can query focus, bounds, and tab selection directly:

- `yard_shell::focused_node(window, cx) -> Option<NodeId>`: innermost shown non-frame node containing keyboard focus.
- `yard_shell::node_bounds(cx, node) -> Option<Bounds<Pixels>>`: last drawn bounds of a shown node.
- `yard_shell::selected_tab(cx, tabs) -> Option<NodeId>`: currently selected child of a `yard.tabs` container.

Expected refusals inside `layout-action` log at `INFO` without changing focus or layout:

- No node has keyboard focus (`no node has the focus`).
- The focused node has no drawn bounds (`no bounds for the focused node`).
- Directional focus finds no visible leaf node in that direction (`no node on the left`, `no node on the right`, `no node above`, or `no node below`).
- Tab switching finds no enclosing `yard.tabs` container (`no tabs container holds it`), or the container has zero or one shown tab (`no tabs`, `one tab`).

## User Keymap File

Custom bindings live in `keymap.json` inside the application's config directory (see [Product Config](./building/product-config.md#application-directories)). The file uses Zed's keymap format: a JSON array of objects, each with an optional `context` predicate and a `bindings` map.

```json
[
  {
    "context": "Shell",
    "bindings": {
      "shift-escape": null,
      "cmd-shift-z": "yard::ToggleZoom",
      "cmd-h": "yard::Hide"
    }
  },
  {
    "bindings": {
      "cmd-q": ["zed::Unbind", "yard::Quit"]
    }
  }
]
```

Binding values take three forms:

- `"namespace::ActionName"`: bind the keystroke to a unit action.
- `["namespace::ActionName", <input>]`: bind the keystroke to an action with JSON input passed directly to GPUI (`App::build_action`), which may be any JSON value (not only an object). For targeted removal, `["zed::Unbind", "<action>"]` removes the default binding of that specific action for those keystrokes.
- `null`: bind GPUI's `NoAction`, disabling the entire keystroke in that context.

### Precedence and Validation

- Bindings are loaded in order: shell defaults, extension defaults, then `keymap.json`. At the same context depth, later bindings win, so user bindings override defaults.
- A more specific (deeper) key context takes precedence over a broader context such as `Shell`.
- A missing `keymap.json` is normal and uses defaults only.
- If the file is not valid JSON or not a JSON array, YARD logs a `WARN` and keeps the defaults. If an individual section, context predicate, keystroke, or action name is invalid, YARD logs a `WARN` and skips that entry while still loading the remaining valid bindings.

## Live Reload

YARD checks `keymap.json` once every second while the application runs. When the file changes (or is created or deleted), YARD clears all active key bindings, re-applies the saved shell and extension defaults, and reloads `keymap.json`.

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `focus-change` | `node` | Keyboard focus moves to a layout node |
| `layout-action` | `action`, `node` | Running a shell layout or focus action |
| `keys-bind` | `count` (plus `extension` for extensions) | Binding shell or extension default keys |
| `keymap-load` | `path` | Loading `keymap.json` |
| `keymap-watch` | `path` | Starting the `keymap.json` watcher |
| `keymap-reload` | `path` | Re-applying defaults and reloading `keymap.json` on change |
