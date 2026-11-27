# Layout

The body of every window holds a tree of layout containers and views. Extensions define the default layout, and users can split, rearrange, resize, hide, show, and zoom nodes. Changes are recorded in a user layer and saved between runs.

## Splits and Tabs

The shell provides three built-in layout containers:

- **Row (`yard.row`):** Arranges children horizontally from left to right.
- **Column (`yard.column`):** Arranges children vertically from top to bottom.
- **Tabs (`yard.tabs`):** Displays a tab bar at the top and shows one selected child at a time.

### Tab Bar Behavior

- **Titles and icons:** Each tab shows its node's title and optional icon (or the node ID if the component provides no title). The selected tab's title uses the default text color; inactive tabs use the muted text color.
- **Overflow:** Tabs keep their natural width. When tabs exceed the bar's width, the tab bar scrolls horizontally with a trackpad gesture or mouse wheel (horizontal or vertical). Selecting a tab automatically scrolls it fully into view. Tab-bar scroll position is not saved between runs.
- **Closing tabs:** Hovering over a tab reveals a close button (`Close Tab` tooltip) on the right side of the tab. Clicking the close button or middle-clicking the tab removes the node. Required nodes have no close button and ignore middle clicks. Closing, hiding, or losing the selected tab falls back to selecting the first shown tab.
- **Item tabs:** Tabs backed by an extension's dynamic item list (such as open documents) show a `+` button (`New Item`) at the end of the bar outside the scrolling area, and closing a tab asks the owning model to close that item.
- **Keyboard switching:** Move between tabs in the focused container with `yard::SelectNextTab` (`cmd-}` / `alt-cmd-right` on macOS, `ctrl-pagedown` on Linux and Windows) and `yard::SelectPreviousTab` (`cmd-{` / `alt-cmd-left` on macOS, `ctrl-pageup` on Linux and Windows).

## Drag to Move and Split on Drop

Any tab in a `yard.tabs` container can be dragged to another place in the same window. Moving a node preserves its view and state.

While dragging, valid drop zones highlight using the theme's `drop_target` color:

- **Tab bar:** Dropping onto another tab inserts the node at that tab's position; dropping onto the empty space at the end of the bar appends it last.
- **Center of a tabs container:** Dropping onto the center of a tabs container's content moves the node into that container as the last tab and selects it.
- **Edges of a split child or tabs container:** Each edge zone is the outer 20% of the node's shorter side (highlighting that half of the node):
  - Dropping on an edge **along** the parent split's axis (left/right in a row, top/bottom in a column) moves the node directly into that split before or after the child.
  - Dropping on an edge **across** a split's axis, or on **any** of the four edges of a tabs container, creates a new `yard.row` or `yard.column` around the target with equal shares and places the dropped node in a new `yard.tabs` container on that side. New containers receive node IDs `split-<n>` and `tabs-<n>` using the first unused positive integer `<n>`, and an emptied source `yard.tabs` container stays in place.

Drops that would create a cycle (dropping a node onto itself or into its own subtree) or violate a slot or context rule are refused without changing the layout.

## Resizing Splits and Snap-Hide

