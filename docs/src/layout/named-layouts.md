# Named Layouts

An application or its extensions can register multiple named layouts for different workflows (for example, writing versus reviewing). Each window shows one named layout at a time and can switch between them independently.

## Default Layout and Switching

- Every window starts with the built-in default layout, named `yard.default`.
- A named layout is defined as a sequence of operations applied on top of the startup default layout, followed by that named layout's own user layer.
- When a window switches to another named layout:
  - Switching to the layout already shown is a no-op (`debug`: `already shown`).
  - Switching to an unknown layout name is refused with an `INFO` log (`no named layout`).
  - Nodes whose ID and component exist in both layouts keep their existing views and in-memory state, even if they moved to a different container.
  - Nodes that leave the layout have their views dropped; newly appearing nodes create fresh views.
  - Active zoom is cleared. Focus remains on the focused node if it is still visible; otherwise focus returns to the window.

## Per-Layout User Layers and Files

Each named layout keeps its own user layer, separate from every other named layout, using the same version-1 `operations` format as [`layout.json`](../layout.md#saved-file-format-and-replay):

- `yard.default` stores the main window's user layer in `<data>/layout.json`.
- Every other named layout `<name>` stores the main window's user layer in `<data>/layouts/<name>.json` when startup configures `yard_shell::set_named_layout_files`. If `set_named_layout_files` is not set, non-default named layout user layers stay in memory only (logging a once-only `WARN`).
- In multi-window sessions, `session.json` records which named layout each open window is showing along with each window's user layers, so every window reopens on the named layout it was using.

Modifying a layout (dragging tabs, resizing splits, hiding panels) records operations only in the active named layout's user layer. Resetting a layout clears only the currently shown named layout's user layer and leaves all other named layouts untouched.

For registering and switching named layouts from code, see [Layout and Components](../building/extensions/layout-and-components.md#named-layouts).

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `layout-switch` | `from`, `to` | Switching a window's active named layout |

Saving, restoring, and resetting use the same `layout-save`, `layout-restore`, and `layout-reset` groups described in [Layout](../layout.md#reference).
