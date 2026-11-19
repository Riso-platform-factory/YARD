# Layout feature 7: hide-show

## What it adds

A node can be hidden while the application runs, and shown again.
- A hidden node keeps its view and its state.
- It gives its space to its siblings: a split shares its length among the children still shown, and a tabs container drops its tab.
- When the node is shown again, it is back in its place with the state it had.

## Model

- The tree keeps, for each node, whether it is hidden. Every node starts shown.
- An extension hides or shows a node through its handle: `set_hidden(node, hidden)`.
  - Any extension may hide or show any node, as Zed's actions toggle any dock. Rules about who may change what come with feature 8, `operations`.
- Hiding a node hides everything under it. Its descendants keep their own flags.

## Rules

| Situation | What happens | Log |
|---|---|---|
| The node is not in the tree | Refused | `INFO` |
| The node is the root | Refused: the window would be empty | `INFO` |
| The node is already in the asked state | Nothing changes | `debug` |
| An ID breaks the slug rule | Refused | `ERROR` |

## How containers follow

The reference is how Zed's `dock.rs` toggles a panel: the panel's view stays alive, and only whether it is drawn changes.

- **Every container asks its slots for the shown views only.** That covers a split, tabs, and the cards of the test extension.
- **Split.** A split's shares are kept per child, so hiding a child and showing it again gives it back its share. While it is hidden, the other children share the length in the proportions of their own shares.
- **Tabs.**
  - A tabs container remembers its selected child by node ID, not by position.
  - When the selected child is hidden, the first shown child is selected.
  - When it is shown again, it is not selected again on its own.
- The window redraws after every change.

## Logging

- A change runs in a group, `layout-op{op=hide|show, node, extension}`, the group of every operation (feature 8). It is opened where the change is asked for, for example inside a click while the application runs.
- Inside the group, the tree logs `node hidden` or `node shown` at `debug`, or the refusal.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_layout/tree` | Dev version beside production: the hidden flag, `set_hidden`, and whether a node is shown |
| `yard_shell/shell` | Dev version: `Extension::set_hidden`; slots give only shown views; shares and tab selection kept by node ID |
| `yard_ui/axis` | Dev version: shares given per child by the caller. The axis no longer resets them when the number of children changes. |
| `yard/startup` | Dev copy that uses the dev shell |

## Test extension: `examples/layout_tour`

- The card gains a counter: clicking the card's text `count: N` adds one.
- A new component, `layout_tour.controls`, shows one button per node it controls. A button reads `hide b` or `show b`, and calls `set_hidden`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=hide-show cargo run dev` | A row: `controls`, `a`, `b`, each a third. |
| Same, click `b`'s count three times, then `hide b` | `b` goes away; `controls` and `a` share the row in halves. `layout-op{op=hide, node=b}`. |
| Same, then `show b` | `b` is back in its place, still at `count: 3`, and the row is in thirds again. |
| Same, drag `b`'s line before hiding it, then hide and show `b` | `b` gets back the width it had. |
| `LAYOUT_TOUR=hide-tab cargo run dev` | A row: `controls` on the left, and tabs `a`, `b` on the right. Select `b`, count to 2, then `hide b`: tab `b` goes away and `a` is selected. `show b`: tab `b` is back, and clicking it shows `count: 2`. |
| `LAYOUT_TOUR=hide-root cargo run dev` | The tour asks to hide `root` at startup. It is refused with an `INFO`. |

## Under review

- `yard_layout/tree`, `yard_shell/shell`, and `yard_ui/axis`, in their dev versions
- `yard/startup`, as a dev copy

## Not in this feature

- Rules about which extension may hide which node: feature 8.
- Keyboard actions and keybindings.
- Saving what is hidden: features 11 and 12.
- Animations.