Children in a row or column can be either **flexible** (sharing remaining space by weight, default share `1`) or **fixed** (keeping a pixel width or height, or a component's preferred size in rems). When a child appears in both `sizes` and `shares`, `sizes` takes precedence.

- When a window grows, flexible children absorb the extra space while fixed children keep their size.
- When a window shrinks, flexible children shrink down to their minimums first; only then do fixed children shrink proportionally, never below their own minimums.
- In a split where every shown child is fixed, the last shown child absorbs any remaining or deficit space so the split always fills its area.
- Every component has a minimum size in rems (default `5rem` wide by `6.25rem` tall, which is `80px` by `100px` at a `16px` UI font). Container minimums come from their shown children (or the component's own `min` hint if larger):
  - **Splits:** the sum of shown children's minimums along the split's axis, and the largest shown child minimum across it.
  - **Tabs:** the largest minimum among its children, plus the tab bar's height.

### Dragging and Resetting Handles

- Drag the `4px` resize handle centered on the `1px` divider between two children to resize them. Releasing the mouse records the resulting shares and fixed sizes in the user layer.
- Double-click a handle to reset flexible shares in that split back to equal proportions (fixed sizes remain fixed).
- **Snap-hide:** Dragging a divider so a child would become smaller than half its minimum snaps that child shut. Releasing hides the child (`hide` operation) while keeping its previous share or fixed size. Dragging back past half the minimum before releasing cancels the snap. Required nodes cannot be snapped shut and stop at their minimum.
- **Snap-open:** Any hidden child in a split (whether hidden by snapping or by an action) leaves a thin grab line at its position. Dragging that line out past half the child's minimum shows the child again (`show` operation).

## Zoom

Focusing any node inside the window body and running `yard::ToggleZoom` (`shift-escape` or **View > Zoom**) expands that node to fill the entire body while keeping the title bar and status bar visible. Running `yard::ToggleZoom` again restores the full layout.

- A zoom target must be a shown body node with an existing view. Zooming a node in the title bar or status bar, the body's root node, a hidden node, or an item-slot copy is refused.
- Zoom automatically ends if the zoomed node is hidden, removed, or becomes empty, on a layout reset, or when switching named layouts.

## Hiding, Showing, and Toggle Buttons

- `yard::Hide` (**View > Hide Node** or via the Command Palette) hides the focused node. Hidden nodes retain their view and in-memory state, take no space in splits, preserve their split share or fixed size, and drop their tab from tab bars. Hiding a container hides its entire subtree without altering descendants' own hidden flags.
- Required nodes (and any ancestor containing a required node) cannot be hidden or removed.
- Extensions can place `yard.toggle` buttons (commonly in the status bar) that show or hide a target node on click. A toggle configured with `alone: true` hides other visible siblings in the same slot before showing its target.
- Components that report themselves empty take no space in splits and tabs automatically without recording a hide operation; they reappear as soon as they have content again.

## Saving, Restoring, and Resetting

Layout changes made by the user (moves, splits, resizes, hides, shows, closed tabs) are stored as a sequence of operations in a **user layer** on top of the default layout:

- The main window saves its default layout's user layer to `layout.json` in the application's data directory.
- Other [named layouts](./layout/named-layouts.md) save their user layers to `layouts/<name>.json`.
- Multi-window sessions store each window's per-layout user layers in `session.json` (see [Windows](./windows.md#window-bounds-and-session-restore)).
- Selected tabs (`yard.tabs`) and zoom state (`yard.window`) are saved in `state.json` and restored on startup.
- Resetting the layout discards the current layout's user layer, clears zoom, and restores the default layout (or the active named layout's definition).

### Saved File Format and Replay

Layout files use version `1` JSON with an `"operations"` array:

```json
{
  "version": 1,
  "operations": [
    { "op": "hide", "node": "sidebar" },
    { "op": "move", "node": "notes", "parent": "main_tabs", "slot": "children", "after": "editor" }
  ]
}
```

- Supported `"op"` values are `"add"`, `"remove"`, `"move"`, `"hide"`, `"show"`, and `"props"`.
- `"add"` records `"node"`, `"component"`, the originating `"extension"`, `"parent"`, `"slot"`, optional `"props"`, and an optional position.
- Position on `"add"` and `"move"` uses at most one of four mutually exclusive fields: `"index"`, `"first"`, `"before"`, or `"after"`. Omitting them places the node last.
- If a file is unreadable, malformed JSON, or has an unsupported version, YARD logs a `WARN` and falls back to the default layout.
- On restore, each well-formed operation is validated against current shell rules; any refused operation (for example, a missing node or a newly required node) is skipped with an `INFO` reason and drops out on the next save while the remaining operations still apply. While an extension is turned off in the settings (see [Settings](./settings.md#turning-extensions-off)), skipped operations are kept in their place instead and written back, so turning the extension on again restores the layout; the `restored` line counts them as `kept`.

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `layout-save` | `operations` | Saving a window's user layer to disk |
| `layout-restore` | `file`, or `layout` for a window that keeps its named layouts' user layers in memory | Restoring a saved user layer |
| `layout-reset` | `extension` | Discarding the active layout's user layer |
| `split-on-drop` | `node`, `target`, `edge` | Splitting a target on drag-and-drop |
| `zoom` | `node`, `extension` | Zooming a node |
| `zoom-out` | `extension` | Exiting zoom |
