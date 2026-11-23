# Command Palette

The Command Palette (`extensions/command_palette`) is a platform extension that lets you search and run any action available at the current keyboard focus.

## Opening the Command Palette

Press `cmd-shift-p` on macOS or `ctrl-shift-p` on Linux and Windows (`command_palette::Toggle`). Pressing the shortcut again, pressing `escape`, or clicking outside closes the palette and restores focus to the element that was focused before opening it.

## Searching and Running Actions

When opened, the palette collects all actions available for the previously focused element (excluding `command_palette::Toggle`, `NoAction`, and `Unbind`) and formats each action name for display (for example, `yard::ToggleZoom` appears as `yard: toggle zoom`).

The palette opens in a `38rem`-wide surface with a list height capped at `24rem` (clamped to the window viewport). Its input placeholder is `Execute a command...`; when nothing matches the query, it displays `No matches`.

- **Filtering and ranking:** Type in the query field to filter actions by case-insensitive subsequence match. Matches that hit word starts, match the leading word start, or have fewer gaps rank higher; ties sort alphabetically.
- **Key hints:** Each row shows the key binding currently assigned to that action in the previous focus context, reflecting any overrides from [`keymap.json`](./key-bindings.md#user-keymap-file).
- **Navigation and execution:** Move through matches with `up` and `down` (`command_palette::SelectPrevious` and `command_palette::SelectNext`), wrapping past the top and bottom and scrolling the selected row into view. Press `enter` or click a row to close the palette, restore the previous focus, and dispatch the selected action there.

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `command-run` | `action` | Dispatching the selected command after restoring previous focus |
