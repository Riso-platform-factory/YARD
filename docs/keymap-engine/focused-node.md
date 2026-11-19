# Keymap feature 1: focused-node

## What it adds

Every shown node can take the keyboard focus, and a click inside a node gives it the focus. The shell knows which node has the focus, and a component can ask whether it is its own. This is what the keyboard's actions will act on.

## How

- **Each node has a GPUI focus handle,** made when its view is made and dropped with it.
- **Each node's view is drawn inside a wrapper** that tracks that handle. So every place that draws a node tracks its focus:
  - the window's root;
  - a split's child, and the selected tab;
  - a slot that a component draws itself.
- **A click gives the focus to the innermost node under it.** GPUI does this by itself when the mouse goes down on a tracked element, and the innermost one wins:
  - a click on a card inside a row focuses the card;
  - a click on a tab bar, outside every tab's view, focuses the tabs container.
- **A container knows that it contains the focus** without having it.
- **A control inside a component can hold the focus itself,** such as a text input with its own GPUI handle. Its node is then the focused node, because it is the innermost node that contains the focus. The shell never takes the focus from it.
- When the window opens, no node has the focus.
- When the focused node stops being shown, because it is hidden, removed, or its tab is not selected, GPUI reports the focus lost, and no node has the focus.

## What code can do

```rust
pub fn focused_node(window: &Window, cx: &App) -> Option<NodeId>;
pub fn focus_handle(cx: &App, node: &str) -> Option<FocusHandle>;
```

- `focused_node` gives the innermost shown node that contains the focus, or nothing.
- `focus_handle` gives a shown node's handle, and nothing for a node that is hidden, inside a hidden node, in a tab that is not selected, or outside the zoomed node. A component uses it to ask whether its own node has the focus (`is_focused`) or contains it (`contains_focused`), while it draws. GPUI draws the window again when the focus moves, so a component needs nothing else to follow it.
- `FocusHandle` is GPUI's.

## Logging

| Situation | Log |
|---|---|
| The focused node changes, by a click or by code | A group `focus-change{node}` at `info`, with `debug`: `focused` |
| The focus leaves every node | `focus-change{node=none}`, with `debug`: `focused` |
| A click inside the node that already has the focus | Nothing |

The change is seen when the window next draws, as GPUI reports focus changes once per frame. Two changes before one draw log only the last.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_shell/shell` | Dev version beside production. A focus handle per node, a wrapper that tracks it, the two functions, and the logging |
| `yard/startup` | Dev copy, with the dev shell |

`examples/layout_tour` builds against the dev shell while this feature is under review.

## Tests

- GPUI tests:
  - no node has the focus when the window opens;
  - a click on a card in a row focuses it, and a click on another moves the focus;
  - a click on a card inside a column inside a row focuses that card, and the column and the row contain the focus;
  - a click on a tab bar focuses the tabs container;
  - selecting another tab, hiding the focused node, and removing it each leave no node focused;
  - `focus_handle` gives nothing for a node that is not in the tree, and for a hidden node;
  - a focusable control inside a card keeps the focus when clicked, and its card is the focused node.
- A logging test alone in its file: every row of the table.

## Test extension: `examples/layout_tour`

The card's text gains a line: `focused` when its node has the focus, `contains focus` when it contains it, and nothing otherwise. Use a fresh home: `YARD_HOME=/tmp/yard-layout`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=row cargo run dev` | No card says `focused`. |
| Click card `b`, then card `c` | `b` says `focused`, then `c` does and `b` no longer does. `focus-change{node=b}`, then `focus-change{node=c}`. |
| `LAYOUT_TOUR=nested-split cargo run dev`, click `c` | `c` says `focused`. |
| `LAYOUT_TOUR=tabs cargo run dev`, click card `a`, then the tab `b` | `a` says `focused`. Selecting `b` shows `b`, and `focus-change{node=…}` names the tabs container, because the click was on its tab bar. |

## Under review

- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- Actions on the focused node, and keys: the next features.
- Moving the focus by direction: `focus-by-direction`.
- Drawing the focus, such as a ring around the focused node: the shell draws no mark of its own. A component shows its focus if it wants to.
