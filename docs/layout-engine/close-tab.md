# Layout feature 24: close-tab

## What it adds

A tab has a close button, as Zed's tabs do. Clicking it, or clicking the tab with the middle mouse button, removes the tab's node with a `remove` operation: the node and everything under it leave the tree, their views are dropped, and the operation is recorded and saved like the others. The tab of a required node (feature 15) has no button, and a middle click on it does nothing.

## How it works

As Zed's `Pane::render_tab` (`crates/workspace/src/pane.rs`) with its default settings (`close_position: right`, `show_close_button: hover`):

- **The button** is in the tab's end slot: an `IconButton` with the `Close` icon, square, `ButtonSize::None`, `IconSize::Small`, muted. It is shown when the pointer is over the tab, and always on the selected tab, as Zed's `visible_on_hover` shows it on the tab's hover group.
- **Its tooltip** is `Close Tab`, from the UI kit's `Tooltip` (UI kit feature 10).
- **A middle click** on a tab closes it, as Zed's `on_aux_click` does.
- **Closing** runs `remove(node)` through the same path as any `remove`: the shell's rules hold (a required node, or a node holding one, is refused with feature 15's `INFO`, which the missing button makes rare), and the tabs container selects its first shown child if the closed tab was selected, as it does after any `remove`.
- **Who may close.** Closing is the user's action through the shell; it records the operation with `extension=riso`, as a drag does.

## In the UI kit

`riso_ui`'s `Tab` gains `on_close(f)`: with it, the tab draws Zed's close button in its end slot and calls `f` on a click of the button or a middle click on the tab. Without it, the tab has no button and ignores middle clicks, as today.

## What code can do

```rust
// riso_ui (production, in place)
impl Tab {
    pub fn on_close(self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self;
}
```

The dev shell has every item of the production shell, unchanged; the change is in its tabs container.

## Logging

| Situation | Log |
|---|---|
| A tab is closed | `debug` `tab close {node}`, then the `layout-op{op=remove, node, extension=riso}` of feature 8, with `view dropped {node}` for each view dropped |
| The close button is drawn | Nothing beyond the tab's own `trace` lines |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/tabs` (production, in place) | `Tab::on_close`, the close button and the middle click. Stamped again. |
| `riso_shell/shell` dev version | The tabs container gives every tab but a required node's an `on_close` that removes its node |
| `riso/startup` | Dev copy that uses the dev shell. While `close-tab` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **UI kit, GPUI tests:** a tab with `on_close` has a close button in its end slot (`debug_bounds`), hidden until the pointer is over the tab and shown on the selected tab; clicking it calls `f` once; a middle click on the tab calls `f` once; a tab without `on_close` has no button and ignores a middle click.
- **Shell, GPUI tests, with the dev shell:**
  - clicking a tab's close button removes its node with one `remove` operation, drops its view, and selects the first shown tab if it was selected;
  - a middle click does the same;
  - a required node's tab has no button, and a middle click on it changes nothing;
  - the close is saved and restored, and a reset brings the tab back.
- **Logging**, alone in its file: `tab close` then `layout-op{op=remove, extension=riso}` with `view dropped`.

## Test extension: `examples/layout_tour`

The case `titles` (feature 14) is used as it is, with one change: `gamma` is declared required, so that its tab shows no button.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-close LAYOUT_TOUR=titles cargo run dev`, rest the pointer on tab `Beta` | A close button appears at its right; resting on it shows `Close Tab`. The selected tab `Alpha` shows its button too. |
| Click `Beta`'s close button | `Beta` goes; `tab close {node=beta}`, `layout-op{op=remove, node=beta, extension=riso}`, `view dropped {node=beta}`. |
| Middle-click tab `Alpha` | `Alpha` goes, and `gamma` is selected. |
| Rest on `gamma` | No close button: it is required. |
| Quit and start again, then `reset layout` | The closed tabs are still closed; after the reset, all three are back. |

## Under review

- `riso_ui/tabs`, in place
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- A key for closing the selected tab (Zed's `cmd-w`, `pane::CloseActiveItem`): with the keymap engine, when a feature needs it.
- Asking before closing an item with unsaved changes: that is an application's, through the window engine's prompts.
- Closing other tabs, closing all, pinned tabs.
