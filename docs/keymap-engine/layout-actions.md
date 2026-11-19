# Keymap feature 2: layout-actions

## What it adds

The shell declares GPUI actions that act on the focused node: zoom it or zoom out, hide it, and go to the next or the previous tab of its tabs container. Anything that sends an action reaches them, such as a key in the next feature, a button, or a command palette later. An action that cannot apply is refused and logged, like any operation.

## The actions

They are in the namespace `yard`, as the shell's components are:

| Action | What it does | As Zed's |
|---|---|---|
| `yard::ToggleZoom` | Zooms the focused node; when a node is zoomed, zooms out | `workspace::ToggleZoom` |
| `yard::Hide` | Hides the focused node | Zed hides a dock; yard hides any node |
| `yard::SelectNextTab` | Selects the next tab of the tabs container that holds the focused node, after the last one the first | `pane::ActivateNextItem` |
| `yard::SelectPreviousTab` | The previous tab, before the first one the last | `pane::ActivatePreviousItem` |

- **"The focused node"** is the innermost node with the focus, as `focused-node` defines it.
- **"Its tabs container"** is the nearest `yard.tabs` node at or above it. A focused tabs container selects among its own tabs. A card inside a row inside tabs selects among those tabs.
- **The operations are the shell's own,** the ones an extension already uses: zooming, zooming out, hiding, and selecting a tab. Each is checked by the same rules. The shell acts on its own behalf, as extension `yard`.
- After `Hide`, no node has the focus, as `focused-node` says of a hidden node. The focus goes to the shell's root, which is no node, so that the next action still reaches the shell and is refused.
- After a tab is selected, the focus goes to the tabs container, so that the next `SelectNextTab` continues there.
- When a node strictly inside the tabs container is zoomed, selecting a tab zooms out first, so that the new tab is seen and the container can take the focus. When the tabs container itself, or a node above it, is zoomed, the zoom stays.

## Where they are handled

- Each node's wrapper handles the four actions. GPUI sends an action along the focus path from the innermost element out, so the innermost node's wrapper handles it, and the ones outside it do not see it.
- The shell's root handles them too, for when no node has the focus.

## Rules and logging

Each action runs in a group, `layout-action{action, node}`, with `node` the focused node or `none`:

| Situation | What happens | Log |
|---|---|---|
| The action applies | The operation runs, with its own group inside | `debug`: `done` |
| No node has the focus | Nothing | `info`: `refused`, reason `no node has the focus` |
| `ToggleZoom` on the root | Nothing | the zoom's own refusal: `the root already fills the window` |
| `Hide` on the root | Nothing | the model's own refusal |
| `SelectNextTab` or `SelectPreviousTab` with no tabs container at or above the node | Nothing | `info`: `refused`, reason `no tabs container holds it` |
| A tabs container with one tab | Nothing | `info`: `refused`, reason `one tab` |
| A tabs container with no shown tab | Nothing | `info`: `refused`, reason `no tabs` |

## What code can do

```rust
actions!(yard, [ToggleZoom, Hide, SelectNextTab, SelectPreviousTab]);
pub fn selected_tab(cx: &App, tabs: &str) -> Option<NodeId>;
```

`selected_tab` gives the node shown by a tabs container. Tests, and later the state engine, need it.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_shell/shell` | Dev version beside production. The actions, their handlers, next and previous tab, `selected_tab`, and the logging |
| `yard/startup` | Dev copy, with the dev shell |

`examples/layout_tour` builds against the dev shell.

## Tests

- GPUI tests, with GPUI's dispatch of an action to the focused element:
  - `ToggleZoom` on a focused card zooms it, and again zooms out;
  - `Hide` hides it, and leaves no node focused;
  - in tabs `a`, `b`, `c`, with `a` focused: `SelectNextTab` selects `b`, then `c`, then `a`; `SelectPreviousTab` from `a` selects `c`;
  - a card in a row inside tabs selects among those tabs;
  - each refusal of the table.
- A logging test alone in its file: every row, with its level and its group.

## Test extension: `examples/layout_tour`

A new case, `actions`: a row with a `controls` card, card `a`, and a tabs container `right` with cards `b`, `c`, and `d`. The `controls` card has one button per action. Its buttons send the action without taking the focus: they stop the mouse-down from focusing, then send the action on click.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=actions cargo run dev`, click a button | Refused: `no node has the focus`. |
| Click card `a`, then `zoom` | `a` fills the window. `zoom` again: back. |
| Click card `b`, then `next tab` three times | `c`, `d`, then `b` again. `previous tab`: `d`. |
| Click card `a`, then `next tab` | Refused: `no tabs container holds it`. |
| Click card `a`, then `hide` | `a` is hidden, and no card says `focused`. |

## Under review

- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- Keys for the actions: `default-keys`.
- Showing a hidden node again by action, and closing a node: by need.
