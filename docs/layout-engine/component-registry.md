# Layout feature 1: component-registry

## What it adds

An extension registers a component type and places one node of it as the root of the main window. The window shows that node, filling the window. Without a root, the window shows only the theme's background, as it does now.

## Model

- A **component type** has an ID and a way to create its view.
  - The ID is a namespaced slug, `<extension>.<name>`, such as `layout_tour.card`.
  - Each of the two parts holds lowercase ASCII letters, digits, `-`, and `_`, and starts with a letter.
  - The way to create the view is a function that receives the node's ID, and returns the view.
- A **node** is a placed instance of a component type. It has an ID and its component type.
  - A node ID is a slug, with the same rule as one part of a component ID.
- The **tree** of the main window has at most one node in this feature: its root.

## Rules

| Situation | What happens | Log |
|---|---|---|
| A component ID or node ID breaks the slug rule | Refused; nothing is registered or placed | `ERROR`: a bug of the extension |
| A component ID is registered twice | The second is refused; the first stays | `ERROR` |
| A root is placed when there is one already | The second is refused; the first stays | `INFO`: an expected refusal |
| The root names a component type that nobody registered | The window shows only the background | `ERROR`, once when the window opens |

## Lifecycle

- Every extension's `init` runs before the main window opens. This changes the order of the startup, which today opens the window first and loads the extensions after it. It is the order Zed uses: everything is registered, then the window opens.
- The root's view is created once, when the window opens, and kept while the window is open.

## Where the code lives

- **`yard_layout`**, a new crate without GPUI, holds the model: the IDs and their rule, the node, and the tree with its root.
  - It is the part `yard_layout/tree`.
  - It is tested without a window.
- **`yard_shell`**, a new crate, binds the model to GPUI.
  - It holds the registry of component types and the window's root view, which shows the root node.
  - Its API for extensions is two functions: register a component type, and place the root.
  - It is the part `yard_shell/shell`.
- **`yard/startup`** changes through a dev copy, as in the earlier foundations:
  - the extensions load before the window opens;
  - the window's root view comes from `yard_shell`.
- **`yard_ui/theming`** gains the color `text`, which the test card needs to write its ID. This is a change to a production part. It is stamped again only when you approve it.

## Logging

All the groups below are inside `app-startup`.

| Group | Fields | Inside |
|---|---|---|
| `component-register` | `component` | the extension's `init` |
| `root-place` | `node`, `component` | the extension's `init` |
| `component-create` | `node`, `component` | `window-open`, when the root's view is created |

A refusal is logged inside the group of the work that was refused.

## Test extension: `examples/layout_tour`

The case is chosen with `LAYOUT_TOUR`. Without it, the extension does nothing, and the window is empty.

The card shows its node ID and its own size in pixels. It writes in the theme's text color, on the theme's background. The size is measured from the card's real bounds, not from the window, so it changes when the window is resized.

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=card cargo run dev` | One card fills the window and shows `root` and its size. Resizing the window changes the size. In the trace: `component-register`, then `root-place`, then `component-create` inside `window-open`. |
| `LAYOUT_TOUR=missing cargo run dev` | The root names `layout_tour.missing`, which is not registered. The window shows only the background, with an `ERROR` in `window-open`. |
| `LAYOUT_TOUR=twice cargo run dev` | Two roots are placed. The second is refused with an `INFO`, and the first card is shown. |
| `LAYOUT_TOUR=bad-id cargo run dev` | A component ID that breaks the slug rule, `Layout Tour.Card`, is refused with an `ERROR`, and the window is empty. |

## Under review

`yard_layout/tree`, `yard_shell/shell`, the dev copy of `yard/startup`, and the change to `yard_ui/theming`. They are reviewed and promoted together, as one feature.

## Not in this feature

- Slots, and a node inside another node: feature 2.
- One extension placing a node in a component it does not own: feature 3.
- Props.
- Several windows.
- Saving the layout.
